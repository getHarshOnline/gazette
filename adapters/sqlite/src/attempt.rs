//! Exact append-only dispatch history; incomplete starts are retained, never retried by storage.

use crate::{Sqlite, authorization, consumption, journal, native_error, with_source};
use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, ReadOptions, ReleaseAttempt, ReleaseAttemptInspection, ReleaseAttemptResult,
        ReleaseAttemptStart,
    },
};
use gazette_ports::store::ReleaseAttemptReader;
use rusqlite::{Connection, params, types::ValueRef};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::Path;

const START_DOMAIN: &[u8] = b"GAZETTE_RELEASE_ATTEMPT_START_V1\0";
const RESULT_DOMAIN: &[u8] = b"GAZETTE_RELEASE_ATTEMPT_RESULT_V1\0";

/// Complete global history, never a batch subset or a shortcut based on result/cleanup labels.
pub(super) fn require_recovery(
    connection: &Connection,
    profile: gazette_core::store::RecoveryProfile,
    options: ReadOptions,
) -> Result<(), Error> {
    let mut statement = connection.prepare(
        "SELECT authorization_id,COUNT(*) FROM gazette_release_attempt_starts GROUP BY authorization_id ORDER BY authorization_id"
    ).map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let mut total = 0_u64;
    while let Some(row) = rows.next().map_err(native_error)? {
        if profile == gazette_core::store::RecoveryProfile::Unstarted {
            return Err(Error::ReleaseQuiescenceRequired);
        }
        let id = row
            .get_ref(0)
            .map_err(native_error)?
            .as_str()
            .map_err(|_| Error::ReleaseAttemptVerification)?;
        if id.len() > consumption::maximum(options)? {
            return Err(Error::StorageLimit);
        }
        let count = u64::try_from(row.get::<_, i64>(1).map_err(native_error)?)
            .map_err(|_| Error::ReleaseAttemptVerification)?;
        total = total.checked_add(count).ok_or(Error::StorageLimit)?;
        if count == 0 || total > options.storage_bytes / 128 {
            return Err(Error::StorageLimit);
        }
        // Reuse the native canonical payload/digest, consumed-target, preparation,
        // authorization and ordered-start/result verification, even for completed rows.
        let history = read(connection, id, options)?;
        if history.consumption.is_none()
            || u64::try_from(history.attempts.len()).ok() != Some(count)
        {
            return Err(Error::ReleaseAttemptVerification);
        }
        if profile == gazette_core::store::RecoveryProfile::LocalTransactions
            && history.attempts.iter().any(|row| {
                row.start.profile != gazette_core::release::DispatchProfile::LocalTransaction
            })
        {
            return Err(Error::ReleaseQuiescenceRequired);
        }
    }
    Ok(())
}

fn digest(domain: &[u8], payload: &[u8]) -> Sha256Digest {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(payload);
    Sha256Digest::from(<[u8; 32]>::from(hash.finalize()))
}

fn decode<T: DeserializeOwned + Serialize>(
    payload: &[u8],
    hash: &[u8],
    domain: &[u8],
    options: ReadOptions,
) -> Result<T, Error> {
    if payload.len() > consumption::maximum(options)? {
        return Err(Error::StorageLimit);
    }
    if digest(domain, payload).as_bytes() != hash {
        return Err(Error::ReleaseAttemptVerification);
    }
    let value =
        gazette_core::json::parse(payload).map_err(|_| Error::ReleaseAttemptVerification)?;
    let record = T::deserialize(&value).map_err(|_| Error::ReleaseAttemptVerification)?;
    if consumption::encode(&record, options)? != payload {
        return Err(Error::ReleaseAttemptVerification);
    }
    Ok(record)
}

pub(super) fn insert_start(
    connection: &Connection,
    start: &ReleaseAttemptStart,
    options: ReadOptions,
) -> Result<(), Error> {
    let payload = consumption::encode(start, options)?;
    connection.execute(
        "INSERT INTO gazette_release_attempt_starts(authorization_id,ordinal,number,payload,payload_sha256) VALUES (?1,?2,?3,?4,?5)",
        params![start.key.authorization_id, start.key.ordinal, i64::try_from(start.key.number).map_err(|_| Error::StorageLimit)?, payload, digest(START_DOMAIN, &payload).as_bytes()],
    ).map_err(native_error)?;
    Ok(())
}

pub(super) fn insert_result(
    connection: &Connection,
    result: &ReleaseAttemptResult,
    options: ReadOptions,
) -> Result<(), Error> {
    let payload = consumption::encode(result, options)?;
    connection.execute(
        "INSERT INTO gazette_release_attempt_results(authorization_id,ordinal,number,payload,payload_sha256) VALUES (?1,?2,?3,?4,?5)",
        params![result.key.authorization_id, result.key.ordinal, i64::try_from(result.key.number).map_err(|_| Error::StorageLimit)?, payload, digest(RESULT_DOMAIN, &payload).as_bytes()],
    ).map_err(native_error)?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: &str,
    options: ReadOptions,
) -> Result<ReleaseAttemptInspection, Error> {
    let inspection = journal::inspect(connection)?;
    let current = inspection.header;
    let consumed = consumption::read(connection, id, current, options)?;
    let Some(consumed) = consumed else {
        return Ok(ReleaseAttemptInspection {
            journal: inspection,
            consumption: None,
            attempts: vec![],
        });
    };
    let registration = authorization::read(connection, id, current, options)?
        .ok_or(Error::ReleaseAttemptVerification)?;
    // Storage bounds cap both each record and the aggregate retained history.
    let maximum = i64::try_from(options.storage_bytes / 128).map_err(|_| Error::StorageLimit)?;
    let mut statement = connection.prepare(
        "SELECT s.ordinal,s.number,s.payload,s.payload_sha256,r.payload,r.payload_sha256 FROM gazette_release_attempt_starts s LEFT JOIN gazette_release_attempt_results r ON r.authorization_id=s.authorization_id AND r.ordinal=s.ordinal AND r.number=s.number WHERE s.authorization_id=?1 ORDER BY s.ordinal,s.number LIMIT ?2",
    ).map_err(native_error)?;
    let mut rows = statement
        .query(params![id, maximum.saturating_add(1)])
        .map_err(native_error)?;
    let mut attempts: Vec<ReleaseAttempt> = Vec::new();
    let mut retained = 0_u64;
    while let Some(row) = rows.next().map_err(native_error)? {
        if i64::try_from(attempts.len()).map_err(|_| Error::StorageLimit)? >= maximum {
            return Err(Error::StorageLimit);
        }
        let ordinal: i64 = row.get(0).map_err(native_error)?;
        let number: i64 = row.get(1).map_err(native_error)?;
        let blob = |column| {
            row.get_ref(column)
                .map_err(native_error)?
                .as_blob()
                .map_err(|_| Error::ReleaseAttemptVerification)
        };
        let payload = blob(2)?;
        retained = retained
            .checked_add(u64::try_from(payload.len()).map_err(|_| Error::StorageLimit)?)
            .ok_or(Error::StorageLimit)?;
        if retained > options.storage_bytes {
            return Err(Error::StorageLimit);
        }
        let start: ReleaseAttemptStart = decode(payload, blob(3)?, START_DOMAIN, options)?;
        if start.key.authorization_id != id
            || u32::try_from(ordinal).ok() != Some(start.key.ordinal)
            || u64::try_from(number).ok() != Some(start.key.number)
        {
            return Err(Error::ReleaseAttemptVerification);
        }
        start.validate(&registration, &consumed)?;
        start.follows(
            attempts
                .last()
                .filter(|previous| previous.start.key.ordinal == start.key.ordinal),
        )?;
        let result = match (
            row.get_ref(4).map_err(native_error)?,
            row.get_ref(5).map_err(native_error)?,
        ) {
            (ValueRef::Null, ValueRef::Null) => None,
            (ValueRef::Blob(payload), ValueRef::Blob(hash)) => {
                retained = retained
                    .checked_add(u64::try_from(payload.len()).map_err(|_| Error::StorageLimit)?)
                    .ok_or(Error::StorageLimit)?;
                if retained > options.storage_bytes {
                    return Err(Error::StorageLimit);
                }
                let result: ReleaseAttemptResult = decode(payload, hash, RESULT_DOMAIN, options)?;
                result.validate(&start, &consumed)?;
                Some(result)
            }
            _ => return Err(Error::ReleaseAttemptVerification),
        };
        attempts.push(ReleaseAttempt { start, result });
    }
    Ok(ReleaseAttemptInspection {
        journal: inspection,
        consumption: Some(consumed),
        attempts,
    })
}

impl ReleaseAttemptReader for Sqlite {
    fn inspect_release_attempts(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<ReleaseAttemptInspection, Error> {
        with_source(path, options, |connection| {
            read(connection, record_id, options)
        })
    }
}
