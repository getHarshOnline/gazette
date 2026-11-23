//! Release verification shares the pure compiler and explicit native application join.

mod observation;
pub mod recovery_session;
mod reviewed;
mod selection;
mod setup;
pub use gazette_core::release::{
    OutputCheckoutOptions, OutputCheckoutParent, OutputCheckoutReport, OutputCheckoutRequest,
    OutputCheckoutState, ParentAcquisitionOptions, ParentAcquisitionReport,
    ParentAcquisitionRequest, RepositorySetupEffect, RepositorySetupError, RepositorySetupLimits,
};
pub use reviewed::{ReviewedCredential, ReviewedScope, ReviewedSelection};
pub use setup::{acquire_parent, prepare_output_checkout};
pub mod session;
pub use gazette_application::release::{
    DispatchAttemptError, DispatchAttemptFailure, DispatchAttemptOptions, DispatchAttemptRequest,
    DispatchReviewedAttemptRequest, PreparedReviewedApprovalRequest,
};
pub use gazette_application::release::{
    ObservationSessionError, ObservationSessionOptions, ObservationSessionRequest,
};
pub use gazette_application::release::{
    ReconcileReleaseError, ReconcileReleaseFailure, ReconcileReleaseOptions,
    ReconcileReleaseRequest,
};
pub use gazette_core::release::{
    DispatchEffect, DispatchError, DispatchFailure, DispatchObservation, DispatchOptions,
    DispatchStatus, GitCredentialSelection, GitSelection,
};
pub use observation::{ObservationSessionReport, record_observation};
pub use selection::parse_preparation_selection;

pub use gazette_application::release::{
    ConsumeAuthorizationError, ConsumeAuthorizationOptions, ConsumeAuthorizationRequest,
    ConsumeFailure,
};
pub use gazette_application::release::{
    PreparationAdmission, PreparationBatch, PreparationBatchError, PreparationBatchOptions,
    PreparationBatchRequest, PreparationEvidenceDigests, PreparationSelection,
    PreparedAuthorizationError, PreparedAuthorizationOptions, PreparedAuthorizationReport,
    PreparedAuthorizationRequest, ReadbackReport, RefComparison, TargetReadback, TargetReadbackRow,
};
pub use gazette_application::release::{RecordObservationError, RecordObservationOptions};
pub use gazette_application::release::{RegisterAuthorizationError, RegisterAuthorizationOptions};
pub use gazette_compiler::release::{
    Error, Limits, TargetSet, VerificationReport, VerifiedAuthorization, check_targets,
    parse_binding, parse_reviewed_binding, parse_targets, reviewed_body_digest, verify,
    verify_reviewed,
};
pub use gazette_core::release::{
    Authorization, AuthorizationSchema, Binding, CommitEffect, CommitError, CommitMetadata,
    CommitOptions, CommitReport, CommitSignature, CustodyEffect, CustodyError, CustodyOptions,
    CustodyReport, DestinationError, DispatchProfile, DispatchRequest, HistoryLimits, KeyState,
    ParentSource, PublicationDestination, PublisherAuthorization, PublisherBinding, PublisherRole,
    RefAdvertisement, ReleaseBinding, ReleaseDocument, RemoteReadError, RemoteReadFailure,
    RemoteReadOptions, RemoteState, ReviewedApproval, ReviewedApprovalSchema, ReviewedBinding,
    ReviewedBindingSchema, ReviewedPublisher, SignatureEnvelope, SignatureSchema, Target,
    WorktreeEffect, WorktreeError, WorktreeLimits, WorktreeOptions, WorktreeOutputSyncOptions,
    WorktreeReport, WorktreeSyncError, WorktreeSyncOptions, custody_reference,
    validate_git_destination, validate_worktree_parent,
};
pub use gazette_core::store::{
    RefObservation, RefObservationInspection, RefObservationReceipt, RefObservationWriteOptions,
};
pub use gazette_ports::release::{AuthorityError, AuthoritySource};

use gazette_adapter_fs::FileSystem;
use gazette_adapter_git::Git;
use gazette_adapter_sqlite::Sqlite;
use std::path::Path;

/// Inspect exact durable observation history without contacting destinations.
///
/// # Errors
/// Refuses native storage, expected-header or retained target-contract mismatch.
pub fn inspect_target_observation(
    path: &Path,
    id: gazette_core::source::Sha256Digest,
    expected: Option<&gazette_core::store::JournalHeader>,
    read: gazette_core::store::ReadOptions,
    limits: Limits,
) -> Result<RefObservationInspection, gazette_core::store::Error> {
    gazette_application::release::inspect_target_observation(
        &Sqlite, path, id, expected, read, limits,
    )
}

/// Observe the complete checked target set through an explicitly selected native Git installation.
/// The isolation directory must already exist, be empty/private, and remain exclusively controlled.
/// This performs configured destination reads, never pushes or release-journal mutations.
///
/// # Errors
/// Refuses global invocation permission/bounds; per-target native failures remain explicit rows.
pub fn observe_targets(
    git: &GitSelection,
    targets: &TargetSet,
    options: RemoteReadOptions,
) -> Result<ReadbackReport, RemoteReadError> {
    let reader = gazette_adapter_git::RemoteReader {
        runner: &gazette_adapter_process::Runner,
        selection: git,
    };
    gazette_application::release::observe_targets(&reader, targets, options)
}

/// Inspect a complete ordered set of checked local preparations without releasing.
/// Explicit `SQLite` coordination-file permission is required. Original inputs
/// are not reopened, and matching native bytes do not certify publication meaning.
///
/// # Errors
/// Refuses incomplete/changed history, output, evidence, selection or native bounds.
pub fn inspect_preparations(
    request: PreparationBatchRequest<'_>,
    options: PreparationBatchOptions,
) -> Result<PreparationBatch, PreparationBatchError> {
    gazette_application::release::inspect_preparations(&FileSystem, &Git, &Sqlite, request, options)
}

/// Verify exact signatures and their complete native local preparation bindings.
/// No signing, SQL/Git writes, authority consumption, network or release occurs.
/// This supplements pure [`verify`]; neither operation certifies or approves content.
///
/// # Errors
/// Refuses pure signature/binding failures or any selected native/evidence mismatch.
pub fn verify_prepared_authorization(
    request: PreparedAuthorizationRequest<'_>,
    options: PreparedAuthorizationOptions,
) -> Result<PreparedAuthorizationReport, PreparedAuthorizationError> {
    gazette_application::release::verify_prepared_authorization(
        &FileSystem,
        &Git,
        &Sqlite,
        request,
        options,
    )
}
