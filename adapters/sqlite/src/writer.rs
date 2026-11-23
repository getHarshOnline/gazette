//! Existing-journal generation acquisition; no arbitrary SQL or release effects.

use crate::{
    Sqlite, cache, check_files, checkpoint, configure_connection, coordination::Reservation,
    journal, native_error, registry, with_source,
};
use gazette_core::store::{
    BuildCacheInput, BuildCacheReceipt, BuildCacheWriteOptions, CheckpointEffect, CheckpointInput,
    CheckpointReceipt, CheckpointWriteError, CheckpointWriteOptions, Error, GenerationEffect,
    JournalHeader, JournalInspection, ReadOptions, RegistryEffect, RegistryInput,
    RegistryInspection, RegistryReceipt, RegistryWriteError, RegistryWriteOptions,
    WriterAcquireError, WriterOptions,
};
use gazette_ports::store::{
    BuildCacheWriter, CheckpointWriter, JournalWriter, RegistryWriter, WriterConnector,
};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{fs, os::unix::fs::MetadataExt};

mod attempt;
mod authorization;
mod consumption;
mod materialization;
mod preparation;
mod readback;
mod recovery;
mod settlement;
pub use recovery::Reconciler;

/// Lifetime-held local generation. Copies and restored history require separate fencing.
///
/// The reservation coordinates cooperating local callers, not arbitrary SQL writers.
/// No idle writable connection or raw SQL capability escapes this adapter.
pub struct Writer {
    reservation: Reservation,
    coordinator: Option<crate::coordinator::Guard>,
    identity: (u64, u64),
    header: JournalHeader,
    options: ReadOptions,
    failed: bool,
    recovery: Option<gazette_core::store::RecoveryProfile>,
}

impl WriterConnector for Sqlite {
    type Writer = Writer;

    fn acquire_writer(
        &self,
        root: &std::path::Path,
        expected: JournalHeader,
        options: WriterOptions,
    ) -> Result<Writer, WriterAcquireError> {
        options.validate()?;
        let reservation = Reservation::acquire(root)?;
        Writer::reserved(reservation, expected, options.session.read)?.start()
    }
}

impl Writer {
    /// Hold and inspect the ordinary namespace without attempting generation advancement.
    pub(super) fn reserved(
        reservation: Reservation,
        expected: JournalHeader,
        options: ReadOptions,
    ) -> Result<Self, Error> {
        let metadata = check_files(
            &reservation.path().join(journal::DATABASE),
            options.storage_bytes,
        )?;
        let mut writer = Writer {
            reservation,
            coordinator: None,
            identity: (metadata.dev(), metadata.ino()),
            header: expected,
            options,
            failed: false,
            recovery: None,
        };
        // Read-only preflight rejects unsupported state before any writable open.
        // Sidecars are never repaired or deleted to make acquisition possible.
        writer.inspect()?;
        Ok(writer)
    }

    pub(super) fn coordinate(&mut self, guard: crate::coordinator::Guard) {
        self.coordinator = Some(guard);
    }

    /// One generation transaction owner shared by ordinary and anchored acquisition.
    pub(super) fn start(mut self) -> Result<Self, WriterAcquireError> {
        if self.header.writer_fence >= i64::MAX as u64 {
            return Err(Error::WriterExhausted.into());
        }
        let mut generation = GenerationEffect::NotAttempted;
        let result = (|| {
            self.advance(&mut generation)?;
            self.header.writer_fence += 1;
            self.inspect()?; // Independent connection after commit and explicit writer close.
            self.reservation.flush()?;
            self.verify_namespace()?;
            Ok(())
        })();
        result.map_err(|cause| WriterAcquireError { cause, generation })?;
        Ok(self)
    }
}

impl Writer {
    fn verify_namespace(&self) -> Result<(), Error> {
        if let Some(guard) = &self.coordinator {
            guard.verify()?;
        }
        self.reservation.verify()?;
        let mut entries = fs::read_dir(self.reservation.path()).map_err(Error::Io)?;
        if entries
            .next()
            .transpose()
            .map_err(Error::Io)?
            .is_none_or(|entry| entry.file_name() != journal::DATABASE)
            || entries.next().transpose().map_err(Error::Io)?.is_some()
        {
            return Err(Error::Changed);
        }
        let current = check_files(
            &self.reservation.path().join(journal::DATABASE),
            self.options.storage_bytes,
        )?;
        // A pre-existing hardlink would allow two local directory lock namespaces
        // for the same journal. Ordinary copies remain a different unresolved boundary.
        if current.nlink() != 1 || current.mode() & 0o077 != 0 {
            return Err(Error::UnsafePath);
        }
        if self.identity != (current.dev(), current.ino()) {
            return Err(Error::Changed);
        }
        Ok(())
    }

    /// Fingerprint only between `SQLite` connections: no descriptor close can invalidate SQL locks.
    pub(super) fn coordinator_seal(
        &self,
        binding: &gazette_core::store::LocalCoordinatorBinding,
        synchronize: bool,
    ) -> Result<gazette_core::store::LocalCoordinatorSeal, Error> {
        self.verify_namespace()?;
        if self.reservation.path() != binding.journal_root
            || self.header.journal_id != binding.journal_id
            || self.header.journal_epoch != binding.journal_epoch
        {
            return Err(Error::CoordinatorMismatch);
        }
        let path = self.reservation.path().join(journal::DATABASE);
        let identity = crate::regular_file(&path)?;
        let (file, _, database_sha256) =
            crate::backup::fingerprint(&path, &identity, self.options.storage_bytes)?;
        if synchronize {
            file.sync_all().map_err(Error::Io)?;
            self.reservation.flush()?;
        }
        drop(file);
        let content_sha256 = crate::coordinator::snapshot::capture(
            &crate::open_existing(&path, self.options)?,
            self.options,
        )?
        .content_sha256;
        self.verify_namespace()?;
        Ok(gazette_core::store::LocalCoordinatorSeal {
            binding: binding.clone(),
            header: self.header,
            database_sha256,
            content_sha256,
        })
    }

    fn verify_snapshot(&self, connection: &Connection) -> Result<RegistryInspection, Error> {
        let inspection = registry::inspect(connection, self.options)?;
        crate::materialization::verify_all(connection, inspection.journal.header, self.options)?;
        if inspection.journal.header != self.header {
            return Err(Error::JournalMismatch);
        }
        // Current writer qualification is for initialized DELETE-mode standalone
        // journals only. Do not silently checkpoint/convert WAL or recover hot state.
        if inspection.journal.verifier.journal_mode != "delete" {
            return Err(Error::RecoveryRequired);
        }
        if let Some(guard) = &self.coordinator {
            guard.verify_transaction(connection, self.options)?;
        }
        if let Some(profile) = self.recovery {
            crate::attempt::require_recovery(connection, profile, self.options)?;
        }
        Ok(inspection)
    }

    fn writable(&self) -> Result<Connection, Error> {
        self.verify_namespace()?;
        let connection = Connection::open_with_flags(
            self.reservation.path().join(journal::DATABASE),
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX
                | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
                | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(native_error)?;
        configure_connection(&connection, self.options)?;
        connection
            .execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON;")
            .map_err(native_error)?;
        Ok(connection)
    }

    /// All mutation families use this one precommit continuity barrier. Do not open
    /// or fingerprint the database file while `SQLite` owns the active transaction.
    fn commit(&mut self, transaction: rusqlite::Transaction<'_>) -> Result<(), Error> {
        self.reservation.verify()?;
        let current = fs::symlink_metadata(self.reservation.path().join(journal::DATABASE))
            .map_err(Error::Io)?;
        if !current.is_file()
            || current.nlink() != 1
            || current.mode() & 0o077 != 0
            || (current.dev(), current.ino()) != self.identity
        {
            return Err(Error::Changed);
        }
        if let Some(guard) = &mut self.coordinator {
            let expected = crate::coordinator::snapshot::capture(&transaction, self.options)?;
            guard.expect_transaction(expected)?;
        }
        transaction.commit().map_err(native_error)
    }

    fn advance(&mut self, generation: &mut GenerationEffect) -> Result<(), Error> {
        let mut connection = self.writable()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(native_error)?;
        self.verify_namespace()?;
        self.verify_snapshot(&transaction)?;
        // Once attempted, any later error is conservatively unverified even if
        // rollback probably happened. A lost result is never permission to retry.
        *generation = GenerationEffect::Unverified;
        journal::advance_generation(&transaction, self.header)?;
        self.commit(transaction)?;
        connection
            .close()
            .map_err(|(_, error)| native_error(error))?;
        Ok(())
    }
}

impl RegistryWriter for Writer {
    fn replace_registry(
        &mut self,
        input: RegistryInput<'_>,
        options: RegistryWriteOptions,
    ) -> Result<RegistryReceipt, RegistryWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        // Pure lowering and identity checks precede any registry SQL mutation.
        let desired = registry::Desired::prepare(input, self.options)?;
        self.inspect()?;
        self.failed = true;
        let mut effect = RegistryEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            effect = RegistryEffect::Unverified;
            desired.replace(&transaction, self.options)?;
            self.commit(transaction)?;
            connection
                .close()
                .map_err(|(_, error)| native_error(error))?;
            self.verify_namespace()?;
            let read_back = with_source(
                &self.reservation.path().join(journal::DATABASE),
                self.options,
                |connection| self.verify_snapshot(connection),
            )?;
            if read_back.registry != Some(desired.head) {
                return Err(Error::RegistryVerification);
            }
            self.reservation.flush()?;
            self.verify_namespace()?;
            Ok(RegistryReceipt {
                journal: self.header,
                registry: desired.head,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(RegistryWriteError { cause, effect }),
        }
    }
}

impl BuildCacheWriter for Writer {
    fn replace_build_cache(
        &mut self,
        input: BuildCacheInput<'_>,
        options: BuildCacheWriteOptions,
    ) -> Result<BuildCacheReceipt, Error> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed);
        }
        let desired = cache::Desired::prepare(input)?;
        self.inspect()?;
        self.failed = true;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            desired.replace(&transaction, self.options)?;
            self.commit(transaction)?;
            connection
                .close()
                .map_err(|(_, error)| native_error(error))?;
            self.verify_namespace()?;
            let read_back = with_source(
                &self.reservation.path().join(journal::DATABASE),
                self.options,
                |connection| cache::inspect(connection, self.options),
            )?;
            if read_back.cache != Some(desired.head) || read_back.journal.header != self.header {
                return Err(Error::BuildCacheVerification);
            }
            self.reservation.flush()?;
            self.verify_namespace()?;
            Ok(BuildCacheReceipt {
                journal: self.header,
                cache: desired.head,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(cause),
        }
    }
}

impl CheckpointWriter for Writer {
    fn record_checkpoint(
        &mut self,
        input: CheckpointInput<'_>,
        options: CheckpointWriteOptions,
    ) -> Result<CheckpointReceipt, CheckpointWriteError> {
        options.validate()?;
        if self.failed {
            return Err(Error::Changed.into());
        }
        checkpoint::validate(input, self.options)?;
        self.inspect()?;
        self.failed = true;
        let mut effect = CheckpointEffect::NotAttempted;
        let result = (|| {
            let mut connection = self.writable()?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(native_error)?;
            self.verify_namespace()?;
            self.verify_snapshot(&transaction)?;
            let created = if let Some(existing) =
                checkpoint::read(&transaction, input.id, self.header, self.options)?
            {
                if !checkpoint::same_payload(&existing, input) {
                    return Err(Error::CheckpointConflict);
                }
                existing.created
            } else {
                effect = CheckpointEffect::Unverified;
                checkpoint::insert(&transaction, input, self.header)?;
                self.header
            };
            let stored = checkpoint::read(&transaction, input.id, self.header, self.options)?
                .ok_or(Error::CheckpointVerification)?;
            if !checkpoint::same_payload(&stored, input) || stored.created != created {
                return Err(Error::CheckpointVerification);
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
                    checkpoint::read(connection, input.id, self.header, self.options)
                },
            )?
            .ok_or(Error::CheckpointVerification)?;
            if read_back != stored {
                return Err(Error::CheckpointVerification);
            }
            self.verify_namespace()?;
            Ok(CheckpointReceipt {
                journal: self.header,
                checkpoint: read_back,
            })
        })();
        match result {
            Ok(receipt) => {
                self.failed = false;
                Ok(receipt)
            }
            Err(cause) => Err(CheckpointWriteError { cause, effect }),
        }
    }
}

impl JournalWriter for Writer {
    fn inspect(&mut self) -> Result<JournalInspection, Error> {
        if self.failed {
            return Err(Error::Changed);
        }
        self.failed = true;
        self.verify_namespace()?;
        let inspection = with_source(
            &self.reservation.path().join(journal::DATABASE),
            self.options,
            |connection| self.verify_snapshot(connection),
        )?;
        self.verify_namespace()?;
        self.failed = false;
        Ok(inspection.journal)
    }

    fn close(mut self) -> Result<(), Error> {
        let coordinated = self.coordinator.is_some();
        let result = (|| {
            self.inspect()?;
            if let Some(guard) = &self.coordinator {
                let seal = self.coordinator_seal(guard.binding(), true)?;
                guard.finish(seal)?;
            }
            Ok(())
        })();
        result.map_err(|cause| match cause {
            Error::CoordinatorUnverified(_) => cause,
            cause if coordinated => Error::CoordinatorUnverified(Box::new(cause)),
            cause => cause,
        })
        // Drop releases the directory lock even on failure; never reset a generation.
    }
}

#[cfg(test)]
#[path = "../tests/internal/writer.rs"]
mod tests;
