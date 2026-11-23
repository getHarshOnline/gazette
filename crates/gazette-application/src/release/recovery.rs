//! Recover exact existing local preparation, never repeat its initialization.

use super::{
    JournalWorktreeInspectionError, JournalWorktreeInspectionOptions,
    JournalWorktreeInspectionRequest, WorktreeRequest, inspect_journal_worktree, inspect_worktree,
};
use gazette_core::{
    destination,
    release::{WorktreeError, WorktreeOutputSyncOptions, WorktreeSyncError, WorktreeSyncOptions},
    source::{CommitId, Sha256Digest},
    store::{self, PreparationReceipt, PreparationRecord},
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeOutputSync, WorktreeReader, WorktreeSync},
    store::{JournalWriter, PreparationReader, PreparationWriter, WriterConnector},
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Previously observed immutable history and exact commit to recover without recreation.
#[derive(Clone, Copy, Debug)]
pub struct RecoverWorktreeRequest<'a> {
    /// Existing private journal directory, separate from source and output.
    pub journal: &'a Path,
    /// Independently retained current coordinates before writer acquisition.
    pub expected: store::JournalHeader,
    /// Recorded intent, retention and Git recipe; no caller-invented replacement recipe.
    pub preparation: &'a PreparationRecord,
    /// Exact existing commit; never discovered from HEAD or regenerated.
    pub commit: &'a CommitId,
    /// Every configured native input root; the journal is additionally protected.
    pub protected_roots: &'a [PathBuf],
}

/// Exact native-history selection for callers that do not carry a preparation record.
/// The journal supplies the recipe; no caller can replace its address or metadata.
#[derive(Clone, Copy, Debug)]
pub struct RecoverRecordedWorktreeRequest<'a> {
    /// Existing private journal database, never initialized or migrated.
    pub database: &'a Path,
    /// Exact stored preparation coordinate; no latest-record discovery.
    pub preparation_id: Sha256Digest,
    /// Independently retained coordinates, required before any writer acquisition.
    pub expected: store::JournalHeader,
    /// Exact previously observed commit, never inferred from HEAD or history.
    pub commit: &'a CommitId,
    /// Every configured native input root, protected before journal access.
    pub protected_roots: &'a [PathBuf],
}

/// Independent synchronization, local writer and history-append permissions.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecoverWorktreeOptions {
    /// Permit recording the exact verified completion; never a new recipe.
    pub record: store::PreparationWriteOptions,
    /// Exact-generation acquisition and bounded native connection effects.
    pub writer: store::WriterOptions,
    /// Explicit synchronization permission and native verification ceilings.
    pub worktree: WorktreeSyncOptions,
}

impl RecoverWorktreeOptions {
    fn validate(self) -> Result<(), WorktreeRecoveryFailure> {
        if !self.worktree.allow_synchronize {
            return Err(WorktreeRecoveryFailure::Permission);
        }
        self.record
            .validate()
            .and_then(|()| self.writer.validate())
            .map_err(WorktreeRecoveryFailure::Journal)
    }
}

/// One synchronization owner's observed progress, not a power-loss guarantee.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub enum SynchronizationState {
    /// No synchronization was attempted by this invocation.
    #[default]
    NotAttempted,
    /// A synchronization may have begun without complete returned read-back.
    Unverified,
    /// Synchronization and its bounded independent read-back returned successfully.
    Verified,
}

/// Known local recovery boundaries; no commit creation or external effect is implied.
#[derive(Clone, Debug, Default, serde::Serialize, schemars::JsonSchema)]
pub struct WorktreeRecoveryProgress {
    /// Local writer acquisition returned; failure may precede observing its coordinates.
    pub writer_acquired: bool,
    /// Latest verified acquired journal coordinates, never a reusable writer lifetime.
    pub journal: Option<store::JournalHeader>,
    /// Exact existing commit independently verified before any synchronization.
    pub verified_commit: Option<CommitId>,
    /// Complete filesystem output synchronization progress.
    pub output_sync: SynchronizationState,
    /// Independent Git administrative-storage synchronization progress.
    pub git_sync: SynchronizationState,
}

/// Failed recovery phase, independent from previously performed effects.
#[derive(Debug)]
pub enum WorktreeRecoveryFailure {
    /// Exact recorded-history/current-byte preflight failed before writer acquisition.
    Inspection(JournalWorktreeInspectionError),
    /// Explicit existing-byte synchronization permission is absent.
    Permission,
    /// A required historical phase is missing; it must not be reconstructed.
    Missing,
    /// Expected, recorded or independently observed evidence disagrees.
    Mismatch,
    /// Protected native roots overlap or cannot be verified.
    Separation(destination::Error),
    /// Local generation acquisition failed, possibly after a generation advance.
    Writer(store::WriterAcquireError),
    /// Journal inspection, permissions, bounds or close failed.
    Journal(store::Error),
    /// Immutable completion append or its read-back failed.
    Record(store::PreparationWriteError),
    /// Exact current Git/filesystem inspection failed.
    Worktree(WorktreeError),
    /// An output or Git synchronization failed without repair or cleanup.
    Synchronization(WorktreeSyncError),
}

/// Recovery preserves known progress and history when later operations fail.
#[derive(Debug)]
pub struct WorktreeRecoveryError {
    /// Redacted failed boundary, not proof of rollback.
    pub cause: Box<WorktreeRecoveryFailure>,
    /// Verified observations and potentially attempted synchronization.
    pub progress: WorktreeRecoveryProgress,
    /// Latest independently verified completion receipt, when an append/replay returned.
    pub receipt: Option<Box<PreparationReceipt>>,
}

impl WorktreeRecoveryError {
    fn before_effects(cause: WorktreeRecoveryFailure) -> Self {
        Self {
            cause: Box::new(cause),
            progress: WorktreeRecoveryProgress::default(),
            receipt: None,
        }
    }

    /// Stable redacted diagnostic class, independent from possible prior effects.
    pub fn code(&self) -> &'static str {
        match self.cause.as_ref() {
            WorktreeRecoveryFailure::Inspection(error) => error.code(),
            WorktreeRecoveryFailure::Permission => "preparation_synchronization_permission",
            WorktreeRecoveryFailure::Missing => "preparation_worktree_missing",
            WorktreeRecoveryFailure::Mismatch => "preparation_mismatch",
            WorktreeRecoveryFailure::Separation(_) => "preparation_separation",
            WorktreeRecoveryFailure::Writer(error) => error.cause.code(),
            WorktreeRecoveryFailure::Journal(error) => error.code(),
            WorktreeRecoveryFailure::Record(error) => error.cause.code(),
            WorktreeRecoveryFailure::Worktree(_) => "preparation_worktree_verification",
            WorktreeRecoveryFailure::Synchronization(_) => "preparation_synchronization",
        }
    }
}

impl fmt::Display for WorktreeRecoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.cause.as_ref() {
            WorktreeRecoveryFailure::Inspection(_) => "recorded worktree preflight failed",
            WorktreeRecoveryFailure::Permission => "synchronization permission missing",
            WorktreeRecoveryFailure::Missing => "recorded preparation phase missing",
            WorktreeRecoveryFailure::Mismatch => "exact preparation evidence mismatch",
            WorktreeRecoveryFailure::Separation(_) => "source/journal/worktree separation failed",
            WorktreeRecoveryFailure::Writer(_) => "journal writer acquisition failed",
            WorktreeRecoveryFailure::Journal(_) => "journal verification or close failed",
            WorktreeRecoveryFailure::Record(_) => "completion record or read-back failed",
            WorktreeRecoveryFailure::Worktree(_) => "current worktree verification failed",
            WorktreeRecoveryFailure::Synchronization(_) => "native synchronization failed",
        };
        write!(
            f,
            "{phase}; writer acquired {}; latest journal {:?}; verified commit {:?}; output sync {:?}; Git sync {:?}; completion receipt observed {}",
            self.progress.writer_acquired,
            self.progress.journal,
            self.progress.verified_commit,
            self.progress.output_sync,
            self.progress.git_sync,
            self.receipt.is_some()
        )?;
        if let WorktreeRecoveryFailure::Writer(error) = self.cause.as_ref() {
            write!(f, "; generation effect {:?}", error.generation)?;
        }
        if let WorktreeRecoveryFailure::Record(error) = self.cause.as_ref() {
            write!(f, "; completion record effect {:?}", error.effect)?;
        }
        f.write_str(
            "; preserve exact history and native bytes; no recreation or release is authorized",
        )
    }
}

impl std::error::Error for WorktreeRecoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.cause.as_ref() {
            WorktreeRecoveryFailure::Inspection(error) => Some(error),
            WorktreeRecoveryFailure::Separation(error) => Some(error),
            WorktreeRecoveryFailure::Writer(error) => Some(error),
            WorktreeRecoveryFailure::Journal(error) => Some(error),
            WorktreeRecoveryFailure::Record(error) => Some(error),
            WorktreeRecoveryFailure::Worktree(error) => Some(error),
            WorktreeRecoveryFailure::Synchronization(error) => Some(error),
            _ => None,
        }
    }
}

/// Recover one recorded preparation through native preflight and the same held-writer operation.
///
/// Permissions are checked before native access. The existing joined inspector
/// verifies the exact journal expectation, stored recipe and current Git/output
/// bytes before any generation advance. Recovery then rechecks that record under
/// its own local writer; preflight is not a transferable lock or freshness proof.
/// No stdin recipe, native source recapture, initialization or commit reconstruction
/// is involved. Missing completion can be appended; exact completion is replayed.
///
/// # Errors
/// Refuses denied effects, missing/stale history, native drift and exhausted bounds.
/// Later recovery errors preserve possible generation/synchronization/record effects.
pub fn recover_recorded_worktree<F, G, C>(
    filesystem: &F,
    git: &G,
    store: &C,
    request: RecoverRecordedWorktreeRequest<'_>,
    options: RecoverWorktreeOptions,
) -> Result<PreparationReceipt, WorktreeRecoveryError>
where
    F: RootSeparation + WorktreeReader + WorktreeOutputSync,
    G: ReleaseWorktrees + WorktreeSync,
    C: PreparationReader + WriterConnector,
    C::Writer: PreparationWriter,
{
    options
        .validate()
        .map_err(WorktreeRecoveryError::before_effects)?;
    let inspected = inspect_journal_worktree(
        filesystem,
        git,
        store,
        JournalWorktreeInspectionRequest {
            database: request.database,
            preparation_id: request.preparation_id,
            expected_header: Some(&request.expected),
            commit: request.commit,
            protected_roots: request.protected_roots,
        },
        JournalWorktreeInspectionOptions {
            read: options.writer.session.read,
            worktree: options.worktree.limits,
        },
    )
    .map_err(|error| {
        WorktreeRecoveryError::before_effects(WorktreeRecoveryFailure::Inspection(error))
    })?;
    let record =
        inspected.history.record.as_ref().ok_or_else(|| {
            WorktreeRecoveryError::before_effects(WorktreeRecoveryFailure::Missing)
        })?;
    let journal = request
        .database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    recover_journal_worktree(
        filesystem,
        git,
        store,
        RecoverWorktreeRequest {
            journal,
            expected: request.expected,
            preparation: record,
            commit: request.commit,
            protected_roots: request.protected_roots,
        },
        options,
    )
}

/// Synchronize an exact existing prepared worktree and record its verified completion.
///
/// A held local writer rechecks immutable history before native effects. Output
/// and Git storage are synchronized by their separate owners, then the complete
/// pair is independently inspected before the existing completion append. Exact
/// replay preserves its original completion coordinates. No source is recaptured,
/// parent source reopened, recipe inserted, commit created, metadata repaired or
/// output removed. This is not certification, anti-rollback qualification, remote
/// release or proof of hardware power-loss durability.
///
/// # Errors
/// Missing/conflicting history, stale writer, native drift or bounds fail closed.
/// Synchronization/record/close failures retain known progress; no error makes an
/// existing worktree safe to recreate or a previous external effect safe to repeat.
pub fn recover_journal_worktree<F, G, C>(
    filesystem: &F,
    git: &G,
    store: &C,
    request: RecoverWorktreeRequest<'_>,
    options: RecoverWorktreeOptions,
) -> Result<PreparationReceipt, WorktreeRecoveryError>
where
    F: RootSeparation + WorktreeReader + WorktreeOutputSync,
    G: ReleaseWorktrees + WorktreeSync,
    C: WriterConnector,
    C::Writer: PreparationWriter,
{
    use WorktreeRecoveryFailure as Failure;
    let mut progress = WorktreeRecoveryProgress::default();
    let mut receipt = None;
    let result = (|| {
        options.validate()?;
        if request.preparation.retention.is_none() || request.preparation.worktree.is_none() {
            return Err(Failure::Missing);
        }
        let root = request.preparation.intent.address.path();
        filesystem
            .check_root_separation(request.journal, request.protected_roots)
            .map_err(Failure::Separation)?;
        let mut protected = request.protected_roots.to_vec();
        protected.push(request.journal.to_owned());
        filesystem
            .check_root_separation(&root, &protected)
            .map_err(Failure::Separation)?;
        let mut writer = store
            .acquire_writer(request.journal, request.expected, options.writer)
            .map_err(Failure::Writer)?;
        progress.writer_acquired = true;
        let history = writer
            .inspect_preparation(request.preparation.intent.id)
            .map_err(Failure::Journal)?;
        progress.journal = Some(history.journal.header);
        let record = history.record.ok_or(Failure::Missing)?;
        if !same_record(request.preparation, &record) {
            return Err(Failure::Mismatch);
        }
        let recipe = record.worktree.as_ref().ok_or(Failure::Missing)?;
        let retention = record.retention.as_ref().ok_or(Failure::Missing)?;
        if recipe.check_sha256 != record.check.as_ref().map(|check| check.evidence_sha256) {
            return Err(Failure::Mismatch);
        }
        let native = WorktreeRequest {
            root: &root,
            expected: record.intent.output,
            metadata: &recipe.metadata,
            parent_source: None,
            protected_roots: &protected,
        };
        let before = inspect_worktree(
            filesystem,
            git,
            native,
            request.commit,
            options.worktree.limits,
        )
        .map_err(Failure::Worktree)?;
        if before.git.commit.entries as u64 != record.intent.entries
            || before.git.commit.bytes != record.intent.bytes
            || recipe
                .completion
                .as_ref()
                .is_some_and(|done| done.report != before.git.commit)
        {
            return Err(Failure::Mismatch);
        }
        progress.verified_commit = Some(before.git.commit.commit.clone());
        writer.inspect().map_err(Failure::Journal)?;
        let output = filesystem
            .synchronize_worktree_output(
                &root,
                &before.output,
                &protected,
                WorktreeOutputSyncOptions {
                    allow_synchronize: true,
                    limits: options.worktree.limits.output.content,
                },
            )
            .map_err(|error| {
                if error.attempted {
                    progress.output_sync = SynchronizationState::Unverified;
                }
                Failure::Synchronization(error)
            })?;
        progress.output_sync = SynchronizationState::Verified;
        if output != before.output {
            return Err(Failure::Mismatch);
        }
        writer.inspect().map_err(Failure::Journal)?;
        let synchronized = git
            .synchronize_worktree(
                &root,
                request.commit,
                record.intent.output,
                &recipe.metadata,
                options.worktree,
            )
            .map_err(|error| {
                if error.attempted {
                    progress.git_sync = SynchronizationState::Unverified;
                }
                Failure::Synchronization(error)
            })?;
        progress.git_sync = SynchronizationState::Verified;
        let after = inspect_worktree(
            filesystem,
            git,
            native,
            request.commit,
            options.worktree.limits,
        )
        .map_err(Failure::Worktree)?;
        if synchronized != before.git || after.git != before.git || after.output != before.output {
            return Err(Failure::Mismatch);
        }
        writer.inspect().map_err(Failure::Journal)?;
        let recorded = writer
            .record_preparation_commit(
                store::PreparationCommitInput {
                    intent: &record.intent,
                    retention,
                    check: record.check.as_ref(),
                    worktree: recipe,
                    report: &after.git.commit,
                },
                options.record,
            )
            .map_err(Failure::Record)?;
        if !same_record(&record, &recorded.record)
            || recorded.journal != history.journal.header
            || recorded
                .record
                .worktree
                .as_ref()
                .and_then(|recipe| recipe.completion.as_ref())
                .is_none_or(|done| done.report != after.git.commit)
        {
            return Err(Failure::Mismatch);
        }
        receipt = Some(recorded);
        writer.close().map_err(Failure::Journal)
    })();
    match result {
        Ok(()) => receipt.ok_or_else(|| WorktreeRecoveryError {
            cause: Box::new(Failure::Mismatch),
            progress,
            receipt: None,
        }),
        Err(cause) => Err(WorktreeRecoveryError {
            cause: Box::new(cause),
            progress,
            receipt: receipt.map(Box::new),
        }),
    }
}

// A previously unobserved completion may already exist after a lost receipt;
// nothing else in the immutable preparation may change during recovery.
fn same_record(expected: &PreparationRecord, actual: &PreparationRecord) -> bool {
    expected.intent == actual.intent
        && expected.retention == actual.retention
        && expected.check == actual.check
        && match (&expected.worktree, &actual.worktree) {
            (Some(left), Some(right)) => {
                left.metadata == right.metadata
                    && left.parent_source == right.parent_source
                    && left.check_sha256 == right.check_sha256
                    && left.created == right.created
                    && left
                        .completion
                        .as_ref()
                        .is_none_or(|done| right.completion.as_ref() == Some(done))
            }
            _ => false,
        }
}
