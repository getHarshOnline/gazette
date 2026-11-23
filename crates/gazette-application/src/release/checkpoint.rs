//! Durable local evidence after native verification, never release authorization.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use gazette_core::{
    destination,
    release::{CommitError, CommitMetadata},
    source::{CommitId, RevisionLimits, Sha256Digest, TreeIdentity},
    store::{
        self, CheckpointInput, CheckpointInspection, CheckpointReceipt, CheckpointWriteOptions,
        JournalHeader, ReadOptions, WriterOptions,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    release::ReleaseCommits,
    store::{CheckpointReader, CheckpointWriter, JournalWriter, WriterConnector},
};

/// Exact local commit expectations; no report or inventory totals are accepted from a caller.
pub struct CheckpointRequest<'a> {
    /// Explicit opaque record coordinate; changed intent requires a different coordinate.
    pub id: Sha256Digest,
    /// Independent local repository, never inferred from the source or working directory.
    pub repository: &'a Path,
    /// Exact existing commit to verify; checkpointing does not create it.
    pub commit: &'a CommitId,
    /// Complete expected retained output identity.
    pub output: TreeIdentity,
    /// Exact Git recipe to compare against native commit bytes.
    pub metadata: &'a CommitMetadata,
    /// All configured source/output roots that must remain separate from the journal.
    pub protected_roots: &'a [PathBuf],
}

/// Separate bounded read, generation and immutable-record permissions.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct CheckpointOptions {
    /// Native Git object/tree read ceilings; never object-write permission.
    pub objects: RevisionLimits,
    /// Explicit checkpoint record permission.
    pub record: CheckpointWriteOptions,
    /// Existing local writer generation/reservation and SQL ceilings.
    pub writer: WriterOptions,
}

/// Failure retains the boundary between native verification, generation and record effects.
#[derive(Debug)]
pub enum CheckpointError {
    /// Refusal before writer acquisition.
    Policy(store::Error),
    /// Native root separation failed without writing Git objects.
    Separation {
        /// Exact separation refusal, not a silently substituted root.
        cause: destination::Error,
        /// True means acquisition already advanced the local journal generation.
        writer_acquired: bool,
    },
    /// Existing native commit could not be verified; no Git write was attempted.
    Git {
        /// Original bounded native verification error.
        cause: CommitError,
        /// True means acquisition already advanced the local journal generation.
        writer_acquired: bool,
    },
    /// Acquisition keeps its own attempted-generation evidence.
    Acquire(store::WriterAcquireError),
    /// Record failure follows successful writer acquisition.
    Write(store::CheckpointWriteError),
    /// Record read-back succeeded, but the lifetime could not be closed successfully.
    Close {
        /// Preserve verified record evidence even when final delivery fails.
        receipt: Box<CheckpointReceipt>,
        /// Native final close/read-back failure.
        cause: store::Error,
    },
}

impl CheckpointError {
    /// Stable failure class without disclosing recipe or SQL contents.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Policy(cause) => cause.code(),
            Self::Separation { .. } => "checkpoint_source_separation",
            Self::Git { .. } => "checkpoint_commit_verification",
            Self::Acquire(error) => error.cause.code(),
            Self::Write(error) => error.cause.code(),
            Self::Close { cause, .. } => cause.code(),
        }
    }
}
impl fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(cause) => write!(f, "{cause}; no writer acquired"),
            Self::Separation {
                cause,
                writer_acquired,
            } => write!(f, "{cause}; writer acquired: {writer_acquired}"),
            Self::Git {
                cause,
                writer_acquired,
            } => write!(f, "{cause}; writer acquired: {writer_acquired}"),
            Self::Acquire(cause) => cause.fmt(f),
            Self::Write(cause) => write!(f, "{cause}; writer generation was already advanced"),
            Self::Close { receipt, cause } => write!(
                f,
                "{cause}; checkpoint {} was verified at generation {}, but final close failed",
                receipt.checkpoint.id, receipt.journal.writer_fence
            ),
        }
    }
}
impl std::error::Error for CheckpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Policy(cause) => Some(cause),
            Self::Separation { cause, .. } => Some(cause),
            Self::Git { cause, .. } => Some(cause),
            Self::Acquire(cause) => Some(cause),
            Self::Write(cause) => Some(cause),
            Self::Close { cause, .. } => Some(cause),
        }
    }
}

/// Verify existing commit objects, acquire a local writer and persist exact evidence.
/// Replays retain original creation coordinates. This creates no Git objects, refs,
/// worktree, certified artifact, authorization consumption or external effect.
///
/// # Errors
/// Refuses permissions, unsafe roots, invalid recipes/pins, stale generation, changed
/// checkpoint intent or native failures; preserve any separately advanced generation.
pub fn checkpoint_commit<C: WriterConnector>(
    connector: &C,
    separation: &impl RootSeparation,
    git: &impl ReleaseCommits,
    root: &Path,
    expected: JournalHeader,
    request: CheckpointRequest<'_>,
    options: CheckpointOptions,
) -> Result<CheckpointReceipt, CheckpointError>
where
    C::Writer: CheckpointWriter,
{
    options.record.validate().map_err(CheckpointError::Policy)?;
    options.writer.validate().map_err(CheckpointError::Policy)?;
    let repository = request
        .repository
        .to_str()
        .filter(|value| !value.is_empty() && request.repository.is_absolute())
        .ok_or(CheckpointError::Policy(
            store::Error::CheckpointVerification,
        ))?;
    // Bound retained recipe/locator text before any generation effect. Git checks
    // the shared recipe and native object budgets; SQLite owns row bounds.
    let text = [
        repository,
        &request.metadata.message,
        &request.metadata.author.name,
        &request.metadata.author.email,
        &request.metadata.committer.name,
        &request.metadata.committer.email,
    ];
    let bytes = text
        .iter()
        .try_fold(0usize, |sum, value| sum.checked_add(value.len()));
    if bytes.is_none_or(|bytes| bytes > options.writer.session.read.value_bytes as usize) {
        return Err(CheckpointError::Policy(
            store::Error::CheckpointVerification,
        ));
    }
    let mut protected = request.protected_roots.to_vec();
    protected.push(request.repository.to_owned());
    separation
        .check_root_separation(root, &protected)
        .map_err(|cause| CheckpointError::Separation {
            cause,
            writer_acquired: false,
        })?;
    let verify = || {
        git.verify_commit(
            request.repository,
            request.commit,
            request.output,
            request.metadata,
            options.objects,
        )
    };
    let first = verify().map_err(|cause| CheckpointError::Git {
        cause,
        writer_acquired: false,
    })?;
    let mut writer = connector
        .acquire_writer(root, expected, options.writer)
        .map_err(CheckpointError::Acquire)?;
    separation
        .check_root_separation(root, &protected)
        .map_err(|cause| CheckpointError::Separation {
            cause,
            writer_acquired: true,
        })?;
    let commit = verify().map_err(|cause| CheckpointError::Git {
        cause,
        writer_acquired: true,
    })?;
    if commit != first {
        return Err(CheckpointError::Git {
            cause: CommitError::refused("local commit evidence changed during writer acquisition"),
            writer_acquired: true,
        });
    }
    let receipt = writer
        .record_checkpoint(
            CheckpointInput {
                id: request.id,
                repository,
                metadata: request.metadata,
                commit: &commit,
            },
            options.record,
        )
        .map_err(CheckpointError::Write)?;
    if let Err(cause) = writer.close() {
        return Err(CheckpointError::Close {
            receipt: Box::new(receipt),
            cause,
        });
    }
    Ok(receipt)
}

/// Inspect exact retained local evidence without checking current object availability.
/// Use native commit verification separately when testing its present usability.
///
/// # Errors
/// Preserves schema/row/native failures and refuses an independently expected header mismatch.
pub fn inspect_checkpoint(
    reader: &impl CheckpointReader,
    path: &Path,
    id: Sha256Digest,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<CheckpointInspection, store::Error> {
    options.validate()?;
    let inspection = reader.inspect_checkpoint(path, id, options)?;
    if expected.is_some_and(|header| *header != inspection.journal.header) {
        return Err(store::Error::JournalMismatch);
    }
    Ok(inspection)
}
