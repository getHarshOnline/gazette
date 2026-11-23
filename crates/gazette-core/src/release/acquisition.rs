//! Explicit parent acquisition before preparation; never a release or an inferred root commit.

use super::{HistoryLimits, ParentSource, RemoteReadOptions};
use crate::{process::Cleanup, source::CommitId};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

/// Admission limits for independent local Git storage and its decoded history.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RepositorySetupLimits {
    /// Complete reachable commit/tree/blob closure.
    pub history: HistoryLimits,
    /// Native storage admission after acquisition, not a streaming fetch ceiling.
    pub storage_bytes: u64,
    /// Traversal, configuration and decoded commit/tree metadata budget.
    pub metadata_bytes: usize,
}
impl Default for RepositorySetupLimits {
    fn default() -> Self {
        Self {
            history: Default::default(),
            storage_bytes: 256 << 20,
            metadata_bytes: 64 << 20,
        }
    }
}

/// Exact independently observed branch and absent private destination for its retained history.
pub struct ParentAcquisitionRequest<'a> {
    /// Fresh local bare repository coordinate; existing state is never adopted.
    pub root: &'a Path,
    /// Explicit remote selected from the publication destination.
    pub remote: &'a str,
    /// Literal branch below refs/heads.
    pub branch: &'a str,
    /// Exact expected remote commit; failed acquisition never becomes a root commit.
    pub expected: &'a CommitId,
    /// Source, journal, output and other paths this operation must not overlap.
    pub protected_roots: &'a [std::path::PathBuf],
}

/// Separate local-write, remote-read/network and bounded-provider permissions.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ParentAcquisitionOptions {
    /// Permit creating the fresh local repository and acquiring objects into it.
    pub allow_acquire: bool,
    /// Permit native reads and, separately, network access; bounds apply to each call.
    pub read: RemoteReadOptions,
    /// Post-fetch storage admission and decoded closure limits.
    pub limits: RepositorySetupLimits,
}

/// Independently read-back parent history suitable for the existing preparation source.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ParentAcquisitionReport {
    /// Independent local retained repository; no shared objects or shallow boundary.
    pub source: ParentSource,
    /// Complete parent closure verified locally and advertised again by the remote.
    pub parent: CommitId,
}

/// Local residue is preserved after any attempted metadata or object write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum RepositorySetupEffect {
    /// Refused before any local setup write.
    NotAttempted,
    /// The selected local root may contain partial metadata or objects.
    LocalStateMayRemain,
}

/// Redacted setup failure; neither cleanup nor a successful retry is implied.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RepositorySetupError {
    /// Static failure explanation, without remote credentials or file contents.
    pub reason: &'static str,
    /// Last native write boundary.
    pub effect: RepositorySetupEffect,
    /// Process cleanup evidence when a native provider was invoked.
    pub cleanup: Option<Cleanup>,
}
impl RepositorySetupError {
    /// Construct a refusal before local writes or process invocation.
    pub fn refused(reason: &'static str) -> Self {
        Self {
            reason,
            effect: RepositorySetupEffect::NotAttempted,
            cleanup: None,
        }
    }
}
impl fmt::Display for RepositorySetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; local effect {:?}; preserve and inspect before retrying",
            self.reason, self.effect
        )
    }
}
impl std::error::Error for RepositorySetupError {}
