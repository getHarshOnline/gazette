//! Independent deployment-worktree mechanics, not publication approval.

use super::CommitReport;
use crate::source::{CommitId, RevisionLimits};
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Component, Path, PathBuf},
};

/// Explicit local acquisition provenance. The commit recipe owns the parent pin.
/// Neither variant permits fetching, discovery, shared storage or source mutation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ParentSource {
    /// Existing independent bare repository.
    Bare {
        /// Exact real repository root, resolved without ancestor discovery.
        path: PathBuf,
    },
    /// Previously prepared independent Gazette worktree with its owned profile.
    Worktree {
        /// Exact output root containing the independent owned `.git` directory.
        path: PathBuf,
    },
}

impl ParentSource {
    /// Exact selected local path; never a remote or a search starting point.
    pub fn path(&self) -> &Path {
        match self {
            Self::Bare { path } | Self::Worktree { path } => path,
        }
    }

    /// Check the durable locator grammar without touching the filesystem.
    ///
    /// # Errors
    /// Refuses relative, root-only, non-UTF-8, NUL-bearing or non-normal paths.
    /// Native existence, real-directory identity and separation remain effect-bound checks.
    pub fn validate(&self) -> Result<(), WorktreeError> {
        let path = self.path();
        if !path.is_absolute()
            || path.components().count() < 2
            || path
                .components()
                .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
            || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
            || path.to_str().is_none_or(|text| text.contains('\0'))
        {
            return Err(WorktreeError::refused(
                "parent source requires an exact normal absolute local path",
            ));
        }
        Ok(())
    }
}

/// Validate that root selection and pinned-parent acquisition cannot be conflated.
///
/// # Errors
/// A parent requires exactly one explicit source; a root commit forbids a source.
pub fn validate_worktree_parent(
    parent: Option<&CommitId>,
    source: Option<&ParentSource>,
) -> Result<(), WorktreeError> {
    match (parent, source) {
        (None, None) => Ok(()),
        (Some(_), Some(source)) => source.validate(),
        _ => Err(WorktreeError::refused(
            "parent pin and explicit parent source must be selected together",
        )),
    }
}

/// Decoded historical-object ceilings, separate from current publication output.
/// These bound retained payload and traversal, not the native decoder's process memory.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryLimits {
    /// Maximum unique reachable commits, trees and blobs, including queued objects.
    #[schemars(range(max = usize::MAX))]
    pub objects: usize,
    /// Maximum total decoded bytes across the complete unique closure.
    pub bytes: u64,
    /// Maximum decoded size of any one object before its payload is read.
    #[schemars(range(max = usize::MAX))]
    pub object_bytes: usize,
}

impl Default for HistoryLimits {
    fn default() -> Self {
        // Bounded local deployment history; callers must explicitly size larger histories.
        Self {
            objects: 100_000,
            bytes: 256 << 20,
            object_bytes: 64 << 20,
        }
    }
}

/// Independent ceilings for current output and retained parent history.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WorktreeLimits {
    /// Current output inventory and native administrative/traversal metadata.
    pub output: RevisionLimits,
    /// Complete parent commit/tree/blob closure, never just the immediate parent.
    pub history: HistoryLimits,
}

/// Explicit permission and bounds for new repository metadata over retained output.
#[derive(Clone, Copy, Debug)]
pub struct WorktreeOptions {
    /// Permit initializing absent metadata, objects, an index and detached HEAD.
    /// Existing repositories are never adopted or reinitialized.
    pub allow_initialize: bool,
    /// Current output, historical objects and Git metadata limits.
    pub limits: WorktreeLimits,
    /// Maximum native repository bytes per selected source or destination.
    /// Source inspection is read-only; only the new destination is synchronized.
    pub storage_bytes: u64,
}

impl Default for WorktreeOptions {
    fn default() -> Self {
        Self {
            allow_initialize: false,
            limits: Default::default(),
            storage_bytes: 256 << 20,
        }
    }
}

/// Current native Git evidence; filesystem output requires its separate reader.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct WorktreeReport {
    /// Complete commit verified against the recipe, index and detached HEAD.
    pub commit: CommitReport,
}

/// Native initialization effects survive an unsuccessful return.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum WorktreeEffect {
    /// No metadata write attempted; pre-existing files may still exist.
    NotAttempted,
    /// Repository initialization or a later metadata write was attempted.
    MetadataMayRemain,
    /// This commit was created before a later phase failed.
    CommitCreated(CommitId),
}

/// Redacted failure preserving native residue without automatic cleanup or retry.
#[derive(Debug)]
pub struct WorktreeError {
    /// Static explanation; contains no source bytes or private recipe fields.
    pub reason: &'static str,
    /// Last known mutation boundary, not proof of rollback.
    pub effect: WorktreeEffect,
}

impl WorktreeError {
    /// Refuse before attempting metadata writes.
    pub fn refused(reason: &'static str) -> Self {
        Self {
            reason,
            effect: WorktreeEffect::NotAttempted,
        }
    }
}

impl fmt::Display for WorktreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; worktree effect {:?}; preserve and inspect before retrying",
            self.reason, self.effect
        )
    }
}

impl std::error::Error for WorktreeError {}
