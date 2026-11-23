//! Full immutable reconciliation chains, independently derived from retained original history.

use crate::{Sqlite, attempt, consumption, native_error, with_source};
use gazette_core::{
    render::LimitedWriter,
    store::{
        Error, ReadOptions, ReleaseReconciliationFrontier, ReleaseReconciliationInspection,
        ReleaseReconciliationRecord,
    },
};
use gazette_ports::store::ReleaseReconciliationReader;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub(super) fn payload(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    if serde_json::to_writer(&mut output, value).is_err() {
        return Err(if output.exceeded() {
            Error::StorageLimit
        } else {
            Error::ReleaseSettlementVerification
        });
    }
    Ok(output.into_inner())
}

pub(super) fn insert(
    connection: &Connection,
    record: &ReleaseReconciliationRecord,
    maximum: usize,
) -> Result<(), Error> {
    let bytes = payload(record, maximum)?;
    let identity = record.identity(maximum)?;
    connection.execute(
        "INSERT INTO gazette_release_reconciliations(authorization_id,number,previous_sha256,payload,payload_sha256) VALUES (?1,?2,?3,?4,?5)",
        params![record.reconciliation.authorization_id, i64::try_from(record.reconciliation.number).map_err(|_| Error::StorageLimit)?, record.reconciliation.frontier.previous.map(|value| *value.as_bytes()), bytes, identity.as_bytes()],
    ).map_err(native_error)?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: &str,
    options: ReadOptions,
) -> Result<ReleaseReconciliationInspection, Error> {
    let maximum = consumption::maximum(options)?;
    let history = attempt::read(connection, id, options)?;
    let Some(consumed) = history.consumption.as_ref() else {
        return Ok(ReleaseReconciliationInspection {
            history,
            records: vec![],
        });
    };
    let limit = i64::try_from(options.storage_bytes / 128).map_err(|_| Error::StorageLimit)?;
    let mut statement = connection.prepare(
        "SELECT number,previous_sha256,payload,payload_sha256 FROM gazette_release_reconciliations WHERE authorization_id=?1 ORDER BY number LIMIT ?2",
    ).map_err(native_error)?;
    let mut rows = statement
        .query(params![id, limit.saturating_add(1)])
        .map_err(native_error)?;
    let mut records: Vec<ReleaseReconciliationRecord> = Vec::new();
    let mut retained = 0_u64;
    while let Some(row) = rows.next().map_err(native_error)? {
        if i64::try_from(records.len()).map_err(|_| Error::StorageLimit)? >= limit {
            return Err(Error::StorageLimit);
        }
        let number: i64 = row.get(0).map_err(native_error)?;
        let previous: Option<[u8; 32]> = row.get(1).map_err(native_error)?;
        let bytes = row
            .get_ref(2)
            .map_err(native_error)?
            .as_blob()
            .map_err(|_| Error::ReleaseSettlementVerification)?;
        let hash: [u8; 32] = row.get(3).map_err(native_error)?;
        if bytes.len() > maximum {
            return Err(Error::StorageLimit);
        }
        retained = retained
            .checked_add(u64::try_from(bytes.len()).map_err(|_| Error::StorageLimit)?)
            .ok_or(Error::StorageLimit)?;
        if retained > options.storage_bytes {
            return Err(Error::StorageLimit);
        }
        let value =
            gazette_core::json::parse(bytes).map_err(|_| Error::ReleaseSettlementVerification)?;
        let record = ReleaseReconciliationRecord::deserialize(&value)
            .map_err(|_| Error::ReleaseSettlementVerification)?;
        if payload(&record, maximum)? != bytes
            || record.identity(maximum)?.as_bytes() != &hash
            || record.reconciliation.authorization_id != id
            || u64::try_from(number).ok() != Some(record.reconciliation.number)
            || record
                .reconciliation
                .frontier
                .previous
                .map(|value| *value.as_bytes())
                != previous
            || record.reconciliation.coordinator.journal.writer_fence
                > history.journal.header.writer_fence
        {
            return Err(Error::ReleaseSettlementVerification);
        }
        let previous = records.last();
        if record
            .reconciliation
            .settle(consumed, &history.attempts, previous, maximum)?
            != record.settlement
        {
            return Err(Error::ReleaseSettlementVerification);
        }
        // A terminal record closed this exact attempt frontier. Later starts/results are corruption,
        // while nonterminal historical has_result=false deliberately stays false on later reads.
        if record.settlement.state.is_terminal()
            && record.reconciliation.frontier
                != ReleaseReconciliationFrontier::current(&history.attempts, previous, maximum)?
        {
            return Err(Error::ReleaseSettlementVerification);
        }
        records.push(record);
    }
    Ok(ReleaseReconciliationInspection { history, records })
}

pub(super) fn terminal(
    connection: &Connection,
    id: &str,
    options: ReadOptions,
) -> Result<bool, Error> {
    Ok(read(connection, id, options)?
        .records
        .last()
        .is_some_and(|record| record.settlement.state.is_terminal()))
}

/// Every consumption must be terminal, including batches with no attempt starts.
pub(super) fn require_terminal(connection: &Connection, options: ReadOptions) -> Result<(), Error> {
    let maximum = consumption::maximum(options)?;
    let limit = i64::try_from(options.storage_bytes / 128).map_err(|_| Error::StorageLimit)?;
    let mut statement = connection.prepare(
        "SELECT authorization_id FROM gazette_release_consumptions ORDER BY authorization_id LIMIT ?1",
    ).map_err(native_error)?;
    let mut rows = statement
        .query([limit.saturating_add(1)])
        .map_err(native_error)?;
    let mut count = 0_i64;
    while let Some(row) = rows.next().map_err(native_error)? {
        if count >= limit {
            return Err(Error::StorageLimit);
        }
        count += 1;
        let id = row
            .get_ref(0)
            .map_err(native_error)?
            .as_str()
            .map_err(|_| Error::ReleaseSettlementVerification)?;
        if id.len() > maximum {
            return Err(Error::StorageLimit);
        }
        if !terminal(connection, id, options)? {
            return Err(Error::ReleaseReconciliationRequired);
        }
    }
    Ok(())
}

impl ReleaseReconciliationReader for Sqlite {
    fn inspect_release_reconciliations(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<ReleaseReconciliationInspection, Error> {
        with_source(path, options, |connection| {
            read(connection, record_id, options)
        })
    }
}
