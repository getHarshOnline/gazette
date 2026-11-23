//! Durable Git intent and verified result, sharing the preparation append lifecycle.

use super::{header, historical, native_error, output, unsigned, validate_retention};
use gazette_core::{
    release::{CommitMetadata, CommitReport, ParentSource, validate_worktree_parent},
    source::{CommitId, Sha256Digest},
    store::{
        Error, JournalHeader, PreparationCheck, PreparationCheckInput, PreparationCommit,
        PreparationCommitInput, PreparationIntent, PreparationRetention, PreparationRetentionInput,
        PreparationWorktree, PreparationWorktreeInput, ReadOptions,
    },
};
use rusqlite::{Connection, params};

fn parent_encoding(
    source: Option<&ParentSource>,
    options: ReadOptions,
) -> Result<Option<String>, Error> {
    let Some(source) = source else {
        return Ok(None);
    };
    let size = source.path().as_os_str().len();
    let bound = usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?;
    if size > bound || size as u64 > options.storage_bytes {
        return Err(Error::StorageLimit);
    }
    let encoded = serde_json::to_string(source).map_err(|_| Error::PreparationVerification)?;
    if encoded.len() > bound || encoded.len() as u64 > options.storage_bytes {
        return Err(Error::StorageLimit);
    }
    Ok(Some(encoded))
}

fn metadata_encoding(metadata: &CommitMetadata, options: ReadOptions) -> Result<String, Error> {
    metadata
        .validate()
        .map_err(|_| Error::PreparationVerification)?;
    if metadata
        .parent
        .as_ref()
        .is_some_and(|id| id.as_str().bytes().all(|byte| byte == b'0'))
    {
        return Err(Error::PreparationVerification);
    }
    let size = [
        &metadata.message,
        &metadata.author.name,
        &metadata.author.email,
        &metadata.committer.name,
        &metadata.committer.email,
    ]
    .into_iter()
    .try_fold(512usize, |sum, value| {
        sum.checked_add(value.len()).ok_or(Error::StorageLimit)
    })?;
    let bound = usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?;
    if size > bound || size as u64 > options.storage_bytes {
        return Err(Error::StorageLimit);
    }
    let encoded = serde_json::to_string(metadata).map_err(|_| Error::PreparationVerification)?;
    if encoded.len() > bound || encoded.len() as u64 > options.storage_bytes {
        return Err(Error::StorageLimit);
    }
    Ok(encoded)
}

pub(crate) fn validate_worktree(
    input: PreparationWorktreeInput<'_>,
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
    if let Some(check) = input.check {
        if check.intent_id != input.intent.id {
            return Err(Error::PreparationVerification);
        }
        super::check::validate(
            PreparationCheckInput {
                intent: input.intent,
                retention: input.retention,
                evidence: &check.evidence,
                evidence_sha256: check.evidence_sha256,
            },
            options,
        )?;
    }
    metadata_encoding(input.metadata, options)?;
    parent_encoding(input.parent_source, options)?;
    validate_worktree_parent(input.metadata.parent.as_ref(), input.parent_source)
        .map_err(|_| Error::PreparationVerification)?;
    Ok(())
}

pub(crate) fn validate_commit(
    input: PreparationCommitInput<'_>,
    options: ReadOptions,
) -> Result<(), Error> {
    if input.worktree.check_sha256 != input.check.map(|check| check.evidence_sha256) {
        return Err(Error::PreparationVerification);
    }
    validate_worktree(
        PreparationWorktreeInput {
            intent: input.intent,
            retention: input.retention,
            check: input.check,
            metadata: &input.worktree.metadata,
            parent_source: input.worktree.parent_source.as_ref(),
        },
        options,
    )?;
    let report = input.report;
    if report.output != input.intent.output
        || report.bytes != input.intent.bytes
        || u64::try_from(report.entries).map_err(|_| Error::StorageLimit)? != input.intent.entries
        || report.parent != input.worktree.metadata.parent
        || report.commit.as_str().bytes().all(|byte| byte == b'0')
    {
        return Err(Error::PreparationVerification);
    }
    i64::try_from(report.entries).map_err(|_| Error::StorageLimit)?;
    i64::try_from(report.bytes).map_err(|_| Error::StorageLimit)?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    intent: &PreparationIntent,
    retention: Option<&PreparationRetention>,
    check: Option<&PreparationCheck>,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<PreparationWorktree>, Error> {
    let mut statement = connection.prepare("SELECT metadata,parent_source,created_schema,created_journal,created_epoch,created_fence,check_sha256 FROM gazette_preparation_worktrees WHERE intent_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement
        .query([intent.id.as_bytes()])
        .map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let retention = retention.ok_or(Error::PreparationVerification)?;
    let encoded: String = row.get(0).map_err(|_| Error::PreparationVerification)?;
    let metadata: CommitMetadata = serde_json::from_value(
        gazette_core::json::parse(encoded.as_bytes())
            .map_err(|_| Error::PreparationVerification)?,
    )
    .map_err(|_| Error::PreparationVerification)?;
    if metadata_encoding(&metadata, options)? != encoded {
        return Err(Error::PreparationVerification);
    }
    let encoded_parent: Option<String> = row.get(1).map_err(|_| Error::PreparationVerification)?;
    let parent_source = encoded_parent
        .as_deref()
        .map(|encoded| {
            if encoded.len()
                > usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?
                || encoded.len() as u64 > options.storage_bytes
            {
                return Err(Error::StorageLimit);
            }
            serde_json::from_value::<ParentSource>(
                gazette_core::json::parse(encoded.as_bytes())
                    .map_err(|_| Error::PreparationVerification)?,
            )
            .map_err(|_| Error::PreparationVerification)
        })
        .transpose()?;
    if parent_encoding(parent_source.as_ref(), options)? != encoded_parent {
        return Err(Error::PreparationVerification);
    }
    let mut worktree = PreparationWorktree {
        metadata,
        parent_source,
        check_sha256: row
            .get::<_, Option<[u8; 32]>>(6)
            .map_err(|_| Error::PreparationVerification)?
            .map(Sha256Digest::from),
        created: header(row, 2)?,
        completion: None,
    };
    if worktree.check_sha256 != check.map(|check| check.evidence_sha256) {
        return Err(Error::PreparationVerification);
    }
    validate_worktree(
        PreparationWorktreeInput {
            intent,
            retention,
            check,
            metadata: &worktree.metadata,
            parent_source: worktree.parent_source.as_ref(),
        },
        options,
    )?;
    historical(worktree.created, current)?;
    if worktree.created.writer_fence < retention.recorded.writer_fence
        || check.is_some_and(|check| worktree.created.writer_fence < check.recorded.writer_fence)
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::PreparationVerification);
    }
    let mut statement = connection.prepare("SELECT commit_oid,output_sha256,parent_oid,entries,bytes,recorded_schema,recorded_journal,recorded_epoch,recorded_fence FROM gazette_preparation_commits WHERE intent_id=?1 LIMIT 2").map_err(native_error)?;
    let mut rows = statement
        .query([intent.id.as_bytes()])
        .map_err(native_error)?;
    if let Some(row) = rows.next().map_err(native_error)? {
        let commit = PreparationCommit {
            report: CommitReport {
                commit: row
                    .get::<_, String>(0)
                    .map_err(|_| Error::PreparationVerification)?
                    .parse()
                    .map_err(|_| Error::PreparationVerification)?,
                output: output(row, 1)?,
                parent: row
                    .get::<_, Option<String>>(2)
                    .map_err(|_| Error::PreparationVerification)?
                    .map(|value| {
                        value
                            .parse::<CommitId>()
                            .map_err(|_| Error::PreparationVerification)
                    })
                    .transpose()?,
                entries: usize::try_from(unsigned(row, 3)?)
                    .map_err(|_| Error::PreparationVerification)?,
                bytes: unsigned(row, 4)?,
            },
            recorded: header(row, 5)?,
        };
        validate_commit(
            PreparationCommitInput {
                intent,
                retention,
                check,
                worktree: &worktree,
                report: &commit.report,
            },
            options,
        )?;
        historical(commit.recorded, current)?;
        if commit.recorded.writer_fence < worktree.created.writer_fence {
            return Err(Error::PreparationVerification);
        }
        worktree.completion = Some(commit);
    }
    if rows.next().map_err(native_error)?.is_some() {
        return Err(Error::PreparationVerification);
    }
    Ok(Some(worktree))
}

pub(crate) fn insert_worktree(
    connection: &Connection,
    input: PreparationWorktreeInput<'_>,
    created: JournalHeader,
) -> Result<(), Error> {
    let encoded =
        serde_json::to_string(input.metadata).map_err(|_| Error::PreparationVerification)?;
    let parent_source = input
        .parent_source
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| Error::PreparationVerification)?;
    connection.execute("INSERT INTO gazette_preparation_worktrees(intent_id,metadata,parent_source,created_schema,created_journal,created_epoch,created_fence,check_sha256) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![input.intent.id.as_bytes(), encoded, parent_source, created.schema_version, created.journal_id.as_slice(), i64::try_from(created.journal_epoch).map_err(|_| Error::PreparationVerification)?, i64::try_from(created.writer_fence).map_err(|_| Error::PreparationVerification)?, input.check.map(|check| check.evidence_sha256.as_bytes().as_slice())]).map_err(native_error)?;
    Ok(())
}

pub(crate) fn insert_commit(
    connection: &Connection,
    input: PreparationCommitInput<'_>,
    recorded: JournalHeader,
) -> Result<(), Error> {
    let report = input.report;
    connection.execute("INSERT INTO gazette_preparation_commits(intent_id,commit_oid,output_sha256,parent_oid,entries,bytes,recorded_schema,recorded_journal,recorded_epoch,recorded_fence) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![input.intent.id.as_bytes(), report.commit.as_str(), report.output.sha256().as_bytes(), report.parent.as_ref().map(CommitId::as_str), i64::try_from(report.entries).map_err(|_| Error::StorageLimit)?, i64::try_from(report.bytes).map_err(|_| Error::StorageLimit)?, recorded.schema_version, recorded.journal_id.as_slice(), i64::try_from(recorded.journal_epoch).map_err(|_| Error::PreparationVerification)?, i64::try_from(recorded.writer_fence).map_err(|_| Error::PreparationVerification)?]).map_err(native_error)?;
    Ok(())
}
