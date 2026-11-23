//! Direct local commit mechanics over the shared application and native adapters.

use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;
pub use gazette_core::release::{
    CommitEffect, CommitError, CommitMetadata, CommitOptions, CommitReport, CommitSignature,
    CustodyEffect, CustodyError, CustodyOptions, CustodyReport, custody_reference,
};
use gazette_core::source::Sha256Digest;
use gazette_core::source::{CommitId, RevisionLimits, TreeIdentity};
use std::path::{Path, PathBuf};

/// Bounded delivery of existing local journal checkpoint and recovery outcomes.
pub mod journal_output;
pub mod output;

pub use gazette_application::release::{CheckpointError, CheckpointOptions, CheckpointRequest};
pub use gazette_application::release::{
    JournalWorktreeError, JournalWorktreeFailure, JournalWorktreeOptions, JournalWorktreeRequest,
};
pub use gazette_application::release::{
    JournalWorktreeInspection, JournalWorktreeInspectionError, JournalWorktreeInspectionOptions,
    JournalWorktreeInspectionRequest,
};
pub use gazette_application::release::{
    RecoverRecordedWorktreeRequest, RecoverWorktreeOptions, RecoverWorktreeRequest,
    SynchronizationState, WorktreeRecoveryError, WorktreeRecoveryFailure, WorktreeRecoveryProgress,
};
pub use gazette_application::release::{WorktreeInspection, WorktreeRequest};
pub use gazette_core::release::{
    HistoryLimits, ParentSource, WorktreeEffect, WorktreeError, WorktreeLimits, WorktreeOptions,
    WorktreeReport, WorktreeSyncOptions,
};
pub use gazette_core::store::{
    CheckpointInspection, CheckpointReceipt, CheckpointWriteOptions, CommitCheckpoint,
};

/// Re-synchronize exact existing preparation and append/replay its immutable completion.
/// No initialization, restaging, parent-source acquisition, certification or release.
///
/// # Errors
/// Preserves stale/changed history, native verification and possible synchronization
/// or journal effects. Failure never permits recreating an existing worktree.
pub fn recover_journal_worktree(
    request: RecoverWorktreeRequest<'_>,
    options: RecoverWorktreeOptions,
) -> Result<gazette_core::store::PreparationReceipt, WorktreeRecoveryError> {
    gazette_application::release::recover_journal_worktree(
        &FileSystem,
        &Git,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}

/// Resolve exact existing history, preflight current bytes, then recover without recreation.
/// Requires every synchronization/journal permission before native access. No
/// publication, certification, source recapture or automatic rebind is performed.
///
/// # Errors
/// Preserves preflight refusal and later possible generation/synchronization/record effects.
pub fn recover_recorded_worktree(
    request: RecoverRecordedWorktreeRequest<'_>,
    options: RecoverWorktreeOptions,
) -> Result<gazette_core::store::PreparationReceipt, WorktreeRecoveryError> {
    gazette_application::release::recover_recorded_worktree(
        &FileSystem,
        &Git,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}
/// Compare exact historical preparation with current native Git and output bytes.
/// No SQL/Git writes, repair, synchronization or completion synthesis; `SQLite`
/// coordination files require explicit permission. Returned history is private.
///
/// # Errors
/// Refuses missing phases, stale expectations, unsafe or changed native state and bounds.
pub fn inspect_journal_worktree(
    request: JournalWorktreeInspectionRequest<'_>,
    options: JournalWorktreeInspectionOptions,
) -> Result<JournalWorktreeInspection, JournalWorktreeInspectionError> {
    gazette_application::release::inspect_journal_worktree(
        &FileSystem,
        &Git,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}

/// Record exact Git intent, prepare the retained worktree and record verified completion.
/// No recompilation, network, certification or publisher authorization is implied.
///
/// # Errors
/// Retains unresolved history and known native effects; never recreates an existing intent.
pub fn prepare_journal_worktree(
    request: JournalWorktreeRequest<'_>,
    options: JournalWorktreeOptions,
) -> Result<gazette_core::store::PreparationReceipt, JournalWorktreeError> {
    gazette_application::release::prepare_journal_worktree(
        &FileSystem,
        &Git,
        &gazette_adapter_sqlite::Sqlite,
        request,
        options,
    )
}

/// Record and prepare one worktree under the same independently selected local coordinator
/// used by its retained preparation history.
///
/// # Errors
/// Preserves the same native history/effect evidence as [`prepare_journal_worktree`] and also
/// refuses missing or changed coordinator custody.
pub fn prepare_coordinated_journal_worktree(
    coordinator: &crate::store::coordinator::LocalCoordinatorSelection,
    allow_anchor_write: bool,
    request: JournalWorktreeRequest<'_>,
    options: JournalWorktreeOptions,
) -> Result<gazette_core::store::PreparationReceipt, JournalWorktreeError> {
    gazette_application::release::prepare_journal_worktree(
        &FileSystem,
        &Git,
        &crate::store::coordinator::Connector {
            selection: Some(coordinator),
            allow_anchor_write,
        },
        request,
        options,
    )
}

/// Prepare an independent deployment worktree over exact previously retained output.
/// Creates local Git metadata only; does not restage output, acquire authorization
/// or append a preparation journal. The coordinator must record intent separately.
///
/// # Errors
/// Preserves source/pin refusal and possible native initialization residue.
pub fn prepare_worktree(
    request: WorktreeRequest<'_>,
    options: WorktreeOptions,
) -> Result<WorktreeInspection, WorktreeError> {
    gazette_application::release::prepare_worktree(&FileSystem, &Git, request, options)
}

/// Independently verify administrative state and the entire current output tree.
/// Does not initialize, repair, synchronize or establish prior preparation history.
///
/// # Errors
/// Refuses unsafe, corrupt, changed, overlapping or over-limit state.
pub fn inspect_worktree(
    request: WorktreeRequest<'_>,
    commit: &CommitId,
    limits: WorktreeLimits,
) -> Result<WorktreeInspection, WorktreeError> {
    gazette_application::release::inspect_worktree(&FileSystem, &Git, request, commit, limits)
}

/// Retain exact verified objects under immutable local custody, never a deployment branch.
/// Requires trusted exclusive repository control and all configured protected roots.
/// Native synchronization is not hardware power-loss qualification or a release receipt.
///
/// # Errors
/// Refuses permission, source overlap, conflicting refs, unsafe objects or bounds;
/// preserves possible ref effects without automatic rollback or retry.
pub fn retain(
    repository: &Path,
    id: Sha256Digest,
    commit: &CommitId,
    output: TreeIdentity,
    metadata: &CommitMetadata,
    protected_roots: &[PathBuf],
    options: CustodyOptions,
) -> Result<CustodyReport, CustodyError> {
    gazette_application::release::retain_commit(
        &Git,
        &FileSystem,
        gazette_application::release::CustodyRequest {
            repository,
            id,
            commit,
            output,
            metadata,
            protected_roots,
        },
        options,
    )
}

/// Read exact custody and objects without writes; no prior synchronization is inferred.
///
/// # Errors
/// Refuses missing, conflicting, symbolic, unsafe or over-limit refs and objects.
pub fn inspect_custody(
    repository: &Path,
    id: Sha256Digest,
    commit: &CommitId,
    output: TreeIdentity,
    metadata: &CommitMetadata,
    limits: RevisionLimits,
) -> Result<CustodyReport, CustodyError> {
    gazette_application::release::inspect_custody(
        &Git, repository, id, commit, output, metadata, limits,
    )
}

/// Record independently verified existing Git objects in the separate local journal.
/// No Git write, authorization consumption or deployment is performed.
///
/// # Errors
/// Preserves native verification, generation, record/replay and close failures.
pub fn checkpoint(
    root: &Path,
    expected: gazette_core::store::JournalHeader,
    request: CheckpointRequest<'_>,
    options: CheckpointOptions,
) -> Result<CheckpointReceipt, CheckpointError> {
    gazette_application::release::checkpoint_commit(
        &gazette_adapter_sqlite::Sqlite,
        &FileSystem,
        &Git,
        root,
        expected,
        request,
        options,
    )
}

/// Inspect historical local evidence; this does not verify current Git object availability.
///
/// # Errors
/// Refuses native/schema/record failures and any expected journal-coordinate mismatch.
pub fn inspect_checkpoint(
    path: &Path,
    id: gazette_core::source::Sha256Digest,
    expected: Option<&gazette_core::store::JournalHeader>,
    options: gazette_core::store::ReadOptions,
) -> Result<CheckpointInspection, gazette_core::store::Error> {
    gazette_application::release::inspect_checkpoint(
        &gazette_adapter_sqlite::Sqlite,
        path,
        id,
        expected,
        options,
    )
}

/// Prepare exact retained output as a detached local commit, without refs or network.
/// This is not a durable preparation receipt, worktree, certification or publication.
///
/// # Errors
/// Refuses permission, confinement, pin, metadata, native or resource failures;
/// possible object residue is reported without automatic cleanup or retry.
pub fn prepare(
    repository: &Path,
    output_root: &Path,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
    protected_roots: &[PathBuf],
    options: CommitOptions,
) -> Result<CommitReport, CommitError> {
    gazette_application::release::prepare_commit(
        &FileSystem,
        &FileSystem,
        &Git,
        repository,
        output_root,
        expected,
        metadata,
        protected_roots,
        options,
    )
}

/// Verify complete local commit objects against exact output and recipe expectations.
/// Does not read a worktree, require HEAD, write data or grant release authority.
///
/// # Errors
/// Refuses unsafe/missing/corrupt objects, mismatched recipe/tree or exhausted bounds.
pub fn verify(
    repository: &Path,
    commit: &CommitId,
    expected: TreeIdentity,
    metadata: &CommitMetadata,
    limits: RevisionLimits,
) -> Result<CommitReport, CommitError> {
    gazette_application::release::verify_commit(
        &Git, repository, commit, expected, metadata, limits,
    )
}
