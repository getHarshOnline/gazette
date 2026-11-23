//! Atomic pending registration within the existing held journal generation.

use super::Writer;
use crate::{authorization, journal, native_error, with_source};
use gazette_core::store::{
    Error, ReleaseAuthorizationEffect, ReleaseAuthorizationInput, ReleaseAuthorizationInspection,
    ReleaseAuthorizationReceipt, ReleaseAuthorizationWriteError, ReleaseAuthorizationWriteOptions,
};
use gazette_ports::store::{JournalWriter, ReleaseAuthorizationWriter};
use rusqlite::TransactionBehavior;

impl ReleaseAuthorizationWriter for Writer {
    fn inspect_release_authorization(
        &mut self,
        record_id: &str,
    ) -> Result<ReleaseAuthorizationInspection, Error> {
        self.inspect()?;
        self.failed = true;
        let result = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                let inspection = self.verify_snapshot(connection)?;
                let record = authorization::read(connection, record_id, self.header, self.options)?;
                Ok(ReleaseAuthorizationInspection {
                    journal: inspection.journal,
                    record,
                })
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(result)
    }

    fn record_release_authorization(
        &mut self,
        database: &std::path::Path,
        input: ReleaseAuthorizationInput<'_>,
        options: ReleaseAuthorizationWriteOptions,
    ) -> Result<ReleaseAuthorizationReceipt, ReleaseAuthorizationWriteError> {
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
        let payload = authorization::payload(input, self.options)?;
        let body = input.validate()?;
        self.inspect()?;
        self.failed = true;
        let mut effect = ReleaseAuthorizationEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            let inserted = match authorization::read(
                &transaction,
                body.record_id(),
                self.header,
                self.options,
            )? {
                Some(existing) => {
                    if existing.as_input() != input {
                        return Err(Error::ReleaseAuthorizationConflict);
                    }
                    false
                }
                None => {
                    if *input.binding.journal() != self.header {
                        return Err(Error::JournalMismatch);
                    }
                    authorization::check_nonce(&transaction, &body)?;
                    authorization::check_preparations(
                        &transaction,
                        input,
                        &body,
                        self.header,
                        self.options,
                    )?;
                    effect = ReleaseAuthorizationEffect::Unverified;
                    authorization::insert(&transaction, &body, input.preparation_ids, &payload)?;
                    true
                }
            };
            let stored =
                authorization::read(&transaction, body.record_id(), self.header, self.options)?
                    .ok_or(Error::ReleaseAuthorizationVerification)?;
            if stored.as_input() != input {
                return Err(Error::ReleaseAuthorizationVerification);
            }
            self.commit(transaction)?;
            connection
                .close()
                .map_err(|(_, error)| native_error(error))?;
            self.reservation.flush()?;
            self.verify_namespace()?;
            let observed = with_source(
                &self.reservation.path().join(journal::DATABASE),
                self.options,
                |connection| {
                    self.verify_snapshot(connection)?;
                    authorization::read(connection, body.record_id(), self.header, self.options)
                },
            )?
            .ok_or(Error::ReleaseAuthorizationVerification)?;
            if observed != stored {
                return Err(Error::ReleaseAuthorizationVerification);
            }
            self.verify_namespace()?;
            Ok(ReleaseAuthorizationReceipt {
                journal: self.header,
                record: observed,
                inserted,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(ReleaseAuthorizationWriteError { cause, effect }),
        }
    }
}
