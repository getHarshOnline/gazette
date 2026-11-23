//! Configured output repository metadata, separate from detached release preparations.

use super::{ParentSource, PublicationDestination, RepositorySetupLimits};
use crate::{destination::OutputBinding, source::CommitId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Independently selected local state required before a configured checkout changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputCheckoutState {
    /// The root may be absent, but a root Git entry must not exist.
    Missing,
    /// Existing independent repository with no refs, objects or staged entries.
    Unborn,
    /// Exact current configured branch commit; only a fast-forward may replace it.
    Commit {
        /// Exact current configured branch commit.
        oid: CommitId,
    },
}

/// Complete local history selected for the remote branch's exact observed parent.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputCheckoutParent {
    /// Verified independent bare or Gazette preparation source.
    pub source: ParentSource,
    /// Exact observed remote commit, including all its ancestry.
    pub commit: CommitId,
}

/// Metadata-only setup; generated and unowned working files are never checked out or removed.
pub struct OutputCheckoutRequest<'a> {
    /// Exact configured output directory, never discovered from a current directory.
    pub root: &'a Path,
    /// Configured repository/branch, including the workspace's main default.
    pub destination: &'a PublicationDestination,
    /// Expected local state, independent of the selected new parent.
    pub expected: &'a OutputCheckoutState,
    /// None means the remote was independently established unborn, never a read failure.
    pub parent: Option<&'a OutputCheckoutParent>,
    /// Sources, journal and peer outputs must remain separate.
    pub protected_roots: &'a [PathBuf],
}

/// Independent permissions for initial setup and an existing branch's fast-forward.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OutputCheckoutOptions {
    /// Permit a missing repository or a strictly empty/unborn repository to be initialized.
    pub allow_initialize: bool,
    /// Permit an existing exact branch to advance to its verified descendant.
    pub allow_update: bool,
    /// Retained closure, native storage and metadata limits.
    pub limits: RepositorySetupLimits,
}

/// Independently verified local metadata, not output ownership or release approval.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct OutputCheckoutReport {
    /// Native root and .git identity for subsequent guarded output planning.
    pub binding: OutputBinding,
    /// Exact selected remote and branch.
    pub destination: PublicationDestination,
    /// Local configured branch after setup; None is still unborn.
    pub parent: Option<CommitId>,
}
