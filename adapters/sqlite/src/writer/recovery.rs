//! Restricted reuse of native journal transactions; never a fresh writer generation.

use super::Writer;
use gazette_core::store::{
    Error, JournalInspection, ReleaseCoordinator, ReleaseReconciliation,
    ReleaseReconciliationInspection, ReleaseReconciliationMode, ReleaseReconciliationOptions,
    ReleaseReconciliationReceipt, ReleaseWriteError,
};
use gazette_ports::store::{
    JournalWriter, ReleaseCoordinatorReader, ReleaseReconciliationWriter, ReleaseRecoverySealer,
};
use std::path::Path;

/// Held restricted recovery. The private writer cannot escape or gain dispatch capabilities.
///
/// ```compile_fail
/// fn needs_attempt<W: gazette_ports::store::ReleaseAttemptWriter>() {}
/// needs_attempt::<gazette_adapter_sqlite::Reconciler>();
/// ```
/// ```compile_fail
/// fn needs_consumption<W: gazette_ports::store::ReleaseConsumptionWriter>() {}
/// needs_consumption::<gazette_adapter_sqlite::Reconciler>();
/// ```
pub struct Reconciler {
    writer: Writer,
    allow_anchor_write: bool,
}

impl Writer {
    pub(crate) fn into_reconciler(
        mut self,
        profile: gazette_core::store::RecoveryProfile,
        allow_anchor_write: bool,
    ) -> Result<Reconciler, Error> {
        self.coordinator
            .as_ref()
            .ok_or(Error::CoordinatorMismatch)?;
        self.recovery = Some(profile);
        self.inspect()?;
        Ok(Reconciler {
            writer: self,
            allow_anchor_write,
        })
    }
}

impl JournalWriter for Reconciler {
    fn inspect(&mut self) -> Result<JournalInspection, Error> {
        self.writer.inspect()
    }

    fn close(mut self) -> Result<(), Error> {
        // Ordinary Writer::close would seal clean. Recovery only verifies and drops custody.
        self.writer.inspect().map(|_| ())
    }
}

impl ReleaseRecoverySealer for Reconciler {
    fn seal_recovery(
        mut self,
        allow_seal: bool,
    ) -> Result<gazette_core::store::LocalCoordinatorSeal, Error> {
        if !allow_seal || !self.allow_anchor_write {
            return Err(Error::CoordinatorPermission);
        }
        if self.reconciliation_mode() == ReleaseReconciliationMode::ObservationOnly {
            return Err(Error::ReleaseQuiescenceRequired);
        }
        self.writer.inspect()?;
        crate::with_source(
            &self
                .writer
                .reservation
                .path()
                .join(crate::journal::DATABASE),
            self.writer.options,
            |connection| {
                self.writer.verify_snapshot(connection)?;
                crate::settlement::require_terminal(connection, self.writer.options)
            },
        )?;
        // No SQL connection remains while fingerprinting. Both native reservations
        // stay held through the existing synchronized anchor transition.
        let guard = self
            .writer
            .coordinator
            .as_ref()
            .ok_or(Error::CoordinatorMismatch)?;
        let seal = self.writer.coordinator_seal(guard.binding(), true)?;
        guard.finish(seal.clone()).map_err(|cause| match cause {
            Error::CoordinatorUnverified(_) => cause,
            cause => Error::CoordinatorUnverified(Box::new(cause)),
        })?;
        Ok(seal)
    }
}

impl ReleaseCoordinatorReader for Reconciler {
    fn inspect_release_coordinator(&mut self) -> Result<ReleaseCoordinator, Error> {
        self.writer.inspect_release_coordinator()
    }
}

impl ReleaseReconciliationWriter for Reconciler {
    fn reconciliation_mode(&self) -> ReleaseReconciliationMode {
        self.writer.reconciliation_mode()
    }

    fn inspect_release_reconciliations(
        &mut self,
        database: &Path,
        record_id: &str,
    ) -> Result<ReleaseReconciliationInspection, Error> {
        self.writer
            .inspect_release_reconciliations(database, record_id)
    }

    fn record_release_reconciliation(
        &mut self,
        database: &Path,
        input: &ReleaseReconciliation,
        options: ReleaseReconciliationOptions,
    ) -> Result<ReleaseReconciliationReceipt, ReleaseWriteError> {
        options.validate()?;
        if !self.allow_anchor_write {
            return Err(Error::CoordinatorPermission.into());
        }
        self.writer
            .record_release_reconciliation(database, input, options)
    }
}
