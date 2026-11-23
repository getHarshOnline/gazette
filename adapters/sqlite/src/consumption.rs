//! Immutable local lease generations and one-use batch/target history.

use crate::{Sqlite, authorization, native_error, registry, with_source};
use gazette_core::{
    render::LimitedWriter,
    source::Sha256Digest,
    store::{
        Error, JournalHeader, ReadOptions, ReleaseAuthorizationRecord, ReleaseConsumption,
        ReleaseConsumptionInspection, ReleaseCoordinator, ReleaseLease, ReleaseLeaseInspection,
        ReleaseLeases, ReleaseTargetIntent,
    },
};
use gazette_ports::store::ReleaseConsumptionReader;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::path::Path;

const LEASE_DOMAIN: &[u8] = b"GAZETTE_RELEASE_LEASE_SET_V1\0";
const BATCH_DOMAIN: &[u8] = b"WEBSITE_RELEASE_BATCH_INTENT_V1\0";
const TARGET_DOMAIN: &[u8] = b"GAZETTE_RELEASE_TARGET_INTENT_V1\0";
const ROW_BYTES: usize = 256;

pub(super) fn maximum(options: ReadOptions) -> Result<usize, Error> {
    options.validate()?;
    usize::try_from(options.value_bytes)
        .map_err(|_| Error::InvalidLimits)?
        .min(usize::try_from(options.storage_bytes).unwrap_or(usize::MAX))
        .checked_sub(ROW_BYTES)
        .ok_or(Error::StorageLimit)
}
pub(super) fn encode(value: &impl Serialize, options: ReadOptions) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), maximum(options)?);
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.exceeded() {
            Error::StorageLimit
        } else {
            Error::ReleaseConsumptionVerification
        });
    }
    Ok(output.into_inner())
}
fn digest(domain: &[u8], bytes: &[u8]) -> Sha256Digest {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(bytes);
    Sha256Digest::from(<[u8; 32]>::from(hash.finalize()))
}
fn decode<T: DeserializeOwned + Serialize>(
    bytes: &[u8],
    hash: &[u8],
    domain: &[u8],
    options: ReadOptions,
) -> Result<T, Error> {
    if bytes.len() > maximum(options)? {
        return Err(Error::StorageLimit);
    }
    if digest(domain, bytes).as_bytes() != hash {
        return Err(Error::ReleaseConsumptionVerification);
    }
    let value =
        gazette_core::json::parse(bytes).map_err(|_| Error::ReleaseConsumptionVerification)?;
    let record = T::deserialize(&value).map_err(|_| Error::ReleaseConsumptionVerification)?;
    if encode(&record, options)? != bytes {
        return Err(Error::ReleaseConsumptionVerification);
    }
    Ok(record)
}
fn key(id: &str, options: ReadOptions) -> Result<(), Error> {
    if id.is_empty() || id.len() > maximum(options)? {
        return Err(Error::ReleaseConsumptionVerification);
    }
    Ok(())
}

pub(super) fn consumed(connection: &Connection, id: &str) -> Result<bool, Error> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM gazette_release_consumptions WHERE authorization_id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(native_error)
}

pub(super) fn read_leases(
    connection: &Connection,
    id: &str,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<ReleaseLeases>, Error> {
    key(id, options)?;
    let mut statement = connection.prepare("SELECT payload,payload_sha256 FROM gazette_release_lease_sets WHERE authorization_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let bytes = row
        .get_ref(0)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReleaseLeaseVerification)?;
    let hash = row
        .get_ref(1)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReleaseLeaseVerification)?;
    let leases: ReleaseLeases = decode(bytes, hash, LEASE_DOMAIN, options)?;
    let registration = authorization::read(connection, id, current, options)?
        .ok_or(Error::ReleaseLeaseVerification)?;
    leases.validate(&registration)?;
    if leases.authorization_id != id || rows.next().map_err(native_error)?.is_some() {
        return Err(Error::ReleaseLeaseVerification);
    }
    let mut statement = connection.prepare("SELECT ordinal,remote,branch,generation FROM gazette_release_lease_history WHERE authorization_id=?1 ORDER BY ordinal LIMIT ?2").map_err(native_error)?;
    let mut rows = statement
        .query(params![
            id,
            i64::try_from(leases.targets.len().saturating_add(1))
                .map_err(|_| Error::StorageLimit)?
        ])
        .map_err(native_error)?;
    for lease in &leases.targets {
        let row = rows
            .next()
            .map_err(native_error)?
            .ok_or(Error::ReleaseLeaseVerification)?;
        let ordinal: i64 = row.get(0).map_err(native_error)?;
        let remote: String = row.get(1).map_err(native_error)?;
        let branch: String = row.get(2).map_err(native_error)?;
        let generation: i64 = row.get(3).map_err(native_error)?;
        if u32::try_from(ordinal).ok() != Some(lease.ordinal)
            || remote != lease.remote
            || branch != lease.branch
            || u64::try_from(generation).ok() != Some(lease.generation)
        {
            return Err(Error::ReleaseLeaseVerification);
        }
    }
    if rows.next().map_err(native_error)?.is_some() {
        return Err(Error::ReleaseLeaseVerification);
    }
    Ok(Some(leases))
}

fn latest(
    connection: &Connection,
    remote: &str,
    branch: &str,
) -> Result<Option<(String, u64)>, Error> {
    let value: Option<(String, i64)> = connection.query_row("SELECT authorization_id,generation FROM gazette_release_lease_history WHERE remote=?1 AND branch=?2 ORDER BY generation DESC LIMIT 1", params![remote, branch], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(native_error)?;
    value
        .map(|(id, generation)| {
            Ok((
                id,
                u64::try_from(generation)
                    .ok()
                    .filter(|value| *value > 0)
                    .ok_or(Error::ReleaseLeaseVerification)?,
            ))
        })
        .transpose()
}

pub(super) fn current_leases(connection: &Connection, leases: &ReleaseLeases) -> Result<(), Error> {
    for lease in &leases.targets {
        if latest(connection, &lease.remote, &lease.branch)?
            != Some((leases.authorization_id.clone(), lease.generation))
        {
            return Err(Error::ReleaseLeaseConflict);
        }
    }
    Ok(())
}

pub(super) fn next_leases(
    connection: &Connection,
    registration: &ReleaseAuthorizationRecord,
    coordinator: ReleaseCoordinator,
    options: ReadOptions,
) -> Result<ReleaseLeases, Error> {
    let body = registration.as_input().validate()?;
    if consumed(connection, body.record_id())? {
        return Err(Error::ReleaseAlreadyConsumed);
    }
    let mut targets = Vec::new();
    for target in body.ordered_targets() {
        let generation = match latest(connection, &target.remote, &target.branch)? {
            None => 1,
            Some((previous_id, generation)) => {
                let previous = read_leases(connection, &previous_id, coordinator.journal, options)?
                    .ok_or(Error::ReleaseLeaseVerification)?;
                if previous.coordinator != coordinator
                    && !coordinator.continues(&previous.coordinator)
                {
                    return Err(Error::ReleaseLeaseConflict);
                }
                if consumed(connection, &previous_id)? {
                    if !crate::settlement::terminal(connection, &previous_id, options)? {
                        return Err(Error::ReleaseLeaseConflict);
                    }
                } else if previous.coordinator.journal.writer_fence
                    >= coordinator.journal.writer_fence
                {
                    return Err(Error::ReleaseLeaseConflict);
                }
                generation
                    .checked_add(1)
                    .filter(|value| *value <= i64::MAX as u64)
                    .ok_or(Error::WriterExhausted)?
            }
        };
        targets.push(ReleaseLease {
            ordinal: target.ordinal,
            remote: target.remote.clone(),
            branch: target.branch.clone(),
            generation,
        });
    }
    let leases = ReleaseLeases {
        authorization_id: body.record_id().clone(),
        coordinator,
        targets,
    };
    leases.validate(registration)?;
    encode(&leases, options)?;
    Ok(leases)
}

pub(super) fn insert_leases(
    connection: &Connection,
    leases: &ReleaseLeases,
    options: ReadOptions,
) -> Result<(), Error> {
    let bytes = encode(leases, options)?;
    connection.execute("INSERT INTO gazette_release_lease_sets(authorization_id,payload,payload_sha256) VALUES (?1,?2,?3)", params![leases.authorization_id, bytes, digest(LEASE_DOMAIN, &bytes).as_bytes()]).map_err(native_error)?;
    let mut statement = connection.prepare("INSERT INTO gazette_release_lease_history(remote,branch,generation,authorization_id,ordinal) VALUES (?1,?2,?3,?4,?5)").map_err(native_error)?;
    for lease in &leases.targets {
        statement
            .execute(params![
                lease.remote,
                lease.branch,
                i64::try_from(lease.generation).map_err(|_| Error::WriterExhausted)?,
                leases.authorization_id,
                lease.ordinal
            ])
            .map_err(native_error)?;
    }
    Ok(())
}

#[derive(Serialize)]
struct TargetRecord<'a> {
    authorization_id: &'a str,
    intent: &'a ReleaseTargetIntent,
}

pub(super) fn insert_consumption(
    connection: &Connection,
    record: &ReleaseConsumption,
    options: ReadOptions,
) -> Result<(), Error> {
    let bytes = encode(record, options)?;
    connection.execute("INSERT INTO gazette_release_consumptions(authorization_id,payload,payload_sha256) VALUES (?1,?2,?3)", params![record.authorization_id, bytes, digest(BATCH_DOMAIN, &bytes).as_bytes()]).map_err(native_error)?;
    let mut statement = connection.prepare("INSERT INTO gazette_release_target_intents(authorization_id,ordinal,remote,branch,lease_generation,payload,payload_sha256) VALUES (?1,?2,?3,?4,?5,?6,?7)").map_err(native_error)?;
    for intent in &record.targets {
        let bytes = encode(
            &TargetRecord {
                authorization_id: &record.authorization_id,
                intent,
            },
            options,
        )?;
        statement
            .execute(params![
                record.authorization_id,
                intent.target.ordinal,
                intent.target.remote,
                intent.target.branch,
                i64::try_from(intent.lease_generation).map_err(|_| Error::WriterExhausted)?,
                bytes,
                digest(TARGET_DOMAIN, &bytes).as_bytes()
            ])
            .map_err(native_error)?;
    }
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: &str,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<ReleaseConsumption>, Error> {
    key(id, options)?;
    let mut statement = connection.prepare("SELECT payload,payload_sha256 FROM gazette_release_consumptions WHERE authorization_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let bytes = row
        .get_ref(0)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReleaseConsumptionVerification)?;
    let hash = row
        .get_ref(1)
        .map_err(native_error)?
        .as_blob()
        .map_err(|_| Error::ReleaseConsumptionVerification)?;
    let record: ReleaseConsumption = decode(bytes, hash, BATCH_DOMAIN, options)?;
    let registration = authorization::read(connection, id, current, options)?
        .ok_or(Error::ReleaseConsumptionVerification)?;
    record.validate(&registration)?;
    let leases =
        read_leases(connection, id, current, options)?.ok_or(Error::ReleaseLeaseVerification)?;
    if record.authorization_id != id
        || record.leases != leases
        || record.created != registration.created
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::ReleaseConsumptionVerification);
    }
    let mut statement = connection.prepare("SELECT ordinal,remote,branch,lease_generation,payload,payload_sha256 FROM gazette_release_target_intents WHERE authorization_id=?1 ORDER BY ordinal LIMIT ?2").map_err(native_error)?;
    let mut rows = statement
        .query(params![
            id,
            i64::try_from(record.targets.len().saturating_add(1))
                .map_err(|_| Error::StorageLimit)?
        ])
        .map_err(native_error)?;
    for intent in &record.targets {
        let row = rows
            .next()
            .map_err(native_error)?
            .ok_or(Error::ReleaseConsumptionVerification)?;
        let ordinal: i64 = row.get(0).map_err(native_error)?;
        let remote: String = row.get(1).map_err(native_error)?;
        let branch: String = row.get(2).map_err(native_error)?;
        let generation: i64 = row.get(3).map_err(native_error)?;
        let bytes = row
            .get_ref(4)
            .map_err(native_error)?
            .as_blob()
            .map_err(|_| Error::ReleaseConsumptionVerification)?;
        let hash = row
            .get_ref(5)
            .map_err(native_error)?
            .as_blob()
            .map_err(|_| Error::ReleaseConsumptionVerification)?;
        if bytes.len() > maximum(options)? {
            return Err(Error::StorageLimit);
        }
        if u32::try_from(ordinal).ok() != Some(intent.target.ordinal)
            || remote != intent.target.remote
            || branch != intent.target.branch
            || u64::try_from(generation).ok() != Some(intent.lease_generation)
            || digest(TARGET_DOMAIN, bytes).as_bytes() != hash
            || encode(
                &TargetRecord {
                    authorization_id: id,
                    intent,
                },
                options,
            )? != bytes
        {
            return Err(Error::ReleaseConsumptionVerification);
        }
    }
    if rows.next().map_err(native_error)?.is_some() {
        return Err(Error::ReleaseConsumptionVerification);
    }
    Ok(Some(record))
}

impl ReleaseConsumptionReader for Sqlite {
    fn inspect_release_leases(
        &self,
        path: &Path,
        id: &str,
        options: ReadOptions,
    ) -> Result<ReleaseLeaseInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let record = read_leases(connection, id, inspection.journal.header, options)?;
            Ok(ReleaseLeaseInspection {
                journal: inspection.journal,
                record,
            })
        })
    }

    fn inspect_release_consumption(
        &self,
        path: &Path,
        id: &str,
        options: ReadOptions,
    ) -> Result<ReleaseConsumptionInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let record = read(connection, id, inspection.journal.header, options)?;
            Ok(ReleaseConsumptionInspection {
                journal: inspection.journal,
                record,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/consumption.rs"]
mod tests;
