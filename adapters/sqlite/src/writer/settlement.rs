//! Complete-frontier reconciliation CAS under actual original or cleanly acquired custody.

use super::Writer;
use crate::{consumption, journal, native_error, settlement, with_source};
use gazette_core::store::{
    Error, ReleaseReconciliation, ReleaseReconciliationFrontier, ReleaseReconciliationInspection,
    ReleaseReconciliationMode, ReleaseReconciliationOptions, ReleaseReconciliationReceipt,
    ReleaseReconciliationRecord, ReleaseWriteEffect, ReleaseWriteError,
};
use gazette_ports::store::{JournalWriter, ReleaseCoordinatorReader, ReleaseReconciliationWriter};
use rusqlite::TransactionBehavior;
use std::path::Path;

impl Writer {
    fn check_reconciliation_database(&self, database: &Path) -> Result<(), Error> {
        let selected = database
            .parent()
            .filter(|_| database.is_absolute())
            .ok_or(Error::UnsafePath)?
            .canonicalize()
            .map_err(Error::Io)?
            .join(database.file_name().ok_or(Error::UnsafePath)?);
        if selected != self.reservation.path().join(journal::DATABASE) {
            return Err(Error::JournalMismatch);
        }
        Ok(())
    }
}

impl ReleaseReconciliationWriter for Writer {
    fn reconciliation_mode(&self) -> ReleaseReconciliationMode {
        match self.recovery {
            Some(gazette_core::store::RecoveryProfile::ObserveInterrupted) => {
                ReleaseReconciliationMode::ObservationOnly
            }
            _ => ReleaseReconciliationMode::Settlement,
        }
    }

    fn inspect_release_reconciliations(
        &mut self,
        database: &Path,
        record_id: &str,
    ) -> Result<ReleaseReconciliationInspection, Error> {
        self.check_reconciliation_database(database)?;
        self.inspect()?;
        self.failed = true;
        let result = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                self.verify_snapshot(connection)?;
                settlement::read(connection, record_id, self.options)
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(result)
    }

    fn record_release_reconciliation(
        &mut self,
        database: &Path,
        input: &ReleaseReconciliation,
        options: ReleaseReconciliationOptions,
    ) -> Result<ReleaseReconciliationReceipt, ReleaseWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        self.check_reconciliation_database(database)?;
        let maximum = consumption::maximum(self.options)?.min(options.evidence_bytes);
        settlement::payload(input, maximum)?;
        let coordinator = self.inspect_release_coordinator()?;
        self.failed = true;
        let mut effect = ReleaseWriteEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_snapshot(&transaction)?;
            if self.held_release_coordinator()? != coordinator {
                return Err(Error::CoordinatorMismatch);
            }
            let history = settlement::read(&transaction, &input.authorization_id, self.options)?;
            let consumed = history
                .history
                .consumption
                .as_ref()
                .ok_or(Error::ReleaseSettlementVerification)?;
            if coordinator.binding != consumed.leases.coordinator.binding {
                return Err(Error::CoordinatorMismatch);
            }
            let (record, inserted) = if let Some(existing) = history
                .records
                .iter()
                .find(|record| record.reconciliation.number == input.number)
            {
                if existing.reconciliation != *input {
                    return Err(Error::ReleaseSettlementConflict);
                }
                existing.identity(maximum)?;
                (existing.clone(), false)
            } else {
                if input.mode != self.reconciliation_mode() {
                    return Err(Error::ReleaseSettlementVerification);
                }
                if input.coordinator != coordinator {
                    return Err(Error::CoordinatorMismatch);
                }
                let previous = history.records.last();
                if previous.is_some_and(|record| record.settlement.state.is_terminal()) {
                    return Err(Error::ReleaseTerminal);
                }
                let expected = ReleaseReconciliationFrontier::current(
                    &history.history.attempts,
                    previous,
                    maximum,
                )?;
                if input.frontier != expected {
                    return Err(Error::ReleaseSettlementConflict);
                }
                let derived =
                    input.settle(consumed, &history.history.attempts, previous, maximum)?;
                let record = ReleaseReconciliationRecord {
                    reconciliation: input.clone(),
                    settlement: derived,
                };
                record.identity(maximum)?;
                effect = ReleaseWriteEffect::Unverified;
                settlement::insert(&transaction, &record, maximum)?;
                (record, true)
            };
            let stored = settlement::read(&transaction, &input.authorization_id, self.options)?
                .records
                .into_iter()
                .find(|row| row.reconciliation.number == input.number)
                .ok_or(Error::ReleaseSettlementVerification)?;
            if stored != record {
                return Err(Error::ReleaseSettlementVerification);
            }
            self.commit(transaction)?;
            connection
                .close()
                .map_err(|(_, error)| native_error(error))?;
            self.reservation.flush()?;
            if self.held_release_coordinator()? != coordinator {
                return Err(Error::CoordinatorMismatch);
            }
            let observed = with_source(
                &self.reservation.path().join(journal::DATABASE),
                self.options,
                |connection| {
                    self.verify_snapshot(connection)?;
                    settlement::read(connection, &input.authorization_id, self.options)
                },
            )?
            .records
            .into_iter()
            .find(|row| row.reconciliation.number == input.number)
            .ok_or(Error::ReleaseSettlementVerification)?;
            if observed != record {
                return Err(Error::ReleaseSettlementVerification);
            }
            self.verify_namespace()?;
            Ok(ReleaseReconciliationReceipt {
                record: observed,
                inserted,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(ReleaseWriteError { cause, effect }),
        }
    }
}
