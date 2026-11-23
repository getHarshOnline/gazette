//! Read-only preparation recovery evidence; never repair or completion synthesis.

use super::{WorktreeInspection, WorktreeRequest, inspect_worktree};
use gazette_core::{
    destination,
    release::{WorktreeError, WorktreeLimits},
    source::{CommitId, Sha256Digest},
    store::{self, JournalHeader, PreparationInspection, ReadOptions},
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    store::PreparationReader,
};
use serde::Serialize;
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Exact historical selection and independently retained native commit coordinate.
#[derive(Clone, Copy, Debug)]
pub struct JournalWorktreeInspectionRequest<'a> {
    /// Existing private journal database file, not its containing directory.
    pub database: &'a Path,
    /// Exact immutable preparation identity; no latest-record discovery.
    pub preparation_id: Sha256Digest,
    /// Independently retained expectation, not a guarantee against stale backups.
    pub expected_header: Option<&'a JournalHeader>,
    /// Exact known commit, never inferred from HEAD or a missing completion record.
    pub commit: &'a CommitId,
    /// Every configured native input root that must remain separate.
    pub protected_roots: &'a [PathBuf],
}

/// Bounded inspection with explicit `SQLite` coordination-file permission only.
#[derive(Clone, Copy, Debug, Default, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct JournalWorktreeInspectionOptions {
    /// Applied independently to each of the two verified journal read snapshots.
    pub read: ReadOptions,
    /// Bounds the existing complete filesystem and Git inspection operations.
    pub worktree: WorktreeLimits,
}

/// Historical and current evidence observed in a stable bracket, not an atomic snapshot.
///
/// No synchronization, journal append, migration, repair, certification or release
/// occurs. A missing completion remains missing, even when the current commit is
/// verified. Matching history cannot prove anti-rollback or earlier durability.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct JournalWorktreeInspection {
    /// Verified original history; includes private paths and evidence, not public output.
    pub history: PreparationInspection,
    /// Current Git and complete output observations using only the recorded recipe.
    pub worktree: WorktreeInspection,
}

/// Refusal returns no joined evidence and never implies that earlier effects were absent.
#[derive(Debug)]
pub enum JournalWorktreeInspectionError {
    /// Native store/schema/integrity/permission failure.
    Journal(store::Error),
    /// Journal or worktree cannot be separated from configured protected roots.
    Separation(destination::Error),
    /// No record exists for the exact supplied preparation identity.
    MissingPreparation,
    /// No durable stage-retention observation exists.
    MissingRetention,
    /// No durable Git recipe exists; do not derive one from native state.
    MissingWorktree,
    /// Recorded inputs, native output or an existing completion disagree.
    Mismatch,
    /// History or provider evidence changed across the inspection bracket.
    Changed,
    /// Exact native Git/filesystem verification failed without repair.
    Worktree(WorktreeError),
}

impl JournalWorktreeInspectionError {
    /// Stable diagnostic category without private paths, recipes or evidence bytes.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Journal(error) => error.code(),
            Self::Separation(_) => "preparation_separation",
            Self::MissingPreparation => "preparation_missing",
            Self::MissingRetention => "preparation_retention_missing",
            Self::MissingWorktree => "preparation_worktree_missing",
            Self::Mismatch => "preparation_mismatch",
            Self::Changed => "preparation_changed",
            Self::Worktree(_) => "preparation_worktree_verification",
        }
    }
}

impl fmt::Display for JournalWorktreeInspectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; no joined preparation/worktree evidence returned; preserve native state",
            self.code()
        )
    }
}

impl std::error::Error for JournalWorktreeInspectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Journal(error) => Some(error),
            Self::Separation(error) => Some(error),
            Self::Worktree(error) => Some(error),
            _ => None,
        }
    }
}

/// Join immutable local preparation history to independently inspected current bytes.
///
/// Uses the journal's address, output and recipe; callers cannot substitute them.
/// The original parent acquisition path is provenance only and is not reopened.
/// Requires trusted exclusive local namespace control. Two matching observations
/// do not detect every intervening change or create a cross-store atomic snapshot.
/// `SQLite` may maintain explicitly permitted WAL/SHM coordination files; no SQL or
/// Git write, writer acquisition, synchronization or completion append is performed.
///
/// # Errors
/// Refuses missing phases, mismatched intent/commit/output, unsafe overlap, bounds,
/// native failures or changed history. Missing completion never proves no effect
/// and never makes reinitializing the worktree safe.
pub fn inspect_journal_worktree<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: JournalWorktreeInspectionRequest<'_>,
    options: JournalWorktreeInspectionOptions,
) -> Result<JournalWorktreeInspection, JournalWorktreeInspectionError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    use JournalWorktreeInspectionError as Error;
    options.read.validate().map_err(Error::Journal)?;
    let journal_root = request
        .database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    // Even read-only SQL may maintain sidecars, so protect inputs before connection.
    filesystem
        .check_root_separation(journal_root, request.protected_roots)
        .map_err(Error::Separation)?;
    let history = crate::store::inspect_preparation(
        store,
        request.database,
        request.preparation_id,
        request.expected_header,
        options.read,
    )
    .map_err(Error::Journal)?;
    let record = history.record.as_ref().ok_or(Error::MissingPreparation)?;
    let retention = record.retention.as_ref().ok_or(Error::MissingRetention)?;
    let recipe = record.worktree.as_ref().ok_or(Error::MissingWorktree)?;
    let intent = &record.intent;
    if intent.id != request.preparation_id
        || recipe.check_sha256 != record.check.as_ref().map(|check| check.evidence_sha256)
        || retention.intent_id != intent.id
        || retention.output != intent.output
        || retention.entries != intent.entries
        || retention.bytes != intent.bytes
        || recipe
            .completion
            .as_ref()
            .is_some_and(|done| done.report.commit != *request.commit)
    {
        return Err(Error::Mismatch);
    }
    let root = intent.address.path();
    let mut protected = request.protected_roots.to_vec();
    protected.push(journal_root.to_owned());
    filesystem
        .check_root_separation(&root, &protected)
        .map_err(Error::Separation)?;
    let worktree = inspect_worktree(
        filesystem,
        git,
        WorktreeRequest {
            root: &root,
            expected: intent.output,
            metadata: &recipe.metadata,
            parent_source: None,
            protected_roots: &protected,
        },
        request.commit,
        options.worktree,
    )
    .map_err(Error::Worktree)?;
    if worktree.git.commit.entries as u64 != intent.entries
        || worktree.git.commit.bytes != intent.bytes
        || recipe
            .completion
            .as_ref()
            .is_some_and(|done| done.report != worktree.git.commit)
    {
        return Err(Error::Mismatch);
    }
    filesystem
        .check_root_separation(journal_root, request.protected_roots)
        .map_err(Error::Separation)?;
    let after = crate::store::inspect_preparation(
        store,
        request.database,
        request.preparation_id,
        None,
        options.read,
    )
    .map_err(Error::Journal)?;
    if after.record != history.record
        || after.journal.header != history.journal.header
        || after.journal.schema_sha256 != history.journal.schema_sha256
        || after.journal.verifier != history.journal.verifier
    {
        return Err(Error::Changed);
    }
    Ok(JournalWorktreeInspection {
        history: after,
        worktree,
    })
}
