//! Immutable, exact checker evidence. Policy meaning remains outside the store.

use super::{digest, header, historical, native_error, validate_retention};
use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, JournalHeader, PreparationCheck, PreparationCheckInput, PreparationIntent,
        PreparationRetention, PreparationRetentionInput, ReadOptions,
    },
};
use rusqlite::{Connection, params};

pub(crate) fn validate(
    input: PreparationCheckInput<'_>,
    options: ReadOptions,
) -> Result<(), Error> {
    validate_retention(
        PreparationRetentionInput {
            intent: input.intent,
            output: input.retention.output,
            entries: input.retention.entries,
            bytes: input.retention.bytes,
        },
        options,
    )?;
    if input.retention.intent_id != input.intent.id {
        return Err(Error::PreparationVerification);
    }
    evidence_bound(input.evidence.len(), options)?;
    if Sha256Digest::of(input.evidence.as_bytes()) != input.evidence_sha256 {
        return Err(Error::PreparationVerification);
    }
    gazette_core::json::parse(input.evidence.as_bytes())
        .map_err(|_| Error::PreparationVerification)?;
    Ok(())
}

fn evidence_bound(bytes: usize, options: ReadOptions) -> Result<(), Error> {
    let size = bytes.checked_add(256).ok_or(Error::StorageLimit)?;
    if size > usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?
        || u64::try_from(size).map_err(|_| Error::StorageLimit)? > options.storage_bytes
    {
        return Err(Error::StorageLimit);
    }
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    intent: &PreparationIntent,
    retention: Option<&PreparationRetention>,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<PreparationCheck>, Error> {
    let mut statement = connection.prepare("SELECT intent_id,evidence,evidence_sha256,recorded_schema,recorded_journal,recorded_epoch,recorded_fence FROM gazette_preparation_checks WHERE intent_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement
        .query([intent.id.as_bytes()])
        .map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let retention = retention.ok_or(Error::PreparationVerification)?;
    // Check the native byte extent before allocating the retained evidence string.
    let value = row.get_ref(1).map_err(native_error)?;
    let bytes = value.as_str().map_err(|_| Error::PreparationVerification)?;
    evidence_bound(bytes.len(), options)?;
    let check = PreparationCheck {
        intent_id: digest(row, 0)?,
        evidence: bytes.to_owned(),
        evidence_sha256: digest(row, 2)?,
        recorded: header(row, 3)?,
    };
    validate(
        PreparationCheckInput {
            intent,
            retention,
            evidence: &check.evidence,
            evidence_sha256: check.evidence_sha256,
        },
        options,
    )?;
    historical(check.recorded, current)?;
    if check.intent_id != intent.id
        || check.recorded.writer_fence < retention.recorded.writer_fence
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::PreparationVerification);
    }
    Ok(Some(check))
}

pub(crate) fn insert(
    connection: &Connection,
    input: PreparationCheckInput<'_>,
    recorded: JournalHeader,
) -> Result<(), Error> {
    connection.execute("INSERT INTO gazette_preparation_checks(intent_id,evidence,evidence_sha256,recorded_schema,recorded_journal,recorded_epoch,recorded_fence) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![input.intent.id.as_bytes(), input.evidence, input.evidence_sha256.as_bytes(), recorded.schema_version, recorded.journal_id.as_slice(), i64::try_from(recorded.journal_epoch).map_err(|_| Error::PreparationVerification)?, i64::try_from(recorded.writer_fence).map_err(|_| Error::PreparationVerification)?]).map_err(native_error)?;
    Ok(())
}
