//! Exact private staging history; no source, stage or release effect belongs here.

use std::path::{Component, Path, PathBuf};

use gazette_core::{
    release::PublicationDestination,
    render::LimitedWriter,
    source::{Sha256Digest, Subdirectory, TreeIdentity},
    stage::Address,
    store::{
        Error, JournalHeader, PreparationInput, PreparationInspection, PreparationIntent,
        PreparationRecord, PreparationRetention, PreparationRetentionInput, ReadOptions,
    },
};
use gazette_ports::store::PreparationReader;
use rusqlite::{Connection, Row, params, types::ValueRef};

use crate::{Sqlite, native_error, registry, with_source};

pub(super) mod check;
pub(super) mod worktree;

pub(super) fn validate(input: PreparationInput<'_>, options: ReadOptions) -> Result<(), Error> {
    options.validate()?;
    let parent = input
        .address
        .parent
        .to_str()
        .ok_or(Error::PreparationVerification)?;
    let remaining = payload_budget(
        parent.len(),
        input.address.name.len(),
        input.evidence.len(),
        options,
    )?;
    encode_destination(input.destination, remaining)?;
    let normalized: PathBuf = input.address.parent.components().collect();
    let name = &input.address.name;
    let selected: Subdirectory = name.parse().map_err(|_| Error::PreparationVerification)?;
    if !input.address.parent.is_absolute()
        || normalized.to_str() != Some(parent)
        || input
            .address
            .parent
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        || parent.chars().any(char::is_control)
        || name.chars().any(char::is_control)
        || selected.components().count() != 1
    {
        return Err(Error::PreparationVerification);
    }
    i64::try_from(input.entries).map_err(|_| Error::StorageLimit)?;
    i64::try_from(input.bytes).map_err(|_| Error::StorageLimit)?;
    if Sha256Digest::of(input.evidence.as_bytes()) != input.evidence_sha256 {
        return Err(Error::PreparationVerification);
    }
    gazette_core::json::parse(input.evidence.as_bytes())
        .map_err(|_| Error::PreparationVerification)?;
    Ok(())
}

fn payload_budget(
    parent: usize,
    name: usize,
    evidence: usize,
    options: ReadOptions,
) -> Result<usize, Error> {
    let size = [parent, name, evidence]
        .into_iter()
        .try_fold(256usize, |sum, bytes| {
            sum.checked_add(bytes).ok_or(Error::StorageLimit)
        })?;
    if size > usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?
        || u64::try_from(size).map_err(|_| Error::StorageLimit)? > options.storage_bytes
    {
        return Err(Error::StorageLimit);
    }
    Ok(usize::try_from(options.value_bytes)
        .map_err(|_| Error::InvalidLimits)?
        .min(usize::try_from(options.storage_bytes).unwrap_or(usize::MAX))
        - size)
}

fn encode_destination(
    destination: Option<&PublicationDestination>,
    maximum: usize,
) -> Result<Option<String>, Error> {
    destination
        .map(|destination| {
            destination
                .validate()
                .map_err(|_| Error::PreparationVerification)?;
            let mut output = LimitedWriter::new(Vec::new(), maximum);
            let encoded = serde_json::to_writer(&mut output, destination);
            if output.exceeded() {
                return Err(Error::StorageLimit);
            }
            encoded.map_err(|_| Error::PreparationVerification)?;
            String::from_utf8(output.into_inner()).map_err(|_| Error::PreparationVerification)
        })
        .transpose()
}

pub(super) fn validate_retention(
    input: PreparationRetentionInput<'_>,
    options: ReadOptions,
) -> Result<(), Error> {
    validate(input.intent.as_input(), options)?;
    if input.output != input.intent.output
        || input.entries != input.intent.entries
        || input.bytes != input.intent.bytes
    {
        return Err(Error::PreparationVerification);
    }
    Ok(())
}

fn historical(recorded: JournalHeader, current: JournalHeader) -> Result<(), Error> {
    if recorded.schema_version != current.schema_version
        || recorded.journal_id != current.journal_id
        || recorded.journal_epoch != current.journal_epoch
        || recorded.writer_fence == 0
        || recorded.writer_fence > current.writer_fence
    {
        return Err(Error::PreparationVerification);
    }
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: Sha256Digest,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<PreparationRecord>, Error> {
    let mut statement = connection.prepare("SELECT id,parent,name,output_sha256,entries,bytes,evidence,evidence_sha256,created_schema,created_journal,created_epoch,created_fence,destination FROM gazette_preparation_intents WHERE id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let intent = decode_intent(row, options)?;
    validate(intent.as_input(), options)?;
    historical(intent.created, current)?;
    if intent.id != id || rows.next().map_err(native_error)?.is_some() {
        return Err(Error::PreparationVerification);
    }
    let mut statement = connection.prepare("SELECT intent_id,output_sha256,entries,bytes,recorded_schema,recorded_journal,recorded_epoch,recorded_fence FROM gazette_preparation_retentions WHERE intent_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let retention = rows
        .next()
        .map_err(native_error)?
        .map(decode_retention)
        .transpose()?;
    if let Some(retained) = &retention {
        historical(retained.recorded, current)?;
        validate_retention(
            PreparationRetentionInput {
                intent: &intent,
                output: retained.output,
                entries: retained.entries,
                bytes: retained.bytes,
            },
            options,
        )?;
        if retained.intent_id != id || retained.recorded.writer_fence < intent.created.writer_fence
        {
            return Err(Error::PreparationVerification);
        }
    }
    if rows.next().map_err(native_error)?.is_some() {
        return Err(Error::PreparationVerification);
    }
    let check = check::read(connection, &intent, retention.as_ref(), current, options)?;
    let worktree = worktree::read(
        connection,
        &intent,
        retention.as_ref(),
        check.as_ref(),
        current,
        options,
    )?;
    Ok(Some(PreparationRecord {
        intent,
        retention,
        check,
        worktree,
    }))
}

fn digest(row: &Row<'_>, column: usize) -> Result<Sha256Digest, Error> {
    row.get::<_, [u8; 32]>(column)
        .map(Sha256Digest::from)
        .map_err(|_| Error::PreparationVerification)
}

fn unsigned(row: &Row<'_>, column: usize) -> Result<u64, Error> {
    u64::try_from(
        row.get::<_, i64>(column)
            .map_err(|_| Error::PreparationVerification)?,
    )
    .map_err(|_| Error::PreparationVerification)
}

fn header(row: &Row<'_>, start: usize) -> Result<JournalHeader, Error> {
    Ok(JournalHeader {
        schema_version: u32::try_from(unsigned(row, start)?)
            .map_err(|_| Error::PreparationVerification)?,
        journal_id: row
            .get(start + 1)
            .map_err(|_| Error::PreparationVerification)?,
        journal_epoch: unsigned(row, start + 2)?,
        writer_fence: unsigned(row, start + 3)?,
    })
}

fn output(row: &Row<'_>, column: usize) -> Result<TreeIdentity, Error> {
    serde_json::from_value(
        serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":digest(row,column)?}),
    )
    .map_err(|_| Error::PreparationVerification)
}

fn decode_intent(row: &Row<'_>, options: ReadOptions) -> Result<PreparationIntent, Error> {
    let text = |column: usize| {
        row.get_ref(column)
            .map_err(|_| Error::PreparationVerification)?
            .as_str()
            .map_err(|_| Error::PreparationVerification)
    };
    let parent = text(1)?;
    let name = text(2)?;
    let evidence = text(6)?;
    // SQLite has already materialized the row; bound retained allocations and JSON
    // parsing from its borrowed native byte extents before copying any payload.
    let remaining = payload_budget(parent.len(), name.len(), evidence.len(), options)?;
    let destination = match row
        .get_ref(12)
        .map_err(|_| Error::PreparationVerification)?
    {
        ValueRef::Null => None,
        ValueRef::Text(bytes) => {
            if bytes.len() > remaining {
                return Err(Error::StorageLimit);
            }
            let value =
                gazette_core::json::parse(bytes).map_err(|_| Error::PreparationVerification)?;
            let destination: PublicationDestination =
                serde_json::from_value(value).map_err(|_| Error::PreparationVerification)?;
            if encode_destination(Some(&destination), remaining)?
                .as_deref()
                .map(str::as_bytes)
                != Some(bytes)
            {
                return Err(Error::PreparationVerification);
            }
            Some(destination)
        }
        _ => return Err(Error::PreparationVerification),
    };
    Ok(PreparationIntent {
        id: digest(row, 0)?,
        address: Address {
            parent: PathBuf::from(parent),
            name: name.to_owned(),
        },
        destination,
        output: output(row, 3)?,
        entries: unsigned(row, 4)?,
        bytes: unsigned(row, 5)?,
        evidence: evidence.to_owned(),
        evidence_sha256: digest(row, 7)?,
        created: header(row, 8)?,
    })
}

fn decode_retention(row: &Row<'_>) -> Result<PreparationRetention, Error> {
    Ok(PreparationRetention {
        intent_id: digest(row, 0)?,
        output: output(row, 1)?,
        entries: unsigned(row, 2)?,
        bytes: unsigned(row, 3)?,
        recorded: header(row, 4)?,
    })
}

pub(super) fn insert_intent(
    connection: &Connection,
    input: PreparationInput<'_>,
    created: JournalHeader,
) -> Result<(), Error> {
    // The append owner validates this exact borrowed input against its invocation
    // ceilings before opening the write transaction; no mutable recipe is reread.
    let destination = input
        .destination
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| Error::PreparationVerification)?;
    connection.execute("INSERT INTO gazette_preparation_intents(id,parent,name,output_sha256,entries,bytes,evidence,evidence_sha256,created_schema,created_journal,created_epoch,created_fence,destination) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)", params![input.id.as_bytes(), input.address.parent.to_str().ok_or(Error::PreparationVerification)?, input.address.name, input.output.sha256().as_bytes(), i64::try_from(input.entries).map_err(|_| Error::StorageLimit)?, i64::try_from(input.bytes).map_err(|_| Error::StorageLimit)?, input.evidence, input.evidence_sha256.as_bytes(), created.schema_version, created.journal_id.as_slice(), i64::try_from(created.journal_epoch).map_err(|_| Error::PreparationVerification)?, i64::try_from(created.writer_fence).map_err(|_| Error::PreparationVerification)?, destination]).map_err(native_error)?;
    Ok(())
}

pub(super) fn insert_retention(
    connection: &Connection,
    input: PreparationRetentionInput<'_>,
    recorded: JournalHeader,
) -> Result<(), Error> {
    connection.execute("INSERT INTO gazette_preparation_retentions(intent_id,output_sha256,entries,bytes,recorded_schema,recorded_journal,recorded_epoch,recorded_fence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![input.intent.id.as_bytes(), input.output.sha256().as_bytes(), i64::try_from(input.entries).map_err(|_| Error::StorageLimit)?, i64::try_from(input.bytes).map_err(|_| Error::StorageLimit)?, recorded.schema_version, recorded.journal_id.as_slice(), i64::try_from(recorded.journal_epoch).map_err(|_| Error::PreparationVerification)?, i64::try_from(recorded.writer_fence).map_err(|_| Error::PreparationVerification)?]).map_err(native_error)?;
    Ok(())
}

impl PreparationReader for Sqlite {
    fn inspect_preparation(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<PreparationInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let record = read(connection, id, inspection.journal.header, options)?;
            Ok(PreparationInspection {
                journal: inspection.journal,
                record,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/preparation.rs"]
mod tests;
