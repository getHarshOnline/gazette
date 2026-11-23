//! Immutable complete ref history; native acquisition remains application-owned.

use std::path::Path;

use gazette_core::{
    render::LimitedWriter,
    source::Sha256Digest,
    store::{
        Error, JournalHeader, ReadOptions, RefObservation, RefObservationInput,
        RefObservationInspection,
    },
};
use gazette_ports::store::RefObservationReader;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{Sqlite, native_error, registry, with_source};

// Two fixed digest columns and conservative per-row bookkeeping, separate from
// the bounded JSON payload. SQLite's connection/storage ceilings remain active.
const ROW_BYTES: usize = 128;

fn payload_limit(options: ReadOptions) -> Result<usize, Error> {
    options.validate()?;
    let value = usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?;
    let storage = usize::try_from(options.storage_bytes).unwrap_or(usize::MAX);
    value
        .min(storage)
        .checked_sub(ROW_BYTES)
        .ok_or(Error::StorageLimit)
}

fn encode(value: &impl Serialize, options: ReadOptions) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), payload_limit(options)?);
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.exceeded() {
            Error::StorageLimit
        } else {
            Error::ReadbackVerification
        });
    }
    Ok(output.into_inner())
}

/// Encode borrowed fields under native bounds without first cloning the target set.
pub(super) fn payload(
    input: RefObservationInput<'_>,
    created: JournalHeader,
    options: ReadOptions,
) -> Result<Vec<u8>, Error> {
    options.validate()?;
    input.validate()?;
    #[derive(Serialize)]
    struct Record<'a> {
        #[serde(flatten)]
        input: RefObservationInput<'a>,
        created: JournalHeader,
    }
    encode(&Record { input, created }, options)
}

pub(super) fn insert(
    connection: &Connection,
    id: Sha256Digest,
    payload: &[u8],
) -> Result<(), Error> {
    connection
        .execute(
            "INSERT INTO gazette_ref_observations(id,payload,payload_sha256) VALUES (?1,?2,?3)",
            params![id.as_bytes(), payload, Sha256Digest::of(payload).as_bytes()],
        )
        .map_err(native_error)?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: Sha256Digest,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<RefObservation>, Error> {
    let maximum = payload_limit(options)?;
    let mut statement = connection
        .prepare(
            "SELECT id,payload,payload_sha256 FROM gazette_ref_observations WHERE id=?1 LIMIT 2",
        )
        .map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let stored_id: [u8; 32] = row.get(0).map_err(|_| Error::ReadbackVerification)?;
    let bytes = row
        .get_ref(1)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReadbackVerification)?;
    if bytes.len() > maximum {
        return Err(Error::StorageLimit);
    }
    let digest: [u8; 32] = row.get(2).map_err(|_| Error::ReadbackVerification)?;
    if &stored_id != id.as_bytes() || Sha256Digest::of(bytes).as_bytes() != &digest {
        return Err(Error::ReadbackVerification);
    }
    let value = gazette_core::json::parse(bytes).map_err(|_| Error::ReadbackVerification)?;
    let record = RefObservation::deserialize(&value).map_err(|_| Error::ReadbackVerification)?;
    record.as_input().validate()?;
    if encode(&record, options)? != bytes
        || record.id != id
        || record.created.schema_version != current.schema_version
        || record.created.journal_id != current.journal_id
        || record.created.journal_epoch != current.journal_epoch
        || record.created.writer_fence == 0
        || record.created.writer_fence > current.writer_fence
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::ReadbackVerification);
    }
    Ok(Some(record))
}

impl RefObservationReader for Sqlite {
    fn inspect_ref_observation(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<RefObservationInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let record = read(connection, id, inspection.journal.header, options)?;
            Ok(RefObservationInspection {
                journal: inspection.journal,
                record,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/readback.rs"]
mod tests;
