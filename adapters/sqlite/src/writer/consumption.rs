//! Held anchored custody, complete local leases, then one atomic consumption.

use super::Writer;
use crate::{authorization, consumption, journal, native_error, with_source};
use gazette_core::store::{
    Error, ReleaseConsumption, ReleaseConsumptionInput, ReleaseConsumptionInspection,
    ReleaseConsumptionOptions, ReleaseConsumptionReceipt, ReleaseCoordinator, ReleaseLeaseOptions,
    ReleaseLeases, ReleaseWriteEffect, ReleaseWriteError,
};
use gazette_ports::store::{JournalWriter, ReleaseConsumptionWriter, ReleaseCoordinatorReader};
use rusqlite::TransactionBehavior;
use std::path::Path;

impl Writer {
    pub(super) fn held_release_coordinator(&self) -> Result<ReleaseCoordinator, Error> {
        self.verify_namespace()?;
        let guard = self
            .coordinator
            .as_ref()
            .ok_or(Error::CoordinatorMismatch)?;
        guard.verify()?;
        let binding = guard.binding();
        if binding.journal_root != self.reservation.path()
            || binding.journal_id != self.header.journal_id
            || binding.journal_epoch != self.header.journal_epoch
        {
            return Err(Error::CoordinatorMismatch);
        }
        Ok(ReleaseCoordinator {
            binding: binding.clone(),
            journal: self.header,
        })
    }
}

impl ReleaseCoordinatorReader for Writer {
    fn inspect_release_coordinator(&mut self) -> Result<ReleaseCoordinator, Error> {
        self.inspect()?;
        self.held_release_coordinator()
    }
}

impl ReleaseConsumptionWriter for Writer {
    fn inspect_release_consumption(
        &mut self,
        record_id: &str,
    ) -> Result<ReleaseConsumptionInspection, Error> {
        self.inspect()?;
        self.failed = true;
        let result = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                let inspection = self.verify_snapshot(connection)?;
                let record = consumption::read(connection, record_id, self.header, self.options)?;
                Ok(ReleaseConsumptionInspection {
                    journal: inspection.journal,
                    record,
                })
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(result)
    }

    fn acquire_release_leases(
        &mut self,
        record_id: &str,
        options: ReleaseLeaseOptions,
    ) -> Result<ReleaseLeases, ReleaseWriteError> {
        options.validate()?;
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
            let registration =
                authorization::read(&transaction, record_id, self.header, self.options)?
                    .ok_or(Error::ReleaseAuthorizationVerification)?;
            if *registration.binding.journal() != self.header {
                return Err(Error::JournalMismatch);
            }
            if consumption::consumed(&transaction, record_id)? {
                return Err(Error::ReleaseAlreadyConsumed);
            }
            let leases =
                match consumption::read_leases(&transaction, record_id, self.header, self.options)?
                {
                    Some(existing) => {
                        if existing.coordinator != coordinator {
                            return Err(Error::ReleaseLeaseVerification);
                        }
                        consumption::current_leases(&transaction, &existing)?;
                        existing
                    }
                    None => {
                        let leases = consumption::next_leases(
                            &transaction,
                            &registration,
                            coordinator.clone(),
                            self.options,
                        )?;
                        effect = ReleaseWriteEffect::Unverified;
                        consumption::insert_leases(&transaction, &leases, self.options)?;
                        leases
                    }
                };
            let stored =
                consumption::read_leases(&transaction, record_id, self.header, self.options)?
                    .ok_or(Error::ReleaseLeaseVerification)?;
            if stored != leases {
                return Err(Error::ReleaseLeaseVerification);
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
                    consumption::current_leases(connection, &stored)?;
                    consumption::read_leases(connection, record_id, self.header, self.options)
                },
            )?
            .ok_or(Error::ReleaseLeaseVerification)?;
            if observed != stored {
                return Err(Error::ReleaseLeaseVerification);
            }
            self.verify_namespace()?;
            Ok(observed)
        })();
        match result {
            Ok(leases) => {
                self.failed = false;
                Ok(leases)
            }
            Err(cause) => Err(ReleaseWriteError { cause, effect }),
        }
    }

    fn consume_release_authorization(
        &mut self,
        database: &Path,
        input: ReleaseConsumptionInput<'_>,
        options: ReleaseConsumptionOptions,
    ) -> Result<ReleaseConsumptionReceipt, ReleaseWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        let selected = database
            .parent()
            .filter(|_| database.is_absolute())
            .ok_or(Error::UnsafePath)?
            .canonicalize()
            .map_err(Error::Io)?
            .join(database.file_name().ok_or(Error::UnsafePath)?);
        if selected != self.reservation.path().join(journal::DATABASE) {
            return Err(Error::JournalMismatch.into());
        }
        // Bound the full borrowed request before core validation clones its body or targets.
        consumption::encode(&input, self.options)?;
        let record = ReleaseConsumption::from_input(input)?;
        consumption::encode(&record, self.options)?;
        let coordinator = self.inspect_release_coordinator()?;
        if record.leases.coordinator != coordinator || record.created != self.header {
            return Err(Error::CoordinatorMismatch.into());
        }
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
            let registration = authorization::read(
                &transaction,
                &record.authorization_id,
                self.header,
                self.options,
            )?
            .ok_or(Error::ReleaseAuthorizationVerification)?;
            if &registration != input.registration || *registration.binding.journal() != self.header
            {
                return Err(Error::ReleaseConsumptionVerification);
            }
            if consumption::consumed(&transaction, &record.authorization_id)? {
                return Err(Error::ReleaseAlreadyConsumed);
            }
            let leases = consumption::read_leases(
                &transaction,
                &record.authorization_id,
                self.header,
                self.options,
            )?
            .ok_or(Error::ReleaseLeaseVerification)?;
            if leases != record.leases {
                return Err(Error::ReleaseLeaseVerification);
            }
            consumption::current_leases(&transaction, &leases)?;
            record.validate(&registration)?;
            effect = ReleaseWriteEffect::Unverified;
            consumption::insert_consumption(&transaction, &record, self.options)?;
            let stored = consumption::read(
                &transaction,
                &record.authorization_id,
                self.header,
                self.options,
            )?
            .ok_or(Error::ReleaseConsumptionVerification)?;
            if stored != record {
                return Err(Error::ReleaseConsumptionVerification);
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
                    consumption::read(
                        connection,
                        &record.authorization_id,
                        self.header,
                        self.options,
                    )
                },
            )?
            .ok_or(Error::ReleaseConsumptionVerification)?;
            if observed != stored {
                return Err(Error::ReleaseConsumptionVerification);
            }
            self.verify_namespace()?;
            Ok(ReleaseConsumptionReceipt {
                journal: self.header,
                record: observed,
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
