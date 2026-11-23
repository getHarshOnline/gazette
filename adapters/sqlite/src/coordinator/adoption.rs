//! Crash-safe exact restore adoption into a fresh successor journal root.

use super::{Anchor, Observed, reserve_journal};
use crate::{
    Sqlite, backup, configure_connection, coordination::Reservation, journal, native_error,
    regular_file, settlement, with_source,
};
use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, LocalCoordinatorAdoption, LocalCoordinatorBinding, LocalCoordinatorState,
        RestoreAdoptionOptions, RestoreAdoptionReceipt, RestoreAdoptionSource,
        RestoreAdoptionTarget, RestoreInspection, RestorePreparation,
    },
};
use gazette_ports::store::{RestoreAdopter, RestoreAdoptionRequest};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
};

const RECEIPT_FILE: &str = "restore.json";

impl RestoreAdopter for Sqlite {
    fn adopt_restore(
        &self,
        request: RestoreAdoptionRequest<'_>,
        options: RestoreAdoptionOptions,
    ) -> Result<RestoreAdoptionReceipt, Error> {
        let RestoreAdoptionRequest {
            anchor_root,
            expected,
            previous,
            restore_root,
            receipt_sha256,
            successor_root,
        } = request;
        options.validate()?;
        if !anchor_root.is_absolute()
            || !restore_root.is_absolute()
            || !successor_root.is_absolute()
        {
            return Err(Error::UnsafePath);
        }
        if previous.binding != *expected {
            return Err(Error::CoordinatorMismatch);
        }
        let next_fence = previous
            .header
            .writer_fence
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(Error::WriterExhausted)?;
        let anchor = Anchor::acquire(anchor_root, options.read)?;
        anchor.validate_coordinator_id(&expected.coordinator_id)?;
        let observed = anchor.read()?;
        match &observed.state {
            LocalCoordinatorState::Clean {
                adoption: Some(completed),
                ..
            } if completed.previous == *previous => {}
            LocalCoordinatorState::Clean { seal, .. } if seal != previous => {
                return Err(Error::CoordinatorMismatch);
            }
            LocalCoordinatorState::InProgress { .. } => return Err(Error::CoordinatorState),
            _ => {}
        }

        let old = reserve_journal(&anchor, &expected.journal_root)?;
        let old_writer = crate::Writer::reserved(old, previous.header, options.read)?;
        if old_writer.coordinator_seal(expected, false)? != *previous {
            return Err(Error::CoordinatorMismatch);
        }
        with_source(
            &expected.journal_root.join(backup::DATABASE_FILE),
            options.read,
            |connection| {
                settlement::require_terminal(connection, options.read)?;
                crate::materialization::require_terminal(connection, options.read)
            },
        )?;

        let (restore_reservation, inspected) = inspect_restore(
            restore_root,
            receipt_sha256,
            options.receipt_bytes,
            options.read,
        )?;
        if inspected.journal.header != previous.header
            || inspected.content_sha256 != previous.content_sha256
        {
            return Err(Error::RestoreAdoptionVerification);
        }
        let source = RestoreAdoptionSource {
            restore_root: inspected.directory,
            receipt_sha256: inspected.receipt_sha256,
            source_manifest_sha256: inspected.source_manifest_sha256,
            database_sha256: inspected.database_sha256,
            content_sha256: inspected.content_sha256,
        };

        let target = Reservation::acquire(successor_root)?;
        if expected.journal_root == target.path() {
            return Err(Error::CoordinatorMismatch);
        }
        let next_binding = LocalCoordinatorBinding {
            coordinator_id: expected.coordinator_id.clone(),
            principal_scope: expected.principal_scope,
            journal_root: target.path().to_owned(),
            journal_id: expected.journal_id,
            journal_epoch: expected.journal_epoch,
        };
        let next_header = gazette_core::store::JournalHeader {
            writer_fence: next_fence,
            ..previous.header
        };
        let next = RestoreAdoptionTarget {
            binding: next_binding.clone(),
            header: next_header,
        };
        let mut adopting = match &observed.state {
            LocalCoordinatorState::Clean { seal, adoption } if seal == previous => {
                if adoption.as_ref().is_some_and(|completed| {
                    completed.previous != *previous || completed.source != source
                }) {
                    return Err(Error::CoordinatorMismatch);
                }
                ensure_empty(&target)?;
                anchor.write(
                    LocalCoordinatorState::Adopting {
                        previous: previous.clone(),
                        source: source.clone(),
                        next: next.clone(),
                        transaction: None,
                    },
                    Some(&observed),
                )?
            }
            LocalCoordinatorState::Adopting {
                previous: retained,
                source: retained_source,
                next: retained_next,
                ..
            } if retained == previous && retained_source == &source && retained_next == &next => {
                observed
            }
            LocalCoordinatorState::Clean {
                seal,
                adoption: Some(completed),
            } if completed.previous == *previous && completed.source == source => {
                if seal.binding != next_binding || seal.header != next_header {
                    return Err(Error::CoordinatorMismatch);
                }
                let target_writer = crate::Writer::reserved(target, next_header, options.read)?;
                if target_writer.coordinator_seal(&next_binding, false)? != *seal {
                    return Err(Error::CoordinatorMismatch);
                }
                verify_restore_unchanged(&restore_reservation, &source, options)?;
                return Ok(RestoreAdoptionReceipt {
                    previous: previous.clone(),
                    source,
                    successor: seal.clone(),
                });
            }
            _ => return Err(Error::CoordinatorState),
        };

        let database = target.path().join(backup::DATABASE_FILE);
        let mut successor = inspect_successor(&target, options.read)?;
        if successor.is_none() {
            copy_source(
                &source.restore_root.join(backup::DATABASE_FILE),
                &database,
                source.database_sha256,
                options.read.storage_bytes,
            )?;
            successor = inspect_successor(&target, options.read)?;
        }
        let Some(mut successor) = successor else {
            return Err(Error::RestoreAdoptionVerification);
        };
        if successor.header == previous.header {
            if successor.database_sha256 != source.database_sha256
                || successor.content_sha256 != source.content_sha256
            {
                return Err(Error::RestoreAdoptionVerification);
            }
            Advance {
                path: &database,
                options: options.read,
                anchor: &anchor,
                previous,
                source: &source,
                next: &next,
            }
            .execute(&mut adopting)?;
            target.flush()?;
            successor = inspect_successor(&target, options.read)?
                .ok_or(Error::RestoreAdoptionVerification)?;
        }
        let expected_transaction = match &adopting.state {
            LocalCoordinatorState::Adopting {
                transaction: Some(expected),
                ..
            } => expected,
            _ => return Err(Error::RestoreAdoptionVerification),
        };
        if successor.header != next_header
            || successor.content_sha256 != expected_transaction.content_sha256
        {
            return Err(Error::RestoreAdoptionVerification);
        }
        let target_writer = crate::Writer::reserved(target, next_header, options.read)?;
        let successor = target_writer.coordinator_seal(&next_binding, true)?;
        if successor.content_sha256 != expected_transaction.content_sha256 {
            return Err(Error::RestoreAdoptionVerification);
        }
        verify_restore_unchanged(&restore_reservation, &source, options)?;
        adopting = anchor.write(
            LocalCoordinatorState::Clean {
                seal: successor.clone(),
                adoption: Some(LocalCoordinatorAdoption {
                    previous: previous.clone(),
                    source: source.clone(),
                }),
            },
            Some(&adopting),
        )?;
        match adopting.state {
            LocalCoordinatorState::Clean { seal, .. } if seal == successor => {}
            _ => {
                return Err(Error::CoordinatorUnverified(Box::new(
                    Error::CoordinatorState,
                )));
            }
        }
        Ok(RestoreAdoptionReceipt {
            previous: previous.clone(),
            source,
            successor,
        })
    }
}

struct SuccessorSnapshot {
    header: gazette_core::store::JournalHeader,
    database_sha256: Sha256Digest,
    content_sha256: Sha256Digest,
}

/// Classify only the three replayable successor states: empty, an exact copied
/// restore at the previous fence, or the exact copy after its one permitted
/// fence advance. Any residue outside that profile is preserved and refused.
fn inspect_successor(
    target: &Reservation,
    options: gazette_core::store::ReadOptions,
) -> Result<Option<SuccessorSnapshot>, Error> {
    target.verify()?;
    let mut entries = fs::read_dir(target.path()).map_err(Error::Io)?;
    let Some(entry) = entries.next().transpose().map_err(Error::Io)? else {
        return Ok(None);
    };
    if entry.file_name() != backup::DATABASE_FILE
        || entries.next().transpose().map_err(Error::Io)?.is_some()
    {
        return Err(Error::RestoreAdoptionVerification);
    }
    let database = target.path().join(backup::DATABASE_FILE);
    let identity = regular_file(&database)?;
    if identity.nlink() != 1 || identity.mode() & 0o077 != 0 {
        return Err(Error::UnsafePath);
    }
    let (file, _, database_sha256) =
        backup::fingerprint(&database, &identity, options.storage_bytes)?;
    drop(file);
    let (journal, transaction) = with_source(&database, options, |connection| {
        Ok((
            journal::inspect(connection)?,
            super::snapshot::capture(connection, options)?,
        ))
    })?;
    if journal.header != transaction.header || journal.verifier.journal_mode != "delete" {
        return Err(Error::RestoreAdoptionVerification);
    }
    target.verify()?;
    Ok(Some(SuccessorSnapshot {
        header: journal.header,
        database_sha256,
        content_sha256: transaction.content_sha256,
    }))
}

fn ensure_empty(target: &Reservation) -> Result<(), Error> {
    target.verify()?;
    if fs::read_dir(target.path())
        .map_err(Error::Io)?
        .next()
        .transpose()
        .map_err(Error::Io)?
        .is_some()
    {
        return Err(Error::StoreExists);
    }
    Ok(())
}

fn inspect_restore(
    root: &Path,
    pin: Sha256Digest,
    receipt_limit: u64,
    read: gazette_core::store::ReadOptions,
) -> Result<(Reservation, RestoreInspection), Error> {
    let reservation = Reservation::acquire(root)?;
    let inspection = inspect_restore_reserved(&reservation, pin, receipt_limit, read)?;
    Ok((reservation, inspection))
}

fn inspect_restore_reserved(
    reservation: &Reservation,
    pin: Sha256Digest,
    receipt_limit: u64,
    read: gazette_core::store::ReadOptions,
) -> Result<RestoreInspection, Error> {
    let identity = fs::symlink_metadata(reservation.path()).map_err(Error::Io)?;
    backup::check_directory(
        reservation.path(),
        &identity,
        &[backup::DATABASE_FILE, RECEIPT_FILE],
    )?;
    let receipt_path = reservation.path().join(RECEIPT_FILE);
    let (receipt_identity, bytes) = backup::read::read_manifest(&receipt_path, receipt_limit)?;
    if Sha256Digest::of(&bytes) != pin {
        return Err(Error::RestoreAdoptionVerification);
    }
    let preparation: RestorePreparation =
        serde_json::from_slice(&bytes).map_err(|_| Error::RestoreAdoptionVerification)?;
    preparation.validate()?;
    if serde_json::to_vec_pretty(&preparation).map_err(|_| Error::RestoreAdoptionVerification)?
        != bytes
    {
        return Err(Error::RestoreAdoptionVerification);
    }
    let path = reservation.path().join(backup::DATABASE_FILE);
    let database_identity = regular_file(&path)?;
    let (file, observed_bytes, observed_hash) =
        backup::fingerprint(&path, &database_identity, read.storage_bytes)?;
    drop(file);
    if observed_bytes != preparation.bytes || observed_hash != preparation.sha256 {
        return Err(Error::RestoreAdoptionVerification);
    }
    let (journal, transaction) = with_source(&path, read, |connection| {
        Ok((
            journal::inspect(connection)?,
            super::snapshot::capture(connection, read)?,
        ))
    })?;
    if journal.header != transaction.header
        || journal.verifier.application_id != preparation.verifier.application_id
        || journal.verifier.user_version != preparation.verifier.user_version
        || journal.verifier.journal_mode != "delete"
    {
        return Err(Error::RestoreAdoptionVerification);
    }
    let (after_identity, after_bytes) = backup::read::read_manifest(&receipt_path, receipt_limit)?;
    if !backup::same_file_state(&receipt_identity, &after_identity) || after_bytes != bytes {
        return Err(Error::Changed);
    }
    reservation.verify()?;
    Ok(RestoreInspection {
        directory: reservation.path().to_owned(),
        receipt_sha256: pin,
        source_manifest_sha256: preparation.source_manifest_sha256,
        database_sha256: observed_hash,
        journal,
        content_sha256: transaction.content_sha256,
    })
}

fn verify_restore_unchanged(
    reservation: &Reservation,
    source: &RestoreAdoptionSource,
    options: RestoreAdoptionOptions,
) -> Result<(), Error> {
    reservation.verify()?;
    let inspection = inspect_restore_reserved(
        reservation,
        source.receipt_sha256,
        options.receipt_bytes,
        options.read,
    )?;
    if inspection.content_sha256 != source.content_sha256
        || inspection.database_sha256 != source.database_sha256
        || inspection.source_manifest_sha256 != source.source_manifest_sha256
    {
        return Err(Error::RestoreAdoptionVerification);
    }
    Ok(())
}

fn copy_source(
    source: &Path,
    target: &Path,
    expected: Sha256Digest,
    limit: u64,
) -> Result<(), Error> {
    let source_identity = regular_file(source)?;
    let mut input = backup::open_unchanged(source, &source_identity)?;
    let mut output = backup::new_file(target)?;
    let copied = std::io::copy(
        &mut Read::by_ref(&mut input).take(limit.saturating_add(1)),
        &mut output,
    )
    .map_err(Error::Io)?;
    if copied > limit {
        return Err(Error::StorageLimit);
    }
    output
        .flush()
        .and_then(|()| output.sync_all())
        .map_err(Error::Io)?;
    drop(output);
    let target_identity = regular_file(target)?;
    let (file, bytes, hash) = backup::fingerprint(target, &target_identity, limit)?;
    drop(file);
    if bytes != copied
        || hash != expected
        || !backup::same_file_state(&source_identity, &regular_file(source)?)
    {
        return Err(Error::RestoreAdoptionVerification);
    }
    Ok(())
}

struct Advance<'a> {
    path: &'a Path,
    options: gazette_core::store::ReadOptions,
    anchor: &'a Anchor,
    previous: &'a gazette_core::store::LocalCoordinatorSeal,
    source: &'a RestoreAdoptionSource,
    next: &'a RestoreAdoptionTarget,
}

impl Advance<'_> {
    fn execute(self, adopting: &mut Observed) -> Result<(), Error> {
        let mut connection = Connection::open_with_flags(
            self.path,
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
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(native_error)?;
        let inspected = journal::inspect(&transaction)?;
        if inspected.header != self.previous.header || inspected.verifier.journal_mode != "delete" {
            return Err(Error::RestoreAdoptionVerification);
        }
        journal::advance_generation(&transaction, self.previous.header)?;
        let next_transaction = super::snapshot::capture(&transaction, self.options)?;
        if next_transaction.header != self.next.header {
            return Err(Error::RestoreAdoptionVerification);
        }
        match &adopting.state {
            LocalCoordinatorState::Adopting {
                transaction: Some(retained),
                ..
            } if retained == &next_transaction => {}
            LocalCoordinatorState::Adopting {
                transaction: None, ..
            } => {
                *adopting = self.anchor.write(
                    LocalCoordinatorState::Adopting {
                        previous: self.previous.clone(),
                        source: self.source.clone(),
                        next: self.next.clone(),
                        transaction: Some(next_transaction),
                    },
                    Some(adopting),
                )?;
            }
            _ => return Err(Error::RestoreAdoptionVerification),
        }
        transaction.commit().map_err(native_error)?;
        connection
            .close()
            .map_err(|(_, error)| native_error(error))?;
        Ok(())
    }
}
