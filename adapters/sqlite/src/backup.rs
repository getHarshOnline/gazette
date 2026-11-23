//! Verified snapshots and recovery copies confined to fresh private output.

use gazette_core::source::Sha256Digest;
use gazette_core::store::{
    BackupOptions, BackupReceipt, BackupSnapshot, Error, Observation, OutputError, ReadOptions,
};
use gazette_ports::store::StoreBackup;
use rusqlite::{
    Connection, OpenFlags,
    backup::{Backup, StepResult},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

use super::{Sqlite, native_error, read_metadata, regular_file, with_source};

mod output;
pub(super) mod read;
mod restore;

pub(super) const DATABASE_FILE: &str = BackupSnapshot::DATABASE_FILE;
const RECEIPT_FILE: &str = "backup.json";

impl StoreBackup for Sqlite {
    fn backup(
        &self,
        source: &Path,
        parent: &Path,
        options: BackupOptions,
    ) -> Result<BackupReceipt, OutputError> {
        options.validate()?;
        let mut created = None;
        let result = (|| {
            let (output, snapshot) = with_source(source, options.read, |connection| {
                // Header reads establish the read snapshot before any destination exists.
                let observation = read_metadata(connection)?;
                check_size(connection, options.database_bytes)?;
                let output =
                    output::OutputDirectory::create(parent, "gazette-backup-", options.output)
                        .map_err(|error| {
                            created = error.directory;
                            error.cause
                        })?;
                created = Some(output.path().to_path_buf());
                let snapshot = copy(connection, output.path(), observation, options)?;
                Ok((output, snapshot))
            })?;
            let manifest_sha256 = output.seal(RECEIPT_FILE, &snapshot)?;
            Ok(BackupReceipt {
                directory: output.path().to_path_buf(),
                snapshot,
                manifest_sha256,
            })
        })();
        result.map_err(|cause| OutputError {
            directory: created,
            cause,
        })
    }
}

fn copy(
    source: &Connection,
    directory: &Path,
    observation: Observation,
    options: BackupOptions,
) -> Result<BackupSnapshot, Error> {
    let path = directory.join(DATABASE_FILE);
    // Exclusive creation happens before SQLite opens it, with no CREATE flag.
    // The private directory is never reused; existing database/sidecars cannot collide.
    let identity = new_file(&path)?.metadata().map_err(Error::Io)?;
    let mut destination = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(native_error)?;
    destination
        .busy_timeout(std::time::Duration::ZERO)
        .map_err(native_error)?;
    destination
        .execute_batch("PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")
        .map_err(native_error)?;
    {
        let backup = Backup::new(source, &mut destination).map_err(native_error)?;
        let mut done = false;
        for _ in 0..options.steps {
            match backup.step(options.pages_per_step).map_err(native_error)? {
                StepResult::Done => {
                    done = true;
                    break;
                }
                StepResult::More => (),
                StepResult::Busy | StepResult::Locked => return Err(Error::Busy),
                _ => return Err(Error::Provider { code: None }),
            }
        }
        if !done {
            return Err(Error::BackupLimit);
        }
    }
    // A standalone backup must not depend on inherited WAL sidecars.
    destination
        .execute_batch("PRAGMA journal_mode=DELETE;")
        .map_err(native_error)?;
    destination
        .close()
        .map_err(|(_, error)| native_error(error))?;
    // No filesystem read/close of the database while SQLite owns a live handle:
    // doing that can invalidate process-local POSIX advisory locks.
    check_copy(&path, &observation, options.read)?;
    let (file, bytes, sha256) = fingerprint(&path, &identity, options.database_bytes)?;
    file.sync_all().map_err(Error::Io)?;
    Ok(BackupSnapshot {
        format: BackupSnapshot::FORMAT.into(),
        database: DATABASE_FILE.into(),
        source: observation,
        bytes,
        sha256,
    })
}

fn check_copy(
    path: &Path,
    source: &Observation,
    options: ReadOptions,
) -> Result<Observation, Error> {
    with_source(path, options, |connection| {
        let copied = read_metadata(connection)?;
        if copied.application_id != source.application_id
            || copied.user_version != source.user_version
            || copied.journal_mode != "delete"
        {
            return Err(Error::BackupVerification);
        }
        let integrity: String = connection
            .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
            .map_err(native_error)?;
        if integrity != "ok" {
            return Err(Error::BackupVerification);
        }
        Ok(copied)
    })
}

pub(super) fn fingerprint(
    path: &Path,
    identity: &fs::Metadata,
    limit: u64,
) -> Result<(File, u64, Sha256Digest), Error> {
    let metadata = regular_file(path)?;
    if (identity.dev(), identity.ino()) != (metadata.dev(), metadata.ino()) {
        return Err(Error::Changed);
    }
    if metadata.len() > limit {
        return Err(Error::BackupLimit);
    }
    if metadata.len() < 100 {
        return Err(Error::BackupVerification);
    }
    let mut file = open_unchanged(path, &metadata)?;
    let mut hash = Sha256::new();
    // Require a standalone rollback-mode header before a later SQLite open.
    // The directory inventory check separately rejects recovery sidecars.
    let mut header = [0; 100];
    file.read_exact(&mut header).map_err(Error::Io)?;
    if &header[..16] != b"SQLite format 3\0" || header[18] != 1 || header[19] != 1 {
        return Err(Error::BackupVerification);
    }
    hash.update(header);
    let mut bytes = header.len() as u64;
    let mut buffer = [0_u8; 65536];
    loop {
        let count = file.read(&mut buffer).map_err(Error::Io)?;
        if count == 0 {
            break;
        }
        bytes = bytes
            .checked_add(count as u64)
            .filter(|&n| n <= limit)
            .ok_or(Error::BackupLimit)?;
        hash.update(&buffer[..count]);
    }
    if bytes != metadata.len()
        || !same_file_state(&metadata, &file.metadata().map_err(Error::Io)?)
        || !same_file_state(&metadata, &regular_file(path)?)
    {
        return Err(Error::Changed);
    }
    Ok((file, bytes, <[u8; 32]>::from(hash.finalize()).into()))
}

// Metadata checks alone cannot constrain a later open: a replaced FIFO would
// block before descriptor validation, and a symlink could follow the same inode.
// Ancestors remain trusted; these flags constrain the selected final component.
pub(super) fn open_unchanged(path: &Path, identity: &fs::Metadata) -> Result<File, Error> {
    use rustix::fs::{Mode, OFlags, open};

    let descriptor = open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|error| {
        if error == rustix::io::Errno::LOOP {
            Error::UnsafePath
        } else {
            Error::Io(error.into())
        }
    })?;
    let file = File::from(descriptor);
    let opened = file.metadata().map_err(Error::Io)?;
    if !opened.is_file() || !same_file_state(identity, &opened) {
        return Err(Error::Changed);
    }
    Ok(file)
}

pub(super) fn same_file_state(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    (
        before.dev(),
        before.ino(),
        before.len(),
        before.mtime(),
        before.mtime_nsec(),
        before.ctime(),
        before.ctime_nsec(),
    ) == (
        after.dev(),
        after.ino(),
        after.len(),
        after.mtime(),
        after.mtime_nsec(),
        after.ctime(),
        after.ctime_nsec(),
    )
}

fn check_size(connection: &Connection, limit: u64) -> Result<(), Error> {
    let pages: i64 = connection
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .map_err(native_error)?;
    let size: i64 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(native_error)?;
    if pages <= 0
        || size <= 0
        || pages
            .checked_mul(size)
            .and_then(|bytes| u64::try_from(bytes).ok())
            .is_none_or(|bytes| bytes > limit)
    {
        return Err(Error::BackupLimit);
    }
    Ok(())
}

pub(super) fn new_file(path: &Path) -> Result<File, Error> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(Error::Io)
}

pub(super) fn check_directory(
    path: &Path,
    identity: &fs::Metadata,
    expected: &[&str],
) -> Result<(), Error> {
    let current = fs::symlink_metadata(path).map_err(Error::Io)?;
    if !current.is_dir() || (identity.dev(), identity.ino()) != (current.dev(), current.ino()) {
        return Err(Error::Changed);
    }
    let mut count = 0;
    for entry in fs::read_dir(path).map_err(Error::Io)? {
        let entry = entry.map_err(Error::Io)?;
        if !entry.file_type().map_err(Error::Io)?.is_file()
            || !expected.iter().any(|name| entry.file_name() == *name)
        {
            return Err(Error::Changed);
        }
        count += 1;
    }
    if count != expected.len() {
        return Err(Error::Changed);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../tests/internal/backup.rs"]
mod tests;
