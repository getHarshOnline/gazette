//! Reviewed-release operations retain the application's private writer lifetime.

use super::Writer;
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseDispatcher, ReleaseWorktrees, RemoteReader, WorktreeReader},
    store::{
        PreparationReader, PreparationWriter, ReleaseAttemptWriter, ReleaseAuthorizationWriter,
        ReleaseConsumptionWriter, ReleaseReconciliationWriter,
    },
};

impl<W: ReleaseAuthorizationWriter> Writer<W> {
    /// Register the exact reviewed output without consuming it or starting delivery.
    ///
    /// # Errors
    /// Preserves verification, custody, conflict and uncertain-write evidence.
    pub fn register_reviewed_approval<
        F: RootSeparation + WorktreeReader,
        G: ReleaseWorktrees,
        S: PreparationReader,
    >(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        request: crate::release::PreparedReviewedApprovalRequest<'_>,
        options: crate::release::RegisterAuthorizationOptions,
    ) -> Result<
        gazette_core::store::ReleaseAuthorizationReceipt,
        crate::release::RegisterAuthorizationError,
    > {
        crate::release::register_reviewed_approval(
            filesystem,
            git,
            store,
            &mut self.0,
            request,
            options,
        )
    }
}

impl<W: ReleaseConsumptionWriter> Writer<W> {
    /// Consume reviewed approval after the complete set of leases and prestate checks.
    ///
    /// # Errors
    /// Retains earlier read/lease evidence and any uncertain local transition.
    #[allow(clippy::too_many_arguments)]
    pub fn consume_reviewed_approval<
        F: RootSeparation + WorktreeReader,
        G: ReleaseWorktrees,
        S: PreparationReader,
        R: RemoteReader,
    >(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        reader: &R,
        request: crate::release::PreparedReviewedApprovalRequest<'_>,
        options: crate::release::ConsumeAuthorizationOptions,
    ) -> Result<
        gazette_core::store::ReleaseConsumptionReceipt,
        crate::release::ConsumeAuthorizationError,
    > {
        crate::release::consume_reviewed_approval(
            filesystem,
            git,
            store,
            &mut self.0,
            reader,
            request,
            options,
        )
    }
}

impl<W: ReleaseReconciliationWriter + ReleaseAttemptWriter + PreparationWriter> Writer<W> {
    /// Dispatch one newly recorded attempt and retain independent target read-back.
    ///
    /// # Errors
    /// Preserves start/result uncertainty; existing attempts never repeat effects.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_reviewed_attempt<
        F: RootSeparation + WorktreeReader,
        G: ReleaseWorktrees,
        S: PreparationReader,
        R: RemoteReader,
        D: ReleaseDispatcher,
    >(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        reader: &R,
        dispatcher: &D,
        request: crate::release::DispatchReviewedAttemptRequest<'_>,
        options: crate::release::DispatchAttemptOptions,
    ) -> Result<gazette_core::store::ReleaseAttempt, crate::release::DispatchAttemptError> {
        crate::release::dispatch_reviewed_attempt(
            filesystem,
            git,
            store,
            &mut self.0,
            reader,
            dispatcher,
            request,
            options,
        )
    }
}
