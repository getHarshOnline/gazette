//! Plain approval uses the existing private writer and independently selected publisher routing.

use super::*;
use crate::release;
use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;

impl Writer {
    /// Record confirmed output using independently resolved workspace publisher scopes.
    ///
    /// # Errors
    /// Refuses changed scope, denied registration, native mismatch or uncertain history delivery.
    pub fn register_reviewed_approval(
        &mut self,
        scope: &release::ReviewedScope,
        request: release::PreparedReviewedApprovalRequest<'_>,
        options: release::RegisterAuthorizationOptions,
    ) -> Result<ReleaseAuthorizationReceipt, release::RegisterAuthorizationError> {
        scope
            .check_binding(request.binding, request.preparations.selections)
            .map_err(|_| {
                release::RegisterAuthorizationError::Verification(
                    release::PreparedAuthorizationError::Mismatch,
                )
            })?;
        self.0
            .register_reviewed_approval(&FileSystem, &Git, &Sqlite, request, options)
    }

    /// Consume the complete confirmed set after reads through each publisher's selected credential.
    ///
    /// # Errors
    /// Preserves earlier leases/readbacks and any uncertain consumption; never dispatches.
    pub fn consume_reviewed_approval(
        &mut self,
        scope: &release::ReviewedScope,
        git: &release::GitSelection,
        request: release::PreparedReviewedApprovalRequest<'_>,
        options: release::ConsumeAuthorizationOptions,
    ) -> Result<ReleaseConsumptionReceipt, release::ConsumeAuthorizationError> {
        scope
            .check_binding(request.binding, request.preparations.selections)
            .map_err(|_| release::ConsumeAuthorizationError {
                cause: release::ConsumeFailure::Verification(
                    release::PreparedAuthorizationError::Mismatch,
                ),
                leases: None,
                observations: None,
            })?;
        self.0.consume_reviewed_approval(
            &FileSystem,
            &Git,
            &Sqlite,
            &scope.transport(git),
            request,
            options,
        )
    }

    /// Dispatch one recorded attempt with the exact selected publisher credential and read-back.
    ///
    /// # Errors
    /// Refuses scope changes or uncertain previous effects; exact replay never dispatches again.
    pub fn dispatch_reviewed_attempt(
        &mut self,
        scope: &release::ReviewedScope,
        git: &release::GitSelection,
        request: release::DispatchReviewedAttemptRequest<'_>,
        options: release::DispatchAttemptOptions,
    ) -> Result<ReleaseAttempt, release::DispatchAttemptError> {
        scope
            .check_binding(
                request.reviewed.binding,
                request.reviewed.preparations.selections,
            )
            .map_err(|_| release::DispatchAttemptError {
                cause: release::DispatchAttemptFailure::Verification(
                    release::PreparedAuthorizationError::Mismatch,
                ),
                prestate: None,
                started: None,
                observed: None,
            })?;
        let transport = scope.transport(git);
        self.0.dispatch_reviewed_attempt(
            &FileSystem,
            &Git,
            &Sqlite,
            &transport,
            &transport,
            request,
            options,
        )
    }

    /// Reconcile the original complete set through independently selected per-publisher credentials.
    ///
    /// # Errors
    /// Retains unavailable/conflicting observations; does not dispatch or renew approval.
    pub fn reconcile_reviewed_release(
        &mut self,
        scope: &release::ReviewedScope,
        git: &release::GitSelection,
        request: release::ReconcileReleaseRequest<'_>,
        options: release::ReconcileReleaseOptions,
    ) -> Result<ReleaseReconciliationReceipt, release::ReconcileReleaseError> {
        self.0
            .reconcile_release(&scope.transport(git), request, options)
    }
}
