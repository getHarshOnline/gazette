//! Append/replay transactions over the existing reserved writer lifetime.

use super::Writer;
use crate::{journal, native_error, preparation, with_source};
use gazette_core::store::{
    Error, PreparationCheckInput, PreparationCommitInput, PreparationEffect, PreparationInput,
    PreparationReceipt, PreparationRetentionInput, PreparationWorktreeInput, PreparationWriteError,
    PreparationWriteOptions,
};
use gazette_ports::store::{JournalWriter, PreparationWriter};
use rusqlite::{Connection, TransactionBehavior};

#[derive(Clone, Copy)]
enum Append<'a> {
    Intent(PreparationInput<'a>),
    Retention(PreparationRetentionInput<'a>),
    Check(PreparationCheckInput<'a>),
    Worktree(PreparationWorktreeInput<'a>),
    Commit(PreparationCommitInput<'a>),
}

impl<'a> Append<'a> {
    fn intent(self) -> PreparationInput<'a> {
        match self {
            Self::Intent(input) => input,
            Self::Retention(input) => input.intent.as_input(),
            Self::Check(input) => input.intent.as_input(),
            Self::Worktree(input) => input.intent.as_input(),
            Self::Commit(input) => input.intent.as_input(),
        }
    }

    fn apply(
        self,
        connection: &Connection,
        writer: &Writer,
        effect: &mut PreparationEffect,
    ) -> Result<bool, Error> {
        let input = self.intent();
        let existing = preparation::read(connection, input.id, writer.header, writer.options)?;
        match self {
            Self::Intent(input) => {
                if let Some(existing) = existing {
                    if existing.intent.as_input() != input {
                        return Err(Error::PreparationConflict);
                    }
                    return Ok(false);
                }
                *effect = PreparationEffect::Unverified;
                preparation::insert_intent(connection, input, writer.header)?;
            }
            Self::Retention(input) => {
                let existing = existing.ok_or(Error::PreparationVerification)?;
                if existing.intent != *input.intent {
                    return Err(Error::PreparationConflict);
                }
                if existing.retention.is_some() {
                    return Ok(false);
                }
                *effect = PreparationEffect::Unverified;
                preparation::insert_retention(connection, input, writer.header)?;
            }
            Self::Check(input) => {
                let existing = existing.ok_or(Error::PreparationVerification)?;
                if existing.intent != *input.intent
                    || existing.retention.as_ref() != Some(input.retention)
                {
                    return Err(Error::PreparationConflict);
                }
                if let Some(check) = existing.check {
                    if check.evidence != input.evidence
                        || check.evidence_sha256 != input.evidence_sha256
                    {
                        return Err(Error::PreparationConflict);
                    }
                    return Ok(false);
                }
                if existing.worktree.is_some() {
                    return Err(Error::PreparationConflict);
                }
                *effect = PreparationEffect::Unverified;
                preparation::check::insert(connection, input, writer.header)?;
            }
            Self::Worktree(input) => {
                let existing = existing.ok_or(Error::PreparationVerification)?;
                if existing.intent != *input.intent
                    || existing.retention.as_ref() != Some(input.retention)
                    || existing.check.as_ref() != input.check
                {
                    return Err(Error::PreparationConflict);
                }
                if let Some(worktree) = existing.worktree {
                    if worktree.metadata != *input.metadata
                        || worktree.parent_source.as_ref() != input.parent_source
                    {
                        return Err(Error::PreparationConflict);
                    }
                    return Ok(false);
                }
                *effect = PreparationEffect::Unverified;
                preparation::worktree::insert_worktree(connection, input, writer.header)?;
            }
            Self::Commit(input) => {
                let existing = existing.ok_or(Error::PreparationVerification)?;
                if existing.intent != *input.intent
                    || existing.retention.as_ref() != Some(input.retention)
                    || existing.check.as_ref() != input.check
                {
                    return Err(Error::PreparationConflict);
                }
                let worktree = existing.worktree.ok_or(Error::PreparationVerification)?;
                if worktree.metadata != input.worktree.metadata
                    || worktree.parent_source != input.worktree.parent_source
                    || worktree.check_sha256 != input.worktree.check_sha256
                    || worktree.created != input.worktree.created
                    || input
                        .worktree
                        .completion
                        .as_ref()
                        .is_some_and(|supplied| worktree.completion.as_ref() != Some(supplied))
                {
                    return Err(Error::PreparationConflict);
                }
                if let Some(completion) = worktree.completion {
                    if completion.report != *input.report {
                        return Err(Error::PreparationConflict);
                    }
                    return Ok(false);
                }
                *effect = PreparationEffect::Unverified;
                preparation::worktree::insert_commit(connection, input, writer.header)?;
            }
        }
        Ok(true)
    }
}

impl Writer {
    fn append_preparation(
        &mut self,
        append: Append<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        match append {
            Append::Intent(input) => preparation::validate(input, self.options)?,
            Append::Retention(input) => preparation::validate_retention(input, self.options)?,
            Append::Check(input) => preparation::check::validate(input, self.options)?,
            Append::Worktree(input) => {
                preparation::worktree::validate_worktree(input, self.options)?
            }
            Append::Commit(input) => preparation::worktree::validate_commit(input, self.options)?,
        }
        self.inspect()?;
        self.failed = true;
        let mut effect = PreparationEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            let inserted = append.apply(&transaction, self, &mut effect)?;
            let input = append.intent();
            let stored = preparation::read(&transaction, input.id, self.header, self.options)?
                .ok_or(Error::PreparationVerification)?;
            if stored.intent.as_input() != input
                || matches!(append, Append::Retention(_)) && stored.retention.is_none()
                || match append {
                    Append::Check(input) => stored.check.as_ref().is_none_or(|check| {
                        check.evidence != input.evidence
                            || check.evidence_sha256 != input.evidence_sha256
                    }),
                    Append::Worktree(input) => stored.worktree.as_ref().is_none_or(|worktree| {
                        worktree.metadata != *input.metadata
                            || worktree.parent_source.as_ref() != input.parent_source
                    }),
                    Append::Commit(input) => stored
                        .worktree
                        .as_ref()
                        .and_then(|worktree| worktree.completion.as_ref())
                        .is_none_or(|completion| completion.report != *input.report),
                    _ => false,
                }
            {
                return Err(Error::PreparationVerification);
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
                    preparation::read(connection, input.id, self.header, self.options)
                },
            )?
            .ok_or(Error::PreparationVerification)?;
            if read_back != stored {
                return Err(Error::PreparationVerification);
            }
            self.verify_namespace()?;
            Ok(PreparationReceipt {
                journal: self.header,
                record: read_back,
                inserted,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(PreparationWriteError { cause, effect }),
        }
    }
}

impl PreparationWriter for Writer {
    fn record_preparation_check(
        &mut self,
        input: PreparationCheckInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.append_preparation(Append::Check(input), options)
    }

    fn inspect_preparation(
        &mut self,
        id: gazette_core::source::Sha256Digest,
    ) -> Result<gazette_core::store::PreparationInspection, Error> {
        if self.failed {
            return Err(Error::Changed);
        }
        self.failed = true;
        self.verify_namespace()?;
        let inspection = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| {
                let inspection = self.verify_snapshot(connection)?;
                let record = preparation::read(connection, id, self.header, self.options)?;
                Ok(gazette_core::store::PreparationInspection {
                    journal: inspection.journal,
                    record,
                })
            },
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(inspection)
    }

    fn record_preparation_worktree(
        &mut self,
        input: PreparationWorktreeInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.append_preparation(Append::Worktree(input), options)
    }

    fn record_preparation_commit(
        &mut self,
        input: PreparationCommitInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.append_preparation(Append::Commit(input), options)
    }

    fn record_preparation_intent(
        &mut self,
        input: PreparationInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.append_preparation(Append::Intent(input), options)
    }

    fn record_preparation_retention(
        &mut self,
        input: PreparationRetentionInput<'_>,
        options: PreparationWriteOptions,
    ) -> Result<PreparationReceipt, PreparationWriteError> {
        self.append_preparation(Append::Retention(input), options)
    }
}
