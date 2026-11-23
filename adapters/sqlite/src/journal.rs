//! Journal coordinates and read-back; schema versioning belongs to `schema`.

use crate::{Sqlite, native_error, read_metadata, schema, with_source};
use gazette_core::store::{Error, JournalHeader, JournalInspection, ReadOptions};
use gazette_ports::store::JournalReader;
use rusqlite::Connection;
use std::path::Path;

pub(super) const DATABASE: &str = "store.sqlite";
pub(super) const INITIAL_EPOCH: u32 = 1;

// Caller owns the IMMEDIATE transaction and verifies schema/header inside it.
// Keep storage SQL here, never duplicate the coordinate mapping in interfaces.
pub(super) fn advance_generation(
    connection: &Connection,
    expected: JournalHeader,
) -> Result<(), Error> {
    let epoch = i64::try_from(expected.journal_epoch).map_err(|_| Error::JournalMismatch)?;
    let fence = i64::try_from(expected.writer_fence).map_err(|_| Error::WriterExhausted)?;
    let next = fence.checked_add(1).ok_or(Error::WriterExhausted)?;
    let changed = connection.execute(
        "UPDATE gazette_journal SET writer_fence=?1 WHERE singleton=1 AND journal_id=?2 AND journal_epoch=?3 AND writer_fence=?4",
        rusqlite::params![next, expected.journal_id.as_slice(), epoch, fence],
    ).map_err(native_error)?;
    if changed != 1 {
        return Err(Error::JournalMismatch);
    }
    Ok(())
}

impl JournalReader for Sqlite {
    fn inspect_journal(
        &self,
        path: &Path,
        options: ReadOptions,
    ) -> Result<JournalInspection, Error> {
        with_source(path, options, inspect)
    }
}

// Called only by the initial migration's transaction hook. Schema versioning
// stays with schema; this module owns the journal coordinates and their SQL.
pub(super) fn seed(connection: &Connection) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT INTO gazette_journal (singleton, journal_id, journal_epoch, writer_fence) VALUES (1, randomblob(32), ?1, 0)",
        [INITIAL_EPOCH],
    )?;
    Ok(())
}

// The caller owns the SQL snapshot and native file lifetime. Initialization and
// existing-journal inspection use this same schema/header/integrity checker.
pub(super) fn inspect(connection: &Connection) -> Result<JournalInspection, Error> {
    let verifier = read_metadata(connection)?;
    schema::verify(connection, &verifier)?;
    let header = read_header(connection)?;
    let integrity: String = connection
        .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
        .map_err(native_error)?;
    if integrity != "ok" {
        return Err(Error::JournalVerification);
    }
    foreign_keys(connection)?;
    Ok(JournalInspection {
        header,
        verifier,
        schema_sha256: schema::digest(),
    })
}

pub(super) fn foreign_keys(connection: &Connection) -> Result<(), Error> {
    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(native_error)?;
    if statement
        .query([])
        .map_err(native_error)?
        .next()
        .map_err(native_error)?
        .is_some()
    {
        return Err(Error::JournalVerification);
    }
    Ok(())
}

fn read_header(connection: &Connection) -> Result<JournalHeader, Error> {
    let schema_version = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(native_error)?;
    if schema_version != schema::VERSION {
        return Err(Error::JournalSchema);
    }
    // Check the whole bounded coordinate set, not only WHERE singleton=1.
    // An external writer may have bypassed CHECK constraints; native integrity
    // diagnostics are supplementary, not proof of this application invariant.
    let mut statement = connection.prepare(
        "SELECT singleton, journal_id, journal_epoch, writer_fence FROM gazette_journal LIMIT 2",
    ).map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let row = rows
        .next()
        .map_err(native_error)?
        .ok_or(Error::JournalVerification)?;
    let (singleton, journal_id, epoch, fence): (i64, [u8; 32], i64, i64) = (
        row.get(0).map_err(|_| Error::JournalVerification)?,
        row.get(1).map_err(|_| Error::JournalVerification)?,
        row.get(2).map_err(|_| Error::JournalVerification)?,
        row.get(3).map_err(|_| Error::JournalVerification)?,
    );
    if singleton != 1 || rows.next().map_err(native_error)?.is_some() {
        return Err(Error::JournalVerification);
    }
    let journal_epoch = u64::try_from(epoch)
        .ok()
        .filter(|&epoch| epoch > 0)
        .ok_or(Error::JournalVerification)?;
    let writer_fence = u64::try_from(fence).map_err(|_| Error::JournalVerification)?;
    Ok(JournalHeader {
        schema_version,
        journal_id,
        journal_epoch,
        writer_fence,
    })
}
