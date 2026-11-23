//! Existing-byte synchronization, separate from creation and publication effects.

use super::WorktreeLimits;
use crate::source::Limits;
use std::fmt;

/// Permission and bounds for synchronizing existing publication output only.
#[derive(Clone, Copy, Debug, Default)]
pub struct WorktreeOutputSyncOptions {
    /// Explicitly permit flushing existing files/directories; never replacement or cleanup.
    pub allow_synchronize: bool,
    /// Complete output observation limits; root `.git` is handled by the Git owner.
    pub limits: Limits,
}

/// Permission and bounds for synchronizing an existing independent Git worktree's metadata.
#[derive(Clone, Copy, Debug)]
pub struct WorktreeSyncOptions {
    /// Explicitly permit flushing existing storage; no init, objects, index or refs are written.
    pub allow_synchronize: bool,
    /// Exact current tree and complete parent-history verification ceilings.
    pub limits: WorktreeLimits,
    /// Maximum existing administrative storage bytes within one synchronization.
    pub storage_bytes: u64,
}

impl Default for WorktreeSyncOptions {
    fn default() -> Self {
        Self {
            allow_synchronize: false,
            limits: Default::default(),
            storage_bytes: 256 << 20,
        }
    }
}

/// Native synchronization failed; existing bytes are never removed or repaired.
#[derive(Debug)]
pub struct WorktreeSyncError {
    /// Static, redacted diagnostic without source bytes or private native paths.
    pub reason: &'static str,
    /// A flush may have started without complete synchronized read-back evidence.
    pub attempted: bool,
}

impl WorktreeSyncError {
    /// Refuse before the first synchronization effect.
    pub const fn refused(reason: &'static str) -> Self {
        Self {
            reason,
            attempted: false,
        }
    }
}

impl fmt::Display for WorktreeSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; synchronization attempted: {}; preserve existing state",
            self.reason, self.attempted
        )
    }
}

impl std::error::Error for WorktreeSyncError {}
