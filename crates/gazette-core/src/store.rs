//! Explicit store mechanics and evidence, not adoption or release authority.

use std::{fmt, io};

use serde::{Deserialize, Serialize};

mod adoption;
mod attempt;
mod authorization;
mod backup;
mod cache;
mod checkpoint;
mod consumption;
mod coordinator;
mod initialize;
mod journal;
mod materialization;
mod output;
mod preparation;
mod readback;
mod registry;
mod restore;
mod session;
mod settlement;
mod writer;
pub use adoption::{
    LocalCoordinatorAdoption, RestoreAdoptionOptions, RestoreAdoptionReceipt,
    RestoreAdoptionSource, RestoreAdoptionTarget, RestoreInspection,
};
pub use attempt::{
    ReleaseAttempt, ReleaseAttemptInspection, ReleaseAttemptKey, ReleaseAttemptOptions,
    ReleaseAttemptReceipt, ReleaseAttemptResult, ReleaseAttemptStart,
};
pub use authorization::{
    ReleaseAuthorizationEffect, ReleaseAuthorizationInput, ReleaseAuthorizationInspection,
    ReleaseAuthorizationReceipt, ReleaseAuthorizationRecord, ReleaseAuthorizationWriteError,
    ReleaseAuthorizationWriteOptions,
};
pub use backup::{
    BackupInspection, BackupOptions, BackupReceipt, BackupSnapshot, BackupVerifyOptions,
};
pub use cache::{
    BuildCacheDependency, BuildCacheDependencyKind, BuildCacheDependencySet, BuildCacheHead,
    BuildCacheInput, BuildCacheInspection, BuildCacheReceipt, BuildCacheSite, BuildCacheSiteInput,
    BuildCacheWriteOptions, CachedBuildSite,
};
pub use checkpoint::{
    CheckpointEffect, CheckpointInput, CheckpointInspection, CheckpointReceipt,
    CheckpointWriteError, CheckpointWriteOptions, CommitCheckpoint,
};
pub use consumption::{
    ReleaseBatchSchema, ReleaseConsumption, ReleaseConsumptionInput, ReleaseConsumptionInspection,
    ReleaseConsumptionOptions, ReleaseConsumptionReceipt, ReleaseCoordinator, ReleaseLease,
    ReleaseLeaseInspection, ReleaseLeaseOptions, ReleaseLeases, ReleaseTargetIntent,
    ReleaseWriteEffect, ReleaseWriteError,
};
pub use coordinator::{
    LocalCoordinatorBinding, LocalCoordinatorSeal, LocalCoordinatorSelection,
    LocalCoordinatorState, LocalCoordinatorTransaction, RecoveryProfile,
};
pub use initialize::{InitializationReceipt, InitializeOptions};
pub use journal::{JournalHeader, JournalInspection};
pub use materialization::{
    MaterializationCompletion, MaterializationCompletionInput, MaterializationEffect,
    MaterializationFrontier, MaterializationInput, MaterializationInspection,
    MaterializationIntent, MaterializationReceipt, MaterializationRecord,
    MaterializationWriteError, MaterializationWriteOptions,
};
pub use output::{OutputError, OutputLimits};
pub use preparation::{
    PreparationCheck, PreparationCheckInput, PreparationCommit, PreparationCommitInput,
    PreparationEffect, PreparationInput, PreparationInspection, PreparationIntent,
    PreparationReceipt, PreparationRecord, PreparationRetention, PreparationRetentionInput,
    PreparationWorktree, PreparationWorktreeInput, PreparationWriteError, PreparationWriteOptions,
};
pub use readback::{
    RefObservation, RefObservationEffect, RefObservationInput, RefObservationInspection,
    RefObservationReceipt, RefObservationWriteError, RefObservationWriteOptions,
};
pub use registry::{
    RegistryEffect, RegistryHead, RegistryInput, RegistryInspection, RegistryReceipt,
    RegistryWriteError, RegistryWriteOptions,
};
pub use restore::{RestoreOptions, RestorePreparation, RestoreReceipt};
pub use session::SessionOptions;
pub use settlement::{
    ReleaseAttemptReference, ReleaseReconciliation, ReleaseReconciliationFrontier,
    ReleaseReconciliationInspection, ReleaseReconciliationMode, ReleaseReconciliationOptions,
    ReleaseReconciliationReceipt, ReleaseReconciliationRecord, ReleaseSettlement,
    ReleaseSettlementState, ReleaseTargetOutcome, ReleaseTargetSettlement,
};
pub use writer::{GenerationEffect, WriterAcquireError, WriterOptions};

/// Explicit connection effects and bounded observation work.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReadOptions {
    /// Allow native WAL/shared-memory coordination files to be maintained.
    /// This never permits SQL data writes, creation, migration or recovery writes.
    pub allow_coordination_files: bool,
    /// Maximum combined observed database and existing sidecar bytes before opening.
    #[schemars(range(max = u64::MAX))]
    pub storage_bytes: u64,
    /// Maximum `SQLite` busy retry duration per lock attempt, not a total deadline.
    #[schemars(range(max = u32::MAX))]
    pub busy_timeout_ms: u32,
    /// Maximum statement or schema SQL bytes handled by the connection.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub sql_bytes: i32,
    /// Maximum individual value or row bytes handled by the connection.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub value_bytes: i32,
    /// Maximum VM progress callbacks per native connection, including all session observations.
    #[schemars(range(max = u64::MAX))]
    pub vm_steps: u64,
}

impl Default for ReadOptions {
    fn default() -> Self {
        Self {
            allow_coordination_files: false,
            storage_bytes: 256 * 1024 * 1024,
            busy_timeout_ms: 250,
            sql_bytes: 1024 * 1024,
            value_bytes: 1024 * 1024,
            vm_steps: 100_000,
        }
    }
}

impl ReadOptions {
    /// Check the explicit effect permission and native limit representation.
    ///
    /// # Errors
    ///
    /// Refuses unacknowledged coordination effects or invalid nonpositive bounds.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_coordination_files {
            return Err(Error::CoordinationPermission);
        }
        if self.storage_bytes == 0
            || self.sql_bytes <= 0
            || self.value_bytes <= 0
            || self.vm_steps == 0
            || self.busy_timeout_ms > i32::MAX as u32
        {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// One connected provider's raw header metadata under a SQL read transaction.
///
/// This is not an integrity check, a stable artifact identity, a selected Website
/// schema, ownership proof, write readiness or recovery/release certification.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    /// Concrete implementation name, supplied by its adapter.
    pub provider: String,
    /// Exact linked provider version.
    pub provider_version: String,
    /// Provider's source/build identity.
    pub provider_source_id: String,
    /// Native database application marker; unrecognized values are not promoted.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub application_id: i32,
    /// Native user/schema marker, not Gazette's acceptance of that schema.
    #[schemars(range(min = i32::MIN, max = i32::MAX))]
    pub user_version: i32,
    /// Observed journaling mode; it is not changed by this operation.
    pub journal_mode: String,
    /// Whether the actual database connection is SQL read-only.
    pub sql_read_only: bool,
}

/// A failed store operation; no successful partial observation or backup is returned.
#[derive(Debug)]
pub enum Error {
    /// Configured-output history writes were not explicitly permitted.
    MaterializationPermission,
    /// Intent, native read-back, stored history or ownership linkage failed validation.
    MaterializationVerification,
    /// An operation coordinate or destination head binds different exact intent.
    MaterializationConflict,
    /// A previous output operation remains unresolved; do not start or blindly retry.
    MaterializationPending,
    /// Local reconciliation and settlement evidence was not permitted.
    ReleaseSettlementPermission,
    /// Complete reconciliation evidence, frontier or derived settlement failed verification.
    ReleaseSettlementVerification,
    /// Reconciliation identity or current history differs from the selected original observation.
    ReleaseSettlementConflict,
    /// This consumed batch is terminal and cannot resume under its original authorization.
    ReleaseTerminal,
    /// Local attempt evidence was not explicitly permitted.
    ReleaseAttemptPermission,
    /// Attempt linkage, comparison or native read-back is invalid.
    ReleaseAttemptVerification,
    /// An attempt coordinate already binds different evidence or is out of order.
    ReleaseAttemptConflict,
    /// Earlier effects or custody must be reconciled before any further dispatch.
    ReleaseReconciliationRequired,
    /// Pending authorization registration was not explicitly permitted.
    ReleaseAuthorizationPermission,
    /// Pending authorization representation, linkage or native read-back failed.
    ReleaseAuthorizationVerification,
    /// A record identity or scoped nonce already binds another exact authorization.
    ReleaseAuthorizationConflict,
    /// Missing explicit destination lease permission.
    ReleaseLeasePermission,
    /// Invalid or stale exact lease set.
    ReleaseLeaseVerification,
    /// Destination held by another unresolved authorization.
    ReleaseLeaseConflict,
    /// Missing explicit one-use consumption permission.
    ReleaseConsumptionPermission,
    /// Invalid consumed input or historical linkage.
    ReleaseConsumptionVerification,
    /// Original authorization is already durably consumed.
    ReleaseAlreadyConsumed,
    /// Remote observations do not prove every exact reviewed pre-state.
    ReleasePrestate,
    /// Anchor enrollment or state updates were not explicitly permitted.
    CoordinatorPermission,
    /// Independently selected local coordinator/journal binding or clean bytes differ.
    CoordinatorMismatch,
    /// Anchor is unsupported, malformed, noncanonical or still in-progress.
    CoordinatorState,
    /// A release attempt exists; unstarted recovery cannot prove effect quiescence.
    ReleaseQuiescenceRequired,
    /// Anchor persistence was attempted; preserve all residue and inspect before any retry.
    CoordinatorUnverified(Box<Error>),
    /// Observation recording was not explicitly permitted.
    ReadbackPermission,
    /// Stored observation is incomplete, inconsistent or corrupted.
    ReadbackVerification,
    /// An observation coordinate already exists or binds different evidence.
    ReadbackConflict,
    /// Preparation history effects were not explicitly permitted.
    PreparationPermission,
    /// Preparation input or stored history failed exact verification.
    PreparationVerification,
    /// An existing preparation coordinate binds different inputs.
    PreparationConflict,
    /// Recording immutable local commit evidence was not explicitly permitted.
    CheckpointPermission,
    /// Checkpoint input, stored row or independent read-back failed verification.
    CheckpointVerification,
    /// An existing checkpoint coordinate is bound to different exact inputs.
    CheckpointConflict,
    /// Replacing the complete private registry was not explicitly permitted.
    RegistryPermission,
    /// Registry coverage, constraints, rows or stored identity failed verification.
    RegistryVerification,
    /// Replacing the rebuildable publication cache was not explicitly permitted.
    BuildCachePermission,
    /// Cached dependencies, artifacts or complete-set identity failed verification.
    BuildCacheVerification,
    /// Advancing the local journal generation was not explicitly permitted.
    WriterPermission,
    /// The native generation cannot be incremented without overflow.
    WriterExhausted,
    /// Fresh journal creation was not explicitly permitted.
    InitializePermission,
    /// Fresh initialization requires an empty directory; existing output is never reused.
    StoreExists,
    /// Initial schema/header/integrity read-back disagrees with the committed transaction.
    JournalVerification,
    /// The journal's native markers or exact DDL do not match a supported storage schema.
    JournalSchema,
    /// Observed journal coordinates differ from the caller's complete expected header.
    JournalMismatch,
    /// Local lifetime coordination reservation was not explicitly permitted.
    ReservationPermission,
    /// Another session holds the selected local coordination directory.
    ReservationBusy,
    /// Coordination-file effects were not explicitly acknowledged.
    CoordinationPermission,
    /// A resource setting cannot be applied as specified.
    InvalidLimits,
    /// Native file access failed; missing source databases are never created.
    Io(io::Error),
    /// A selected file/sidecar is unsafe, or the backup parent is not a directory.
    UnsafePath,
    /// The selected file is empty, not an initialized database.
    Uninitialized,
    /// Pre-open database and sidecar byte bounds were exceeded.
    StorageLimit,
    /// A source/output path identity, size or expected inventory changed.
    Changed,
    /// Native locking could not complete within the selected busy policy.
    Busy,
    /// The provider requires writable recovery, which observation never authorizes.
    RecoveryRequired,
    /// Backup creation was not explicitly authorized.
    BackupPermission,
    /// A backup byte or native copy-step ceiling was exhausted.
    BackupLimit,
    /// Copied database or receipt read-back failed verification.
    BackupVerification,
    /// Retained backup manifest is malformed, unsupported or inconsistent.
    BackupManifest,
    /// Retained manifest exceeds the selected pre-decode byte ceiling.
    BackupManifestLimit,
    /// Detached restoration preparation was not explicitly permitted.
    RestorePermission,
    /// Live adoption of a verified restore was not explicitly permitted.
    RestoreAdoptionPermission,
    /// Restore adoption evidence is incomplete, stale or does not match the clean frontier.
    RestoreAdoptionVerification,
    /// Native progress limits stopped the operation.
    Interrupted,
    /// The native provider rejected input or failed; SQL and row data are not copied.
    Provider {
        /// Native extended diagnostic code, when available.
        code: Option<i32>,
    },
}

impl Error {
    /// Stable failure class independent of platform wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MaterializationPermission => "store_materialization_permission",
            Self::MaterializationVerification => "store_materialization_verification",
            Self::MaterializationConflict => "store_materialization_conflict",
            Self::MaterializationPending => "store_materialization_pending",
            Self::BuildCachePermission => "store_build_cache_permission",
            Self::BuildCacheVerification => "store_build_cache_verification",
            Self::CoordinatorPermission => "store_coordinator_permission",
            Self::CoordinatorMismatch => "store_coordinator_mismatch",
            Self::CoordinatorState => "store_coordinator_state",
            Self::ReleaseQuiescenceRequired => "store_release_quiescence_required",
            Self::CoordinatorUnverified(_) => "store_coordinator_unverified",
            Self::ReleaseAuthorizationPermission => "store_release_authorization_permission",
            Self::ReleaseAuthorizationVerification => "store_release_authorization_verification",
            Self::ReleaseAuthorizationConflict => "store_release_authorization_conflict",
            Self::ReleaseLeasePermission => "store_release_lease_permission",
            Self::ReleaseLeaseVerification => "store_release_lease_verification",
            Self::ReleaseLeaseConflict => "store_release_lease_conflict",
            Self::ReleaseConsumptionPermission => "store_release_consumption_permission",
            Self::ReleaseConsumptionVerification => "store_release_consumption_verification",
            Self::ReleaseSettlementPermission => "store_release_settlement_permission",
            Self::ReleaseSettlementVerification => "store_release_settlement_verification",
            Self::ReleaseSettlementConflict => "store_release_settlement_conflict",
            Self::ReleaseTerminal => "store_release_terminal",
            Self::ReleaseAttemptPermission => "store_release_attempt_permission",
            Self::ReleaseAttemptVerification => "store_release_attempt_verification",
            Self::ReleaseAttemptConflict => "store_release_attempt_conflict",
            Self::ReleaseReconciliationRequired => "store_release_reconciliation_required",
            Self::ReleaseAlreadyConsumed => "store_release_already_consumed",
            Self::ReleasePrestate => "store_release_prestate",
            Self::ReadbackPermission => "store_readback_permission",
            Self::ReadbackVerification => "store_readback_verification",
            Self::ReadbackConflict => "store_readback_conflict",
            Self::PreparationPermission => "store_preparation_permission",
            Self::PreparationVerification => "store_preparation_verification",
            Self::PreparationConflict => "store_preparation_conflict",
            Self::CheckpointPermission => "store_checkpoint_permission",
            Self::CheckpointVerification => "store_checkpoint_verification",
            Self::CheckpointConflict => "store_checkpoint_conflict",
            Self::RegistryPermission => "store_registry_permission",
            Self::RegistryVerification => "store_registry_verification",
            Self::WriterPermission => "store_writer_permission",
            Self::WriterExhausted => "store_writer_exhausted",
            Self::InitializePermission => "store_initialize_permission",
            Self::StoreExists => "store_exists",
            Self::JournalVerification => "store_journal_verification",
            Self::JournalSchema => "store_journal_schema",
            Self::JournalMismatch => "store_journal_mismatch",
            Self::ReservationPermission => "store_reservation_permission",
            Self::ReservationBusy => "store_reservation_busy",
            Self::CoordinationPermission => "store_coordination_permission",
            Self::InvalidLimits => "store_invalid_limits",
            Self::Io(_) => "store_io",
            Self::UnsafePath => "store_unsafe_path",
            Self::Uninitialized => "store_uninitialized",
            Self::StorageLimit => "store_storage_limit",
            Self::Changed => "store_changed",
            Self::Busy => "store_busy",
            Self::RecoveryRequired => "store_recovery_required",
            Self::BackupPermission => "store_backup_permission",
            Self::BackupLimit => "store_backup_limit",
            Self::BackupVerification => "store_backup_verification",
            Self::BackupManifest => "store_backup_manifest",
            Self::BackupManifestLimit => "store_backup_manifest_limit",
            Self::RestorePermission => "store_restore_permission",
            Self::RestoreAdoptionPermission => "store_restore_adoption_permission",
            Self::RestoreAdoptionVerification => "store_restore_adoption_verification",
            Self::Interrupted => "store_interrupted",
            Self::Provider { .. } => "store_provider",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaterializationPermission => f.write_str("output-history recording requires explicit permission; no filesystem or publication authority follows"),
            Self::MaterializationVerification => f.write_str("output history or complete native read-back failed verification; preserve the destination and journal"),
            Self::MaterializationConflict => f.write_str("output operation coordinate or ownership head binds different intent; preserve original history"),
            Self::MaterializationPending => f.write_str("configured destination has an unresolved output operation; inspect its exact intent and native state; never blindly retry"),
            Self::BuildCachePermission => f.write_str("build-cache replacement includes complete-set stale-row removal; explicitly permit it; authored truth and release history are never replaced"),
            Self::BuildCacheVerification => f.write_str("build-cache dependencies, artifacts or complete-set identity failed verification; discard the rebuildable projection and perform a clean build"),
            Self::CoordinatorPermission => f.write_str("local coordinator anchor writes require explicit permission; no release authority follows"),
            Self::CoordinatorMismatch => f.write_str("local coordinator binding or sealed journal bytes differ; preserve independently retained state; do not adopt or reset"),
            Self::CoordinatorState => f.write_str("local coordinator anchor is invalid or in-progress; preserve and inspect; no automatic repair or new writer is permitted"),
            Self::ReleaseQuiescenceRequired => f.write_str("journal contains a release attempt; unstarted recovery is unavailable without separately qualified effect quiescence"),
            Self::CoordinatorUnverified(cause) => write!(f, "{cause}; coordinator anchor may have changed even without journal generation advancement; preserve anchor and journal; never automatically retry or infer rollback"),
            Self::ReleaseAuthorizationPermission => f.write_str("pending authorization registration requires explicit permission; no consumption or dispatch follows"),
            Self::ReleaseAuthorizationVerification => f.write_str("authorization history failed verification; preserve the journal and inspect exact original evidence"),
            Self::ReleaseAuthorizationConflict => f.write_str("authorization identity or scoped nonce already binds different evidence; preserve original history"),
            Self::ReleaseLeasePermission => f.write_str("destination leases require explicit permission; no lease acquisition attempted"),
            Self::ReleaseLeaseVerification => f.write_str("destination lease evidence is incomplete or stale; inspect the original holder and generations"),
            Self::ReleaseLeaseConflict => f.write_str("destination has another unresolved holder; preserve existing release history"),
            Self::ReleaseConsumptionPermission => f.write_str("authorization consumption requires explicit permission; no consumption attempted"),
            Self::ReleaseConsumptionVerification => f.write_str("consumption evidence failed verification; preserve the journal and inspect the exact original intent"),
            Self::ReleaseSettlementPermission => f.write_str("release reconciliation recording was not permitted"),
            Self::ReleaseSettlementVerification => f.write_str("release reconciliation or complete-set settlement failed verification"),
            Self::ReleaseSettlementConflict => f.write_str("release reconciliation conflicts with its exact selected history"),
            Self::ReleaseTerminal => f.write_str("release batch is terminal; new effects require a new authorization"),
            Self::ReleaseAttemptPermission => f.write_str("release attempt recording was not permitted"),
            Self::ReleaseAttemptVerification => f.write_str("release attempt evidence failed exact verification"),
            Self::ReleaseAttemptConflict => f.write_str("release attempt coordinate conflicts with original history"),
            Self::ReleaseReconciliationRequired => f.write_str("release history requires explicit reconciliation; do not redispatch"),
            Self::ReleaseAlreadyConsumed => f.write_str("authorization is already consumed; inspect its original intent rather than consuming it again"),
            Self::ReleasePrestate => f.write_str("complete reviewed remote pre-state was not observed; do not consume or dispatch"),
            Self::ReadbackPermission => f.write_str("recording native ref observations requires explicit permission; no release authority follows"),
            Self::ReadbackVerification => f.write_str("ref observation failed complete evidence verification; preserve the journal and inspect the exact coordinate"),
            Self::ReadbackConflict => f.write_str("ref observation coordinate already binds history; inspect it and choose a new coordinate for a fresh observation"),
            Self::PreparationPermission => f.write_str("recording preparation requires explicit permission; no release authority follows"),
            Self::PreparationVerification => f.write_str("preparation evidence failed verification; preserve the journal and exact stage before reconciliation"),
            Self::PreparationConflict => f.write_str("preparation coordinate binds different inputs; preserve original intent and select a new coordinate for changed work"),
            Self::CheckpointPermission => f.write_str("recording local commit evidence requires explicit permission; no release authority follows"),
            Self::CheckpointVerification => f.write_str("local commit checkpoint failed verification; preserve the journal and inspect exact retained inputs"),
            Self::CheckpointConflict => f.write_str("checkpoint coordinate already binds different inputs; preserve the original record and use a new explicit coordinate for changed intent"),
            Self::RegistryPermission => f.write_str("registry replacement includes complete-set removals; explicitly permit that operation; no journal erasure or release authority follows"),
            Self::RegistryVerification => f.write_str("registry rows, constraints or identity failed verification; preserve the store and inspect the retained inputs; no complete registry evidence was produced"),
            Self::WriterPermission => f.write_str("writer acquisition changes the journal generation; explicitly permit that exact operation; no release authority follows"),
            Self::WriterExhausted => f.write_str("journal writer generation is exhausted; preserve the journal and resolve a separately governed transition; never wrap or reset the counter"),
            Self::InitializePermission => f.write_str("initialization creates a new journal; explicitly permit creation in an empty private directory; no adoption or release authority follows"),
            Self::StoreExists => f.write_str("initialization directory is not empty; preserve existing files and inspect their ownership/recovery state; never delete them to retry initialization"),
            Self::JournalVerification => f.write_str("journal failed coordinate or integrity read-back; retain the store for inspection; no successful journal verification evidence was produced"),
            Self::JournalSchema => f.write_str("journal storage markers or schema are unsupported, missing or changed; preserve the store and resolve its version/migration history; never initialize over it"),
            Self::JournalMismatch => f.write_str("journal identity, schema version, epoch or writer generation differs from the expected header; preserve both observations and reconcile their history before any new write"),
            Self::ReservationPermission => f.write_str("connecting reserves local coordination exclusively; explicitly permit that reservation; no SQL-write or release authority is granted"),
            Self::ReservationBusy => f.write_str("selected local coordination directory is reserved; close or reconcile its owning session; never replace the directory to bypass the lock"),
            Self::CoordinationPermission => f.write_str("connection may maintain WAL coordination files; explicitly allow those effects before connecting"),
            Self::InvalidLimits => f.write_str("invalid store operation limits; use positive representable ceilings"),
            Self::Io(error) => write!(f, "cannot access selected store files: {error}; inspect their native locations"),
            Self::UnsafePath => f.write_str("unsafe store path; require regular non-symlink database/sidecar files and an existing backup parent directory"),
            Self::Uninitialized => f.write_str("selected database is empty; initialization is a separate operation"),
            Self::StorageLimit => f.write_str("selected database/sidecars exceed the pre-open byte ceiling"),
            Self::Changed => f.write_str("store or backup path identity, size or inventory changed; preserve output and inspect native ownership before retrying"),
            Self::Busy => f.write_str("store is busy; inspect the current writer before retrying"),
            Self::RecoveryRequired => f.write_str("store requires writable recovery; observation made no recovery writes; preserve the database and sidecars for a separately authorized recovery operation"),
            Self::BackupPermission => f.write_str("backup creates private database output; explicitly allow backup creation"),
            Self::BackupLimit => f.write_str("backup exceeded its byte or copy-step ceiling; no complete backup receipt was produced"),
            Self::BackupVerification => f.write_str("backup or recovery copy failed database or receipt read-back; do not activate it"),
            Self::BackupManifest => f.write_str("backup manifest is malformed, unsupported or inconsistent; preserve it and recover the original receipt"),
            Self::BackupManifestLimit => f.write_str("backup manifest exceeds the pre-decode byte ceiling; inspect it before explicitly changing the limit"),
            Self::RestorePermission => f.write_str("restore preparation creates a detached private recovery copy; explicitly allow preparation without activating it"),
            Self::RestoreAdoptionPermission => f.write_str("live restore adoption requires explicit permission; no store was selected or replaced"),
            Self::RestoreAdoptionVerification => f.write_str("restore adoption evidence does not prove the exact latest clean frontier; preserve every root and stop without activating or retrying"),
            Self::Interrupted => f.write_str("store operation exceeded its native VM work ceiling"),
            Self::Provider { code } => write!(f, "store provider rejected the operation (native code {code:?}); inspect database/recovery state without reinitializing it"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CoordinatorUnverified(cause) => Some(cause.as_ref()),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}
