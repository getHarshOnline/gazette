//! Immutable pending-authorization storage, not cryptographic approval or consumption.

use std::path::Path;

use gazette_core::{
    release::ReleaseDocument,
    render::LimitedWriter,
    source::Sha256Digest,
    store::{
        Error, JournalHeader, ReadOptions, ReleaseAuthorizationInput,
        ReleaseAuthorizationInspection, ReleaseAuthorizationRecord,
    },
};
use gazette_ports::store::ReleaseAuthorizationReader;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use crate::{Sqlite, native_error, preparation, registry, with_source};

// Native SQLite row/storage limits remain independent of the encoded payload budget.
const ROW_BYTES: usize = 256;

fn payload_limit(options: ReadOptions) -> Result<usize, Error> {
    options.validate()?;
    usize::try_from(options.value_bytes)
        .map_err(|_| Error::InvalidLimits)?
        .min(usize::try_from(options.storage_bytes).unwrap_or(usize::MAX))
        .checked_sub(ROW_BYTES)
        .ok_or(Error::StorageLimit)
}

fn encode(value: &impl Serialize, options: ReadOptions) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), payload_limit(options)?);
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.exceeded() {
            Error::StorageLimit
        } else {
            Error::ReleaseAuthorizationVerification
        });
    }
    Ok(output.into_inner())
}

pub(super) fn payload(
    input: ReleaseAuthorizationInput<'_>,
    options: ReadOptions,
) -> Result<Vec<u8>, Error> {
    #[derive(Serialize)]
    struct Record<'a> {
        #[serde(flatten)]
        input: ReleaseAuthorizationInput<'a>,
        created: JournalHeader,
    }
    // Bound escaped JSON and borrowed collections before allocating the decoded body.
    encode(
        &Record {
            input,
            created: *input.binding.journal(),
        },
        options,
    )
}

pub(super) fn check_preparations(
    connection: &Connection,
    input: ReleaseAuthorizationInput<'_>,
    body: &ReleaseDocument,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<(), Error> {
    for (id, target) in input.preparation_ids.iter().zip(body.ordered_targets()) {
        let record = preparation::read(connection, *id, current, options)?
            .ok_or(Error::ReleaseAuthorizationVerification)?;
        let completed = record
            .worktree
            .as_ref()
            .and_then(|worktree| worktree.completion.as_ref())
            .ok_or(Error::ReleaseAuthorizationVerification)?;
        if completed.report.commit != target.local_commit_oid
            || completed.report.output.sha256() != target.output_digest
        {
            return Err(Error::ReleaseAuthorizationVerification);
        }
    }
    Ok(())
}

pub(super) fn check_nonce(connection: &Connection, body: &ReleaseDocument) -> Result<(), Error> {
    let occupied: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM gazette_release_authorizations WHERE coordinator_id=?1 AND nonce=?2)",
        params![body.coordinator_id(), body.nonce()], |row| row.get(0)).map_err(native_error)?;
    if occupied {
        return Err(Error::ReleaseAuthorizationConflict);
    }
    Ok(())
}

// Caller owns one IMMEDIATE transaction covering this row and every preparation link.
pub(super) fn insert(
    connection: &Connection,
    body: &ReleaseDocument,
    ids: &[Sha256Digest],
    bytes: &[u8],
) -> Result<(), Error> {
    connection.execute(
        "INSERT INTO gazette_release_authorizations(record_id,coordinator_id,nonce,payload,payload_sha256) VALUES (?1,?2,?3,?4,?5)",
        params![body.record_id(), body.coordinator_id(), body.nonce(), bytes, Sha256Digest::of(bytes).as_bytes()],
    ).map_err(native_error)?;
    let mut links = connection.prepare("INSERT INTO gazette_release_authorization_preparations(record_id,ordinal,preparation_id) VALUES (?1,?2,?3)").map_err(native_error)?;
    for (ordinal, id) in ids.iter().enumerate() {
        links
            .execute(params![
                body.record_id(),
                i64::try_from(ordinal).map_err(|_| Error::StorageLimit)?,
                id.as_bytes()
            ])
            .map_err(native_error)?;
    }
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    record_id: &str,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<ReleaseAuthorizationRecord>, Error> {
    let maximum = payload_limit(options)?;
    if record_id.is_empty() || record_id.len() > maximum {
        return Err(Error::ReleaseAuthorizationVerification);
    }
    let mut statement = connection.prepare("SELECT record_id,coordinator_id,nonce,payload,payload_sha256 FROM gazette_release_authorizations WHERE record_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([record_id]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let key: String = row.get(0).map_err(native_error)?;
    let coordinator: String = row.get(1).map_err(native_error)?;
    let nonce: String = row.get(2).map_err(native_error)?;
    let bytes = row
        .get_ref(3)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReleaseAuthorizationVerification)?;
    if bytes.len() > maximum {
        return Err(Error::StorageLimit);
    }
    let digest: [u8; 32] = row
        .get(4)
        .map_err(|_| Error::ReleaseAuthorizationVerification)?;
    if Sha256Digest::of(bytes).as_bytes() != &digest {
        return Err(Error::ReleaseAuthorizationVerification);
    }
    let value =
        gazette_core::json::parse(bytes).map_err(|_| Error::ReleaseAuthorizationVerification)?;
    let record = ReleaseAuthorizationRecord::deserialize(&value)
        .map_err(|_| Error::ReleaseAuthorizationVerification)?;
    let body = record.as_input().validate()?;
    if key != record_id
        || body.record_id() != record_id
        || body.coordinator_id() != &coordinator
        || body.nonce() != &nonce
        || encode(&record, options)? != bytes
        || record.created != *record.binding.journal()
        || record.created.schema_version != current.schema_version
        || record.created.journal_id != current.journal_id
        || record.created.journal_epoch != current.journal_epoch
        || record.created.writer_fence > current.writer_fence
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::ReleaseAuthorizationVerification);
    }
    let mut statement = connection.prepare("SELECT ordinal,preparation_id FROM gazette_release_authorization_preparations WHERE record_id=?1 ORDER BY ordinal LIMIT ?2").map_err(native_error)?;
    let count = record.preparation_ids.len();
    let mut links = statement
        .query(params![
            record_id,
            i64::try_from(count.saturating_add(1)).map_err(|_| Error::StorageLimit)?
        ])
        .map_err(native_error)?;
    for (ordinal, id) in record.preparation_ids.iter().enumerate() {
        let link = links
            .next()
            .map_err(native_error)?
            .ok_or(Error::ReleaseAuthorizationVerification)?;
        let index: i64 = link
            .get(0)
            .map_err(|_| Error::ReleaseAuthorizationVerification)?;
        let stored: [u8; 32] = link
            .get(1)
            .map_err(|_| Error::ReleaseAuthorizationVerification)?;
        if usize::try_from(index).ok() != Some(ordinal) || &stored != id.as_bytes() {
            return Err(Error::ReleaseAuthorizationVerification);
        }
    }
    if links.next().map_err(native_error)?.is_some() {
        return Err(Error::ReleaseAuthorizationVerification);
    }
    // Every prerequisite had to exist by registration; later generations cannot
    // retroactively complete missing preparation history during recovery.
    check_preparations(
        connection,
        record.as_input(),
        &body,
        record.created,
        options,
    )?;
    Ok(Some(record))
}

impl ReleaseAuthorizationReader for Sqlite {
    fn inspect_release_authorization(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<ReleaseAuthorizationInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let record = read(connection, record_id, inspection.journal.header, options)?;
            Ok(ReleaseAuthorizationInspection {
                journal: inspection.journal,
                record,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/authorization.rs"]
mod tests;
