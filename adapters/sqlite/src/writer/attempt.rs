//! Original-holder attempt appends; a replayed start never grants another dispatch.

use super::Writer;
use crate::{attempt, authorization, consumption, journal, native_error, settlement, with_source};
use gazette_core::store::{
    Error, ReleaseAttempt, ReleaseAttemptInspection, ReleaseAttemptKey, ReleaseAttemptOptions,
    ReleaseAttemptReceipt, ReleaseAttemptResult, ReleaseAttemptStart, ReleaseWriteEffect,
    ReleaseWriteError,
};
use gazette_ports::store::{JournalWriter, ReleaseAttemptWriter, ReleaseCoordinatorReader};
use rusqlite::{Connection, TransactionBehavior};
use std::path::Path;

enum AttemptPhase {
    Start,
    Result,
}

impl Writer {
    fn append_attempt(
        &mut self,
        key: &ReleaseAttemptKey,
        phase: AttemptPhase,
        mutate: impl FnOnce(
            &Connection,
            &ReleaseAttemptInspection,
            &mut ReleaseWriteEffect,
        ) -> Result<bool, Error>,
    ) -> Result<ReleaseAttemptReceipt, ReleaseWriteError> {
        let coordinator = self.inspect_release_coordinator()?;
        self.failed = true;
        let mut effect = ReleaseWriteEffect::NotAttempted;
        let outcome = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_snapshot(&transaction)?;
            if self.held_release_coordinator()? != coordinator {
                return Err(Error::CoordinatorMismatch);
            }
            let settled = settlement::read(&transaction, &key.authorization_id, self.options)?;
            let terminal = settled
                .records
                .last()
                .is_some_and(|record| record.settlement.state.is_terminal());
            let history = settled.history;
            let consumed = history
                .consumption
                .as_ref()
                .ok_or(Error::ReleaseAttemptVerification)?;
            if consumed.created != self.header || consumed.leases.coordinator != coordinator {
                return Err(Error::CoordinatorMismatch);
            }
            let replay = history
                .attempts
                .iter()
                .find(|row| &row.start.key == key)
                .is_some_and(|row| matches!(phase, AttemptPhase::Start) || row.result.is_some());
            if !replay {
                if terminal {
                    return Err(Error::ReleaseTerminal);
                }
                consumption::current_leases(&transaction, &consumed.leases)?;
            }
            let created = mutate(&transaction, &history, &mut effect)?;
            let recorded = attempt::read(&transaction, &key.authorization_id, self.options)?;
            let record = recorded
                .attempts
                .into_iter()
                .find(|row| &row.start.key == key)
                .ok_or(Error::ReleaseAttemptVerification)?;
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
                    if created {
                        consumption::current_leases(connection, &consumed.leases)?;
                    }
                    attempt::read(connection, &key.authorization_id, self.options)
                },
            )?;
            let observed = observed
                .attempts
                .into_iter()
                .find(|row| &row.start.key == key)
                .ok_or(Error::ReleaseAttemptVerification)?;
            if observed != record {
                return Err(Error::ReleaseAttemptVerification);
            }
            self.verify_namespace()?;
            Ok(ReleaseAttemptReceipt {
                record: observed,
                created,
            })
        })();
        match outcome {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(ReleaseWriteError { cause, effect }),
        }
    }
}

impl ReleaseAttemptWriter for Writer {
    fn inspect_release_attempts(
        &mut self,
        record_id: &str,
    ) -> Result<ReleaseAttemptInspection, Error> {
        self.inspect()?;
        self.failed = true;
        let result = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                self.verify_snapshot(connection)?;
                attempt::read(connection, record_id, self.options)
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(result)
    }

    fn start_release_attempt(
        &mut self,
        database: &Path,
        input: &ReleaseAttemptStart,
        options: ReleaseAttemptOptions,
    ) -> Result<ReleaseAttemptReceipt, ReleaseWriteError> {
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
        consumption::encode(input, self.options)?;
        let bounds = self.options;
        self.append_attempt(
            &input.key,
            AttemptPhase::Start,
            |connection, history, effect| {
                let consumed = history
                    .consumption
                    .as_ref()
                    .ok_or(Error::ReleaseAttemptVerification)?;
                let registration = authorization::read(
                    connection,
                    &input.key.authorization_id,
                    consumed.created,
                    bounds,
                )?
                .ok_or(Error::ReleaseAttemptVerification)?;
                input.validate(&registration, consumed)?;
                if let Some(existing) = history
                    .attempts
                    .iter()
                    .find(|row| row.start.key == input.key)
                {
                    return if existing.start == *input {
                        Ok(false)
                    } else {
                        Err(Error::ReleaseAttemptConflict)
                    };
                }
                input.follows(
                    history
                        .attempts
                        .iter()
                        .rev()
                        .find(|row| row.start.key.ordinal == input.key.ordinal),
                )?;
                *effect = ReleaseWriteEffect::Unverified;
                attempt::insert_start(connection, input, bounds)?;
                Ok(true)
            },
        )
    }

    fn finish_release_attempt(
        &mut self,
        input: &ReleaseAttemptResult,
        options: ReleaseAttemptOptions,
    ) -> Result<ReleaseAttempt, ReleaseWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        consumption::encode(input, self.options)?;
        let bounds = self.options;
        self.append_attempt(
            &input.key,
            AttemptPhase::Result,
            |connection, history, effect| {
                let consumed = history
                    .consumption
                    .as_ref()
                    .ok_or(Error::ReleaseAttemptVerification)?;
                let existing = history
                    .attempts
                    .iter()
                    .find(|row| row.start.key == input.key)
                    .ok_or(Error::ReleaseAttemptVerification)?;
                input.validate(&existing.start, consumed)?;
                if let Some(result) = &existing.result {
                    return if result == input {
                        Ok(false)
                    } else {
                        Err(Error::ReleaseAttemptConflict)
                    };
                }
                *effect = ReleaseWriteEffect::Unverified;
                attempt::insert_result(connection, input, bounds)?;
                Ok(true)
            },
        )
        .map(|receipt| receipt.record)
    }
}
