//! Join native Git metadata and complete filesystem output without compiling again.

use gazette_core::{
    release::{
        CommitMetadata, ParentSource, WorktreeEffect, WorktreeError, WorktreeLimits,
        WorktreeOptions, WorktreeReport, validate_worktree_parent,
    },
    source::{CommitId, Inventory, TreeIdentity},
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    source::SourceCapture,
};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Exact retained output and native recipe; no remote or publication permission.
#[derive(Clone, Copy, Debug)]
pub struct WorktreeRequest<'a> {
    /// Existing retained output directory, under trusted exclusive local control.
    pub root: &'a Path,
    /// Independently retained complete output pin, not learned from this operation.
    pub expected: TreeIdentity,
    /// Exact deterministic commit recipe.
    pub metadata: &'a CommitMetadata,
    /// Acquisition source for a pinned parent; not reopened by read-only inspection.
    pub parent_source: Option<&'a ParentSource>,
    /// Every native input and private journal root that must remain separate.
    pub protected_roots: &'a [PathBuf],
}

/// Paired current observations, not an atomic snapshot, certification or release receipt.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct WorktreeInspection {
    /// Verified independent Git metadata, index, HEAD and complete commit.
    pub git: WorktreeReport,
    /// Complete filesystem output, including ignored and untracked files.
    pub output: Inventory,
}

/// Initialize an independent deployment repository without changing its output bytes.
/// The source capture and Git writer share the same retained bytes. This operation
/// synchronizes Git storage; its already-retained filesystem input is not restaged.
/// A durable preparation coordinator must record this exact intent before calling.
///
/// # Errors
/// Refuses permissions, overlap and mismatched input before writes. Later errors
/// retain the exact commit coordinate when known, never recreate or remove residue.
pub fn prepare_worktree<F, G>(
    filesystem: &F,
    git: &G,
    request: WorktreeRequest<'_>,
    options: WorktreeOptions,
) -> Result<WorktreeInspection, WorktreeError>
where
    F: SourceCapture + RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
{
    if !options.allow_initialize {
        return Err(WorktreeError::refused(
            "explicit deployment-worktree initialization is required",
        ));
    }
    validate_worktree_parent(request.metadata.parent.as_ref(), request.parent_source)?;
    let mut protected = request.protected_roots.to_vec();
    if let Some(source) = request.parent_source {
        protected.push(source.path().to_owned());
    }
    filesystem
        .check_root_separation(request.root, &protected)
        .map_err(|_| {
            WorktreeError::refused("deployment worktree is not separate from protected roots")
        })?;
    let captured = crate::source::capture_pinned(
        filesystem,
        request.root,
        request.expected,
        options.limits.output.content,
    )
    .map_err(|_| {
        WorktreeError::refused("complete output capture does not match its retained pin")
    })?;
    filesystem
        .check_root_separation(request.root, &protected)
        .map_err(|_| {
            WorktreeError::refused("deployment worktree separation changed before initialization")
        })?;
    let prepared = git.prepare_worktree(
        request.root,
        captured.as_input(),
        request.metadata,
        request.parent_source,
        options,
    )?;
    let commit = prepared.commit.commit.clone();
    let observed =
        inspect_worktree(filesystem, git, request, &commit, options.limits).map_err(|error| {
            WorktreeError {
                reason: error.reason,
                effect: WorktreeEffect::CommitCreated(commit.clone()),
            }
        })?;
    if observed.git != prepared || observed.output != *captured.inventory() {
        return Err(WorktreeError {
            reason: "native worktree read-back differs from retained preparation inputs",
            effect: WorktreeEffect::CommitCreated(commit),
        });
    }
    Ok(observed)
}

/// Inspect Git state before and after the complete confined output observation.
/// Source separation is required even for this read-only operation. A successful
/// read does not establish that an earlier synchronization or journal append happened.
///
/// # Errors
/// Refuses unsafe Git metadata, changed HEAD/index/objects, changed output,
/// source overlap or bounds without repairing any state.
pub fn inspect_worktree<F: WorktreeReader, G: ReleaseWorktrees>(
    filesystem: &F,
    git: &G,
    request: WorktreeRequest<'_>,
    commit: &CommitId,
    limits: WorktreeLimits,
) -> Result<WorktreeInspection, WorktreeError> {
    let before = git.inspect_worktree(
        request.root,
        commit,
        request.expected,
        request.metadata,
        limits,
    )?;
    let output = filesystem
        .read_worktree(request.root, request.protected_roots, limits.output.content)
        .map_err(|_| {
            WorktreeError::refused("complete worktree output is unsafe, overlapping or unavailable")
        })?;
    if output.identity() != request.expected
        || output.entries().len() != before.commit.entries
        || output.total_bytes() != before.commit.bytes
    {
        return Err(WorktreeError::refused(
            "worktree output differs from the exact committed tree",
        ));
    }
    let after = git.inspect_worktree(
        request.root,
        commit,
        request.expected,
        request.metadata,
        limits,
    )?;
    if before != after {
        return Err(WorktreeError::refused(
            "Git worktree evidence changed during output observation",
        ));
    }
    Ok(WorktreeInspection { git: after, output })
}
