//! Exact native dispatch observations; neither transport return nor a lease grants approval.

use super::{CommitMetadata, RemoteState, Target, WorktreeLimits};
use crate::{
    process::Cleanup,
    source::{Sha256Digest, TreeIdentity},
};
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path, time::Duration};

/// One independently prepared target. The application owns durable start and current authority.
#[derive(Clone, Copy)]
pub struct DispatchRequest<'a> {
    /// Exact separately prepared deployment worktree, under trusted exclusive custody.
    pub repository: &'a Path,
    /// Immutable consumed target, not a reconstructed workspace destination.
    pub target: &'a Target,
    /// Original complete commit recipe, verified natively before dispatch.
    pub metadata: &'a CommitMetadata,
    /// Original complete output identity.
    pub output: TreeIdentity,
}

impl DispatchRequest<'_> {
    /// Check the exact single-ref root-creation or fast-forward recipe without native reads.
    /// The provider must additionally verify the actual raw commit and independent worktree.
    ///
    /// # Errors
    /// Rejects incompatible parent recipes, relative roots and altered output/refspec fields.
    pub fn validate(self) -> Result<(), DispatchError> {
        let parent_matches = match &self.target.expected_remote_oid_or_unborn {
            RemoteState::Commit { oid } => {
                self.metadata.parent.as_ref() == Some(oid) && &self.target.local_commit_oid != oid
            }
            RemoteState::Unborn => self.metadata.parent.is_none(),
        };
        if !self.repository.is_absolute()
            || !parent_matches
            || self.target.output_digest != self.output.sha256()
            || self.target.refspec
                != format!(
                    "{}:refs/heads/{}",
                    self.target.local_commit_oid, self.target.branch
                )
            || super::validate_git_destination(&self.target.remote, &self.target.branch).is_err()
        {
            return Err(DispatchError::refused(DispatchFailure::Selection));
        }
        Ok(())
    }
}

/// Explicit native effect mechanism; selecting one never changes publisher authority.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum DispatchProfile {
    /// Native Git receive-pack and its destination policy. Descendants may outlive the caller.
    #[default]
    ReceivePack,
    /// Synchronous, child-free local object transfer and one exact ref transaction.
    /// Only the qualified hook-free local destination profile is admitted.
    LocalTransaction,
}

/// Selected native write permission and bounded provider work, independent from read permission.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DispatchOptions {
    /// Explicit provider profile, retained in the durable start before any dispatch.
    pub profile: DispatchProfile,
    /// Permit exactly the supplied target dispatch; not publisher approval.
    pub allow_dispatch: bool,
    /// Additionally permit qualified network transports; local fixture operation needs none.
    pub allow_network: bool,
    /// Process profile: post-spawn deadline including response and direct-child exit.
    /// Local profile: deadline checked between bounded native phases, not syscall cancellation.
    pub timeout: Duration,
    /// Maximum complete response, never a usable truncated prefix.
    #[schemars(range(max = usize::MAX))]
    pub max_response_bytes: usize,
    /// Native prepared object/history verification bounds.
    pub worktree: WorktreeLimits,
}
impl Default for DispatchOptions {
    fn default() -> Self {
        Self {
            profile: DispatchProfile::default(),
            allow_dispatch: false,
            allow_network: false,
            timeout: Duration::from_secs(30),
            max_response_bytes: 64 << 10,
            worktree: WorktreeLimits::default(),
        }
    }
}
impl DispatchOptions {
    /// Check explicit effect permission and logical process limits before any effect.
    ///
    /// # Errors
    /// Rejects denied dispatch, deadlines outside 1 ms–5 min or responses over 1 MiB.
    pub fn validate(self) -> Result<(), DispatchError> {
        if !self.allow_dispatch {
            return Err(DispatchError::refused(DispatchFailure::Permission));
        }
        if self.timeout < Duration::from_millis(1)
            || self.timeout > Duration::from_secs(300)
            || self.max_response_bytes > 1 << 20
        {
            return Err(DispatchError::refused(DispatchFailure::Limit));
        }
        Ok(())
    }
}

/// What the selected native transport reported, not independently established destination state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchStatus {
    /// The provider reported an accepted update; native read-back remains necessary.
    Updated,
    /// The provider reported the intended ref already present; no old-state CAS is implied.
    AlreadyPresent,
    /// The provider reported rejection; absence of effects still requires read-back.
    Rejected,
}

/// Complete bounded native return; never a release-success receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DispatchObservation {
    /// Exact native report class for the single selected target.
    pub status: DispatchStatus,
    /// Native exit code after reaping; absent for an in-process provider.
    pub exit_code: Option<i32>,
    /// Digest of the complete transport response, without exposing provider text.
    pub response_sha256: Sha256Digest,
    /// Direct-child custody only; `NotStarted` also means the in-process profile spawned none.
    /// This field alone is never proof of provider-descendant quiescence.
    pub cleanup: Cleanup,
}

/// Stable redacted refusal or uncertain-dispatch classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchFailure {
    /// No explicit dispatch or network permission.
    Permission,
    /// Invalid exact target, native path or immutable recipe.
    Selection,
    /// Unsupported native mechanism or unqualified transport.
    Unsupported,
    /// Changed or unverifiable prepared native objects.
    LocalState,
    /// Unsafe or changed isolation state.
    Isolation,
    /// Exhausted input, object or response bound.
    Limit,
    /// Native invocation failed without a complete usable report.
    Transport,
    /// Post-spawn deadline expired.
    Timeout,
    /// Malformed or incompatible complete native response.
    Response,
}

/// Whether a provider dispatch may have occurred. Never inferred from transport exit alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DispatchEffect {
    /// Native dispatch did not start.
    NotStarted,
    /// Dispatch was attempted without independent proof of its effect.
    Unverified,
}

/// Retained provider uncertainty, with no raw native output or automatic retry permission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DispatchError {
    /// Stable failed phase.
    pub failure: DispatchFailure,
    /// Explicit possible-effect classification.
    pub effect: DispatchEffect,
    /// Known direct-child custody.
    pub cleanup: Cleanup,
}
impl DispatchError {
    /// Refusal before starting a provider dispatch.
    pub const fn refused(failure: DispatchFailure) -> Self {
        Self {
            failure,
            effect: DispatchEffect::NotStarted,
            cleanup: Cleanup::NotStarted,
        }
    }
}
impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "release dispatch {:?}; effect {:?}; preserve intent and reconcile before retry",
            self.failure, self.effect
        )
    }
}
impl std::error::Error for DispatchError {}
