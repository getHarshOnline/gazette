//! Immutable local commit observations; Git validation remains application-owned.

use std::path::{Component, Path};

use gazette_core::{
    release::{CommitMetadata, CommitReport, CommitSignature},
    source::{CommitId, Sha256Digest, TreeIdentity},
    store::{
        CheckpointInput, CheckpointInspection, CommitCheckpoint, Error, JournalHeader, ReadOptions,
    },
};
use gazette_ports::store::CheckpointReader;
use rusqlite::{Connection, Row, params};

use crate::{Sqlite, native_error, registry, with_source};

const SELECT: &str = "SELECT id,repository,commit_oid,output_sha256,parent_oid,entries,bytes,author_name,author_email,author_seconds,author_offset,committer_name,committer_email,committer_seconds,committer_offset,message,created_schema,created_journal,created_epoch,created_fence FROM gazette_local_commit_checkpoints WHERE id=?1 LIMIT 2";

/// Check storage representation only, not whether the asserted Git objects exist.
pub(super) fn validate(input: CheckpointInput<'_>, options: ReadOptions) -> Result<(), Error> {
    options.validate()?;
    input
        .metadata
        .validate()
        .map_err(|_| Error::CheckpointVerification)?;
    let path = Path::new(input.repository);
    if !path.is_absolute()
        || path.parent().is_none()
        || path
            .components()
            .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        || input.repository.chars().any(char::is_control)
        || input.metadata.parent != input.commit.parent
        || input
            .commit
            .commit
            .as_str()
            .bytes()
            .all(|byte| byte == b'0')
        || input
            .metadata
            .parent
            .as_ref()
            .is_some_and(|id| id.as_str().bytes().all(|byte| byte == b'0'))
    {
        return Err(Error::CheckpointVerification);
    }
    i64::try_from(input.commit.entries).map_err(|_| Error::StorageLimit)?;
    i64::try_from(input.commit.bytes).map_err(|_| Error::StorageLimit)?;
    let mut bytes = 256usize;
    for text in [input.repository, input.metadata.message.as_str()] {
        bytes = bytes.checked_add(text.len()).ok_or(Error::StorageLimit)?;
    }
    for signature in [&input.metadata.author, &input.metadata.committer] {
        for text in [&signature.name, &signature.email] {
            bytes = bytes.checked_add(text.len()).ok_or(Error::StorageLimit)?;
        }
    }
    if bytes > usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?
        || u64::try_from(bytes).map_err(|_| Error::StorageLimit)? > options.storage_bytes
    {
        return Err(Error::StorageLimit);
    }
    Ok(())
}

pub(super) fn same_payload(record: &CommitCheckpoint, input: CheckpointInput<'_>) -> bool {
    record.id == input.id
        && record.repository == input.repository
        && record.metadata == *input.metadata
        && record.commit == *input.commit
}

pub(super) fn insert(
    connection: &Connection,
    input: CheckpointInput<'_>,
    created: JournalHeader,
) -> Result<(), Error> {
    let author = &input.metadata.author;
    let committer = &input.metadata.committer;
    connection.execute(
        "INSERT INTO gazette_local_commit_checkpoints(id,repository,commit_oid,output_sha256,parent_oid,entries,bytes,author_name,author_email,author_seconds,author_offset,committer_name,committer_email,committer_seconds,committer_offset,message,created_schema,created_journal,created_epoch,created_fence) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)",
        params![input.id.as_bytes(), input.repository, input.commit.commit.as_str(), input.commit.output.sha256().as_bytes(), input.metadata.parent.as_ref().map(CommitId::as_str), i64::try_from(input.commit.entries).map_err(|_| Error::StorageLimit)?, i64::try_from(input.commit.bytes).map_err(|_| Error::StorageLimit)?, author.name,author.email,author.seconds,author.offset_minutes,committer.name,committer.email,committer.seconds,committer.offset_minutes,input.metadata.message,created.schema_version,created.journal_id.as_slice(),i64::try_from(created.journal_epoch).map_err(|_| Error::JournalMismatch)?,i64::try_from(created.writer_fence).map_err(|_| Error::JournalMismatch)?],
    ).map_err(native_error)?;
    Ok(())
}

pub(super) fn read(
    connection: &Connection,
    id: Sha256Digest,
    current: JournalHeader,
    options: ReadOptions,
) -> Result<Option<CommitCheckpoint>, Error> {
    let mut statement = connection.prepare(SELECT).map_err(native_error)?;
    let mut rows = statement.query([id.as_bytes()]).map_err(native_error)?;
    let Some(row) = rows.next().map_err(native_error)? else {
        return Ok(None);
    };
    let record = decode(row).map_err(|_| Error::CheckpointVerification)?;
    validate(
        CheckpointInput {
            id: record.id,
            repository: &record.repository,
            metadata: &record.metadata,
            commit: &record.commit,
        },
        options,
    )?;
    if record.id != id
        || record.created.schema_version != current.schema_version
        || record.created.journal_id != current.journal_id
        || record.created.journal_epoch != current.journal_epoch
        || record.created.writer_fence == 0
        || record.created.writer_fence > current.writer_fence
        || rows.next().map_err(native_error)?.is_some()
    {
        return Err(Error::CheckpointVerification);
    }
    Ok(Some(record))
}

fn decode(row: &Row<'_>) -> Result<CommitCheckpoint, ()> {
    let get = |column| row.get::<_, String>(column).map_err(|_| ());
    let integer = |column| row.get::<_, i64>(column).map_err(|_| ());
    let digest = |column| {
        row.get::<_, [u8; 32]>(column)
            .map(Sha256Digest::from)
            .map_err(|_| ())
    };
    let signature = |start| -> Result<CommitSignature, ()> {
        Ok(CommitSignature {
            name: get(start)?,
            email: get(start + 1)?,
            seconds: integer(start + 2)?,
            offset_minutes: i32::try_from(integer(start + 3)?).map_err(|_| ())?,
        })
    };
    let parent = row
        .get::<_, Option<String>>(4)
        .map_err(|_| ())?
        .map(|value| value.parse::<CommitId>().map_err(|_| ()))
        .transpose()?;
    let output = serde_json::from_value(
        serde_json::json!({"format":TreeIdentity::FORMAT,"sha256":digest(3)?}),
    )
    .map_err(|_| ())?;
    Ok(CommitCheckpoint {
        id: digest(0)?,
        repository: get(1)?,
        metadata: CommitMetadata {
            parent: parent.clone(),
            author: signature(7)?,
            committer: signature(11)?,
            message: get(15)?,
        },
        commit: CommitReport {
            commit: get(2)?.parse().map_err(|_| ())?,
            output,
            parent,
            entries: usize::try_from(integer(5)?).map_err(|_| ())?,
            bytes: u64::try_from(integer(6)?).map_err(|_| ())?,
        },
        created: JournalHeader {
            schema_version: u32::try_from(integer(16)?).map_err(|_| ())?,
            journal_id: row.get(17).map_err(|_| ())?,
            journal_epoch: u64::try_from(integer(18)?).map_err(|_| ())?,
            writer_fence: u64::try_from(integer(19)?).map_err(|_| ())?,
        },
    })
}

impl CheckpointReader for Sqlite {
    fn inspect_checkpoint(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<CheckpointInspection, Error> {
        with_source(path, options, |connection| {
            let inspection = registry::inspect(connection, options)?;
            let checkpoint = read(connection, id, inspection.journal.header, options)?;
            Ok(CheckpointInspection {
                journal: inspection.journal,
                checkpoint,
            })
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/checkpoint.rs"]
mod tests;
