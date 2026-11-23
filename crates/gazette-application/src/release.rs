//! Technical preparation and explicitly selected ref observation; no publication approval or push.

mod authorization;
mod checkpoint;
mod consumption;
mod custody;
mod dispatch;
mod inspection;
mod journal_worktree;
mod observation;
mod observation_session;
mod preparations;
mod recorded_observation;
mod recovery;
mod registration;
mod reviewed;
mod settlement;
mod worktree;
pub use authorization::{
    PreparedAuthorizationError, PreparedAuthorizationOptions, PreparedAuthorizationReport,
    PreparedAuthorizationRequest, PreparedAuthorizationStatus, verify_prepared_authorization,
};
pub use checkpoint::{
    CheckpointError, CheckpointOptions, CheckpointRequest, checkpoint_commit, inspect_checkpoint,
};
pub use consumption::{
    ConsumeAuthorizationError, ConsumeAuthorizationOptions, ConsumeAuthorizationRequest,
    ConsumeFailure, consume_prepared_authorization,
};
pub use custody::{CustodyRequest, inspect_custody, retain_commit};
pub use dispatch::{
    DispatchAttemptError, DispatchAttemptFailure, DispatchAttemptOptions, DispatchAttemptRequest,
    dispatch_prepared_attempt,
};
pub use inspection::{
    JournalWorktreeInspection, JournalWorktreeInspectionError, JournalWorktreeInspectionOptions,
    JournalWorktreeInspectionRequest, inspect_journal_worktree,
};
pub use journal_worktree::{
    JournalWorktreeError, JournalWorktreeFailure, JournalWorktreeOptions, JournalWorktreeRequest,
    prepare_journal_worktree,
};
pub use observation::{
    ReadbackReport, ReadbackStatus, RefComparison, TargetReadback, TargetReadbackRow,
    observe_targets,
};
pub use observation_session::{
    ObservationSessionError, ObservationSessionOptions, ObservationSessionRequest,
    record_observation_session,
};
pub use preparations::{
    PreparationAdmission, PreparationBatch, PreparationBatchError, PreparationBatchOptions,
    PreparationBatchRequest, PreparationEvidenceDigests, PreparationSelection, PreparationStatus,
    inspect_preparations,
};
pub use recorded_observation::{
    RecordObservationError, RecordObservationOptions, inspect_target_observation,
    record_target_observation,
};
pub use recovery::{
    RecoverRecordedWorktreeRequest, RecoverWorktreeOptions, RecoverWorktreeRequest,
    SynchronizationState, WorktreeRecoveryError, WorktreeRecoveryFailure, WorktreeRecoveryProgress,
    recover_journal_worktree, recover_recorded_worktree,
};
pub use registration::{
    RegisterAuthorizationError, RegisterAuthorizationOptions, register_prepared_authorization,
};
pub use reviewed::{
    DispatchReviewedAttemptRequest, PreparedReviewedApprovalRequest, consume_reviewed_approval,
    dispatch_reviewed_attempt, register_reviewed_approval, verify_prepared_reviewed_approval,
};
pub use settlement::{
    ReconcileReleaseError, ReconcileReleaseFailure, ReconcileReleaseOptions,
    ReconcileReleaseRequest, reconcile_release,
};
pub use worktree::{WorktreeInspection, WorktreeRequest, inspect_worktree, prepare_worktree};

use gazette_core::{
    release::{CommitError, CommitMetadata, CommitOptions, CommitReport},
    source::{CommitId, RevisionLimits, TreeIdentity},
};
use gazette_ports::{destination::RootSeparation, release::ReleaseCommits, source::SourceCapture};
use std::path::{Path, PathBuf};

/// Capture an exact output and prepare its detached commit in separate local storage.
///
/// All configured native inputs must be included in `protected_roots`. The output
/// itself is always protected. An input pin establishes bytes, not review or
/// disclosure approval. Success is object read-back, not durable preparation,
/// a deploy worktree, a release journal record or permission to consume authority.
///
/// # Errors
/// Refuses unacknowledged object writes, unsafe/overlapping roots, missing or
/// changed output, unsupported native state, exceeded bounds or failed read-back.
#[allow(clippy::too_many_arguments)]
pub fn prepare_commit(
    source: &impl SourceCapture,
    separation: &impl RootSeparation,
    git: &impl ReleaseCommits,
    repository: &Path,
    output_root: &Path,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
    protected_roots: &[PathBuf],
    options: CommitOptions,
) -> Result<CommitReport, CommitError> {
    if !options.allow_write_objects {
        return Err(CommitError::refused(
            "explicit local Git object-write permission is required",
        ));
    }
    let mut protected = protected_roots.to_vec();
    protected.push(output_root.to_owned());
    separation
        .check_root_separation(repository, &protected)
        .map_err(|_| {
            CommitError::refused(
                "deployment storage overlaps or cannot be separated from protected inputs",
            )
        })?;
    let captured =
        crate::source::capture_pinned(source, output_root, expected, options.limits.content)
            .map_err(|_| {
                CommitError::refused(
                    "output capture failed or differs from the exact retained tree pin",
                )
            })?;
    separation
        .check_root_separation(repository, &protected)
        .map_err(|_| {
            CommitError::refused("deployment storage separation changed before object writes")
        })?;
    git.prepare_commit(repository, captured.as_input(), metadata, options)
}

/// Independently compare existing commit objects against exact expected inputs.
/// No source recapture, database, clock, ref update or write is performed.
///
/// # Errors
/// Refuses unsafe/missing/corrupt objects, mismatched recipe/tree or exhausted bounds.
pub fn verify_commit(
    git: &impl ReleaseCommits,
    repository: &Path,
    commit: &CommitId,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
    limits: RevisionLimits,
) -> Result<CommitReport, CommitError> {
    git.verify_commit(repository, commit, expected, metadata, limits)
}
