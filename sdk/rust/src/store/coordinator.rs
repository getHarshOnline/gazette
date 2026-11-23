//! Explicit trusted-local coordinator selection over the existing journal writer.

use super::{Error, JournalHeader, SessionOptions, Writer, WriterAcquireError, WriterOptions};
use gazette_adapter_sqlite::{LocalCoordinator, Sqlite};
pub use gazette_application::store::coordinator::AdoptionRequest;
pub use gazette_application::store::coordinator::EnrollmentRequest;
pub use gazette_core::store::{
    LocalCoordinatorBinding, LocalCoordinatorSeal, LocalCoordinatorSelection,
    LocalCoordinatorState, LocalCoordinatorTransaction, RecoveryProfile, RestoreAdoptionOptions,
    RestoreAdoptionReceipt, RestoreAdoptionSource, RestoreAdoptionTarget,
};
use std::path::Path;

/// Native selection only; all operation sequencing and lifetime close stay application-owned.
pub(crate) struct Connector<'a> {
    pub(crate) selection: Option<&'a LocalCoordinatorSelection>,
    pub(crate) allow_anchor_write: bool,
}

impl gazette_ports::store::WriterConnector for Connector<'_> {
    type Writer = gazette_adapter_sqlite::Writer;

    fn acquire_writer(
        &self,
        root: &Path,
        expected: JournalHeader,
        options: WriterOptions,
    ) -> Result<Self::Writer, WriterAcquireError> {
        match self.selection {
            Some(selected) => LocalCoordinator::new(
                selected.anchor_root.clone(),
                selected.binding.clone(),
                self.allow_anchor_write,
            )
            .acquire_writer(root, expected, options),
            None => Sqlite.acquire_writer(root, expected, options),
        }
    }
}

/// Enroll a fresh journal with a separate private anchor; never adopt restored history.
///
/// Protected roots must include the caller's sources, output, staging and backup roots.
/// Keep the returned binding independently; an anchor restored together with its journal
/// cannot detect that rollback. This profile assumes one trusted local host and principal.
///
/// # Errors
/// Refuses unsafe/overlapping paths, permissions, nonfresh history or native failure.
/// Preserve uncertain anchor output rather than removing it to retry.
pub fn enroll(
    request: EnrollmentRequest<'_>,
    options: SessionOptions,
    allow_anchor_write: bool,
) -> Result<LocalCoordinatorSeal, Error> {
    gazette_application::store::coordinator::enroll(
        &gazette_adapter_fs::FileSystem,
        &Sqlite,
        request,
        options,
        allow_anchor_write,
    )
}

/// Observe retained control state without resetting an interrupted writer or resampling refs.
///
/// # Errors
/// Refuses permission, mismatched binding, malformed state or failed native checks.
pub fn inspect(
    anchor_root: &Path,
    expected: &LocalCoordinatorBinding,
    options: SessionOptions,
) -> Result<LocalCoordinatorState, Error> {
    gazette_application::store::coordinator::inspect(&Sqlite, anchor_root, expected, options)
}

/// Verify and adopt an exact latest-clean detached restore into a fresh successor root.
/// The old journal and restore remain unchanged. Failure never selects a fallback store.
///
/// # Errors
///
/// Refuses invalid evidence, unsafe or overlapping roots, missing permissions, stale
/// coordinates, nonfresh successor state, or any failed native verification or effect.
pub fn adopt(
    request: AdoptionRequest<'_>,
    options: RestoreAdoptionOptions,
) -> Result<RestoreAdoptionReceipt, Error> {
    gazette_application::store::coordinator::adopt(
        &gazette_adapter_fs::FileSystem,
        &Sqlite,
        request,
        options,
    )
}

/// Acquire the existing writer with an independently bound local rollback guard.
///
/// The anchor is marked in progress before journal mutation and sealed only on successful
/// explicit close, while both native reservations remain held. Dropping the writer leaves
/// reconciliation required. No lease, publisher authority or release consumption follows.
///
/// # Errors
/// Refuses changed journal bytes (even at matching generation), copied/moved bindings,
/// interrupted state, permission or contention. Failure may leave anchor or generation
/// effects; retain original coordinates and inspect, never automatically retry.
pub fn acquire_writer(
    anchor_root: &Path,
    binding: &LocalCoordinatorBinding,
    expected: JournalHeader,
    options: WriterOptions,
    allow_anchor_write: bool,
) -> Result<Writer, WriterAcquireError> {
    let connector =
        LocalCoordinator::new(anchor_root.to_owned(), binding.clone(), allow_anchor_write);
    gazette_application::store::acquire_writer(&connector, &binding.journal_root, expected, options)
        .map(Writer)
}

/// Restricted interrupted-release observation and settlement; no fresh writer or retry.
/// Closing or dropping preserves the in-progress anchor, including new exact evidence.
/// Explicit sealing additionally requires whole-journal terminal history and separate permission.
pub struct Reconciler(gazette_application::store::Writer<gazette_adapter_sqlite::Reconciler>);

impl Reconciler {
    /// Consume recovery custody and seal its exact terminal, child-free history.
    /// A later writer must be acquired separately; this grants no dispatch or generation.
    ///
    /// # Errors
    /// Refuses denied seal/anchor permission, unresolved history or changed custody.
    /// A late error may follow anchor persistence; inspect rather than retry blindly.
    pub fn seal(self, allow_seal: bool) -> Result<LocalCoordinatorSeal, Error> {
        self.0.seal_recovery(allow_seal)
    }

    /// Inspect the exact retained generation without advancing it.
    ///
    /// # Errors
    /// Refuses changed custody, continuity or history incompatible with the recovery profile.
    pub fn inspect(&mut self) -> Result<super::JournalInspection, Error> {
        self.0.inspect()
    }

    /// Reconcile original consumed intent using complete native destination observations.
    /// No authority provider, signing key or dispatch capability is available here.
    ///
    /// # Errors
    /// Preserves unavailable/changed evidence and uncertain local delivery without retry.
    pub fn reconcile_release(
        &mut self,
        git: &crate::release::GitSelection,
        request: crate::release::ReconcileReleaseRequest<'_>,
        options: crate::release::ReconcileReleaseOptions,
    ) -> Result<super::ReleaseReconciliationReceipt, crate::release::ReconcileReleaseError> {
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        self.0.reconcile_release(&reader, request, options)
    }

    /// Observe original reviewed intent through explicit publisher credentials, without approval renewal.
    ///
    /// # Errors
    /// Preserves unavailable destinations and uncertain evidence writes without dispatch or retry.
    pub fn reconcile_reviewed_release(
        &mut self,
        scope: &crate::release::ReviewedScope,
        git: &crate::release::GitSelection,
        request: crate::release::ReconcileReleaseRequest<'_>,
        options: crate::release::ReconcileReleaseOptions,
    ) -> Result<super::ReleaseReconciliationReceipt, crate::release::ReconcileReleaseError> {
        self.0
            .reconcile_release(&scope.transport(git), request, options)
    }

    /// Verify and release reservations without sealing the coordinator clean.
    ///
    /// # Errors
    /// Preserves native inspection failure; uncertain state remains interrupted.
    pub fn close(self) -> Result<(), Error> {
        self.0.close()
    }
}

/// Recover restricted custody under an explicit whole-journal native lifetime profile.
/// Local transactions are child-free. `ObserveInterrupted` admits verified receive-pack
/// history for nonterminal observations only, without claiming effect-lifetime quiescence.
/// Admission does not recover hot SQL state, rewrite history, clear the anchor or grant a retry.
/// A separate explicit seal requires whole-journal terminal verification and is unavailable
/// under `ObserveInterrupted`, even when all current destination refs match the intent.
///
/// `allow_anchor_write` permits subsequent evidence appends and explicitly requested sealing,
/// not admission itself. Reconciliation and sealing each require their separate operation
/// permission. No generation permission is requested or consumed.
///
/// # Errors
/// Refuses permission, null/ahead/changed expectations, unsupported starts or competing custody.
pub fn recover_release(
    anchor_root: &Path,
    binding: &LocalCoordinatorBinding,
    expected: JournalHeader,
    profile: RecoveryProfile,
    options: SessionOptions,
    allow_anchor_write: bool,
) -> Result<Reconciler, Error> {
    let connector =
        LocalCoordinator::new(anchor_root.to_owned(), binding.clone(), allow_anchor_write);
    gazette_application::store::recover_release(
        &connector,
        &binding.journal_root,
        expected,
        profile,
        options,
    )
    .map(Reconciler)
}
