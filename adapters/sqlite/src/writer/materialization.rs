//! Reserved append/read-back transactions; no filesystem effects or automatic retry.

use super::Writer;
use crate::{journal, materialization, native_error, with_source};
use gazette_core::store::{
    Error, MaterializationCompletionInput, MaterializationEffect, MaterializationInput,
    MaterializationInspection, MaterializationReceipt, MaterializationWriteError,
    MaterializationWriteOptions,
};
use gazette_ports::store::{JournalWriter, MaterializationWriter};
use rusqlite::TransactionBehavior;
use std::path::Path;

#[derive(Clone, Copy)]
enum Append<'a> {
    Intent(MaterializationInput<'a>),
    Completion(MaterializationCompletionInput<'a>),
}
impl<'a> Append<'a> {
    fn input(self) -> MaterializationInput<'a> {
        match self {
            Self::Intent(input) => input,
            Self::Completion(input) => input.intent.as_input(),
        }
    }
}
impl Writer {
    fn append_materialization(
        &mut self,
        append: Append<'_>,
        options: MaterializationWriteOptions,
    ) -> Result<MaterializationReceipt, MaterializationWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        let input = append.input();
        materialization::validate(input, self.options)?;
        // Exact current header encoding is admitted before entering the SQL effect boundary.
        materialization::payload(input, self.header, self.options)?;
        if let Append::Completion(input) = append {
            input.validate()?;
        }
        self.inspect()?;
        self.failed = true;
        let mut effect = MaterializationEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            let existing =
                materialization::read(&transaction, input.id, self.header, self.options)?;
            let frontier = materialization::frontier(
                &transaction,
                input.binding.canonical_root(),
                self.header,
                self.options,
            )?;
            let inserted = match append {
                Append::Intent(input) => {
                    if let Some(existing) = existing {
                        if existing.intent.as_input() != input {
                            return Err(Error::MaterializationConflict);
                        }
                        false
                    } else {
                        if frontier.active.is_some() {
                            return Err(Error::MaterializationPending);
                        }
                        if frontier.head.as_ref().map(|record| record.intent.id) != input.previous
                            || frontier.head.as_ref().is_some_and(|record| {
                                record.intent.site != *input.site
                                    || record.intent.binding != *input.binding
                            })
                        {
                            return Err(Error::MaterializationConflict);
                        }
                        effect = MaterializationEffect::Unverified;
                        materialization::insert_intent(
                            &transaction,
                            input,
                            self.header,
                            self.options,
                        )?;
                        true
                    }
                }
                Append::Completion(input) => {
                    let existing = existing.ok_or(Error::MaterializationVerification)?;
                    if existing.intent != *input.intent {
                        return Err(Error::MaterializationConflict);
                    }
                    if existing.completion.is_some() {
                        false
                    } else {
                        if frontier.active.as_ref().map(|record| record.intent.id)
                            != Some(input.intent.id)
                            || frontier.head.as_ref().map(|record| record.intent.id)
                                != input.intent.previous
                        {
                            return Err(Error::MaterializationConflict);
                        }
                        effect = MaterializationEffect::Unverified;
                        materialization::insert_completion(
                            &transaction,
                            input.intent,
                            self.header,
                            self.options,
                        )?;
                        true
                    }
                }
            };
            materialization::verify_all(&transaction, self.header, self.options)?;
            let stored = materialization::read(&transaction, input.id, self.header, self.options)?
                .ok_or(Error::MaterializationVerification)?;
            if stored.intent.as_input() != input
                || matches!(append, Append::Completion(_)) && stored.completion.is_none()
            {
                return Err(Error::MaterializationVerification);
            }
            self.commit(transaction)?;
            connection
                .close()
                .map_err(|(_, error)| native_error(error))?;
            self.reservation.flush()?;
            self.verify_namespace()?;
            let read_back = with_source(
                &self.reservation.path().join(journal::DATABASE),
                self.options,
                |connection| {
                    self.verify_snapshot(connection)?;
                    materialization::read(connection, input.id, self.header, self.options)
                },
            )?
            .ok_or(Error::MaterializationVerification)?;
            if read_back != stored {
                return Err(Error::MaterializationVerification);
            }
            self.verify_namespace()?;
            Ok(MaterializationReceipt {
                inserted,
                journal: self.header,
                record: read_back,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(MaterializationWriteError { cause, effect }),
        }
    }
}
impl MaterializationWriter for Writer {
    fn materialization_roots(&mut self) -> Result<Vec<std::path::PathBuf>, Error> {
        self.inspect()?;
        self.failed = true;
        let result = (|| {
            self.verify_namespace()?;
            let mut roots = vec![self.reservation.path().to_path_buf()];
            if let Some(guard) = &self.coordinator {
                roots.push(guard.materialization_root()?);
            }
            self.verify_namespace()?;
            Ok(roots)
        })();
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    fn inspect_materialization(&mut self, root: &Path) -> Result<MaterializationInspection, Error> {
        if self.failed {
            return Err(Error::Changed);
        }
        self.failed = true;
        self.verify_namespace()?;
        let inspection = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                let inspected = self.verify_snapshot(connection)?;
                let frontier =
                    materialization::frontier(connection, root, self.header, self.options)?;
                Ok(MaterializationInspection {
                    journal: inspected.journal,
                    frontier,
                })
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(inspection)
    }
    fn record_materialization_intent(
        &mut self,
        input: MaterializationInput<'_>,
        options: MaterializationWriteOptions,
    ) -> Result<MaterializationReceipt, MaterializationWriteError> {
        self.append_materialization(Append::Intent(input), options)
    }
    fn record_materialization_completion(
        &mut self,
        input: MaterializationCompletionInput<'_>,
        options: MaterializationWriteOptions,
    ) -> Result<MaterializationReceipt, MaterializationWriteError> {
        self.append_materialization(Append::Completion(input), options)
    }
}
