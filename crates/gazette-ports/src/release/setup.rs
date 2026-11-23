//! Separate metadata owners for acquired parent history and configured output repositories.

use gazette_core::release::*;

/// Acquire an explicitly pinned branch into fresh independent local object storage.
pub trait ReleaseParentAcquisition {
    /// Bound native capture, re-read the exact remote ref, verify the complete local closure,
    /// and synchronize the retained source. Never substitute an absent parent on failure.
    ///
    /// # Errors
    /// Refuses permissions, changed refs, unsupported storage or exhausted bounds.
    /// Partial local state is retained with process cleanup evidence; no automatic retry.
    fn acquire_parent(
        &self,
        request: ParentAcquisitionRequest<'_>,
        options: ParentAcquisitionOptions,
    ) -> Result<ParentAcquisitionReport, RepositorySetupError>;
}

/// Configure local output Git metadata without changing any generated or unrelated working file.
pub trait OutputCheckout {
    /// Preserve root/.git identity, bind origin and tracking to the configured destination,
    /// and import complete history before a non-forced exact local branch transition.
    /// Existing detached release-worktree custody is not adopted by this operation.
    ///
    /// # Errors
    /// Refuses changed pre-state, divergence, unsafe configuration, shared storage or limits.
    /// Preserve partial local metadata on failure; never reset, clean or retry it.
    fn prepare_output_checkout(
        &self,
        request: OutputCheckoutRequest<'_>,
        options: OutputCheckoutOptions,
    ) -> Result<OutputCheckoutReport, RepositorySetupError>;
}
