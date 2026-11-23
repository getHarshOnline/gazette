//! Ref-observation append/replay within the existing reserved writer lifetime.

use super::Writer;
use crate::{journal, native_error, readback, with_source};
use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, RefObservationEffect, RefObservationInput, RefObservationInspection,
        RefObservationReceipt, RefObservationWriteError, RefObservationWriteOptions,
    },
};
use gazette_ports::store::{JournalWriter, RefObservationWriter};
use rusqlite::TransactionBehavior;

impl RefObservationWriter for Writer {
    fn inspect_ref_observation(
        &mut self,
        id: Sha256Digest,
    ) -> Result<RefObservationInspection, Error> {
        self.inspect()?;
        self.failed = true;
        let result = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                let inspection = self.verify_snapshot(connection)?;
                let record = readback::read(connection, id, self.header, self.options)?;
                Ok(RefObservationInspection {
                    journal: inspection.journal,
                    record,
                })
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(result)
    }

    fn record_ref_observation(
        &mut self,
        input: RefObservationInput<'_>,
        options: RefObservationWriteOptions,
    ) -> Result<RefObservationReceipt, RefObservationWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        input.validate()?;
        // Replay retains its original creation coordinates. Do not budget a
        // hypothetical new record whose later fence may have more JSON digits.
        let payload = if self.inspect_ref_observation(input.id)?.record.is_none() {
            Some(readback::payload(input, self.header, self.options)?)
        } else {
            None
        };
        self.failed = true;
        let mut effect = RefObservationEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            let (created, inserted) =
                match readback::read(&transaction, input.id, self.header, self.options)? {
                    Some(existing) => {
                        if existing.as_input() != input {
                            return Err(Error::ReadbackConflict);
                        }
                        (existing.created, false)
                    }
                    None => {
                        let payload = payload.as_deref().ok_or(Error::ReadbackVerification)?;
                        effect = RefObservationEffect::Unverified;
                        readback::insert(&transaction, input.id, payload)?;
                        (self.header, true)
                    }
                };
            let stored = readback::read(&transaction, input.id, self.header, self.options)?
                .ok_or(Error::ReadbackVerification)?;
            if stored.as_input() != input || stored.created != created {
                return Err(Error::ReadbackVerification);
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
                    readback::read(connection, input.id, self.header, self.options)
                },
            )?
            .ok_or(Error::ReadbackVerification)?;
            if observed != stored {
                return Err(Error::ReadbackVerification);
            }
            self.verify_namespace()?;
            Ok(RefObservationReceipt {
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
            Err(cause) => Err(RefObservationWriteError { cause, effect }),
        }
    }
}
