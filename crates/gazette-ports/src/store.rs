//! Narrow store effects; initialization never overwrites an existing store.

use gazette_core::source::Sha256Digest;
use gazette_core::store::{
    BackupInspection, BackupOptions, BackupReceipt, BackupVerifyOptions, Error,
    InitializationReceipt, InitializeOptions, JournalHeader, JournalInspection, Observation,
    OutputError, ReadOptions, RestoreOptions, RestoreReceipt, SessionOptions, WriterAcquireError,
    WriterOptions,
};
use std::path::Path;

/// Configured-output history under one existing reserved journal writer lifetime.
/// The application owns native effects; this port owns exact append/head linkage.
pub trait MaterializationWriter: JournalWriter {
    /// Return actual held journal/anchor roots after validating native custody.
    /// These roots must stay outside every configured publication destination.
    ///
    /// # Errors
    /// Refuses stale or uncertain namespace state; never accepts caller root assertions.
    fn materialization_roots(&mut self) -> Result<Vec<std::path::PathBuf>, Error>;

    /// Inspect the destination head and unresolved operation without filesystem reads.
    ///
    /// # Errors
    /// Refuses stale, corrupt, unsafe or over-limit state and invalidates uncertain custody.
    fn inspect_materialization(
        &mut self,
        root: &Path,
    ) -> Result<gazette_core::store::MaterializationInspection, Error>;

    /// Append exact intent and claim its pending slot before any filesystem effect.
    /// Equal replay returns inserted=false and never authorizes another write.
    ///
    /// # Errors
    /// Refuses permission, changed payload, mismatched head or another unresolved operation.
    fn record_materialization_intent(
        &mut self,
        input: gazette_core::store::MaterializationInput<'_>,
        options: gazette_core::store::MaterializationWriteOptions,
    ) -> Result<
        gazette_core::store::MaterializationReceipt,
        gazette_core::store::MaterializationWriteError,
    >;

    /// Verify original intent and append completion/head advance in one transaction.
    /// Native read-back must match the original binding and complete expected tree.
    ///
    /// # Errors
    /// Refuses missing/changed intent, wrong pending slot, divergent read-back or uncertain SQL.
    fn record_materialization_completion(
        &mut self,
        input: gazette_core::store::MaterializationCompletionInput<'_>,
        options: gazette_core::store::MaterializationWriteOptions,
    ) -> Result<
        gazette_core::store::MaterializationReceipt,
        gazette_core::store::MaterializationWriteError,
    >;
}

/// Historical output inspection without migration, reconciliation or native effects.
pub trait MaterializationReader {
    /// Read one exact destination frontier in a verified store snapshot.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, unsupported or over-limit storage; never repairs it.
    fn inspect_materialization(
        &self,
        path: &Path,
        root: &Path,
        options: ReadOptions,
    ) -> Result<gazette_core::store::MaterializationInspection, Error>;
}

/// Reconciliation-only evidence under actual native custody, never implicit attempt adoption.
pub trait ReleaseReconciliationWriter: ReleaseCoordinatorReader {
    /// Interpretation permitted by this held lifetime; cannot weaken during acquisition.
    /// Native appends independently check this restriction before accepting new evidence.
    fn reconciliation_mode(&self) -> gazette_core::store::ReleaseReconciliationMode;

    /// Verify the exact selected native database and inspect history without renewing authority.
    ///
    /// # Errors
    /// Refuses stale, corrupt or over-limit native state and invalidates uncertain lifetimes.
    fn inspect_release_reconciliations(
        &mut self,
        database: &Path,
        record_id: &str,
    ) -> Result<gazette_core::store::ReleaseReconciliationInspection, Error>;

    /// Atomically bind complete observations to exact current history and derive settlement.
    /// May use a cleanly acquired newer holder with the same fixed native binding.
    ///
    /// # Errors
    /// Refuses absent permission, stale frontier, changed original intent, terminal history,
    /// wrong actual database/custody, incomplete target coverage or uncertain persistence.
    fn record_release_reconciliation(
        &mut self,
        database: &Path,
        input: &gazette_core::store::ReleaseReconciliation,
        options: gazette_core::store::ReleaseReconciliationOptions,
    ) -> Result<
        gazette_core::store::ReleaseReconciliationReceipt,
        gazette_core::store::ReleaseWriteError,
    >;
}

/// Clock-free historical reconciliation/settlement observation, without repair or provider access.
pub trait ReleaseReconciliationReader {
    /// Read complete original history and validate every immutable frontier and derived conclusion.
    ///
    /// # Errors
    /// Refuses unsafe, unsupported, corrupt, partial or over-limit storage without modification.
    fn inspect_release_reconciliations(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<gazette_core::store::ReleaseReconciliationInspection, Error>;
}

/// Exact original-holder attempt appends. Native storage owns custody, ordering and replay.
pub trait ReleaseAttemptWriter: ReleaseConsumptionWriter {
    /// Inspect complete history without renewing authority or sampling destinations.
    ///
    /// # Errors
    /// Refuses stale or corrupt native state and invalidates uncertain lifetimes.
    fn inspect_release_attempts(
        &mut self,
        record_id: &str,
    ) -> Result<gazette_core::store::ReleaseAttemptInspection, Error>;

    /// Durably append one exact start before dispatch; exact replay returns created=false.
    /// The database must be the actual held database, never an equal-header copy.
    ///
    /// # Errors
    /// Refuses missing consumption, changed intent, inactive leases, gaps or unresolved history.
    fn start_release_attempt(
        &mut self,
        database: &Path,
        input: &gazette_core::store::ReleaseAttemptStart,
        options: gazette_core::store::ReleaseAttemptOptions,
    ) -> Result<gazette_core::store::ReleaseAttemptReceipt, gazette_core::store::ReleaseWriteError>;

    /// Retain exact transport and independent read-back evidence after a persisted start.
    /// Exact replay is harmless; changed evidence never overwrites the original result.
    ///
    /// # Errors
    /// Refuses missing/changed start, stale custody, invalid comparisons or uncertain writes.
    fn finish_release_attempt(
        &mut self,
        input: &gazette_core::store::ReleaseAttemptResult,
        options: gazette_core::store::ReleaseAttemptOptions,
    ) -> Result<gazette_core::store::ReleaseAttempt, gazette_core::store::ReleaseWriteError>;
}

/// Historical attempt inspection requires neither current authority nor native remote access.
pub trait ReleaseAttemptReader {
    /// Read the complete immutable history, including starts whose results were never delivered.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, partial, unsupported or over-limit storage without repair.
    fn inspect_release_attempts(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<gazette_core::store::ReleaseAttemptInspection, Error>;
}

/// Observation of actual held coordinator custody, without acquiring effect capabilities.
pub trait ReleaseCoordinatorReader {
    /// Inspect the actual held coordinator guard; ordinary unanchored writers refuse.
    ///
    /// # Errors
    /// Refuses missing, changed or mismatched native custody.
    fn inspect_release_coordinator(
        &mut self,
    ) -> Result<gazette_core::store::ReleaseCoordinator, Error>;
}

/// One local release transaction under an independently anchored existing writer.
/// Native implementations revalidate custody and leases; supplied structs grant no authority.
pub trait ReleaseConsumptionWriter: ReleaseAuthorizationWriter + ReleaseCoordinatorReader {
    /// Inspect original consumed history without reauthorizing it.
    ///
    /// # Errors
    /// Refuses stale or corrupt native history and invalidates uncertain lifetimes.
    fn inspect_release_consumption(
        &mut self,
        record_id: &str,
    ) -> Result<gazette_core::store::ReleaseConsumptionInspection, Error>;

    /// Acquire the complete target lease set of an existing pending authorization.
    /// Same held set is idempotent; consumed or competing unresolved holders refuse.
    ///
    /// # Errors
    /// Refuses permission, missing custody, stale binding, partial targets or uncertain writes.
    fn acquire_release_leases(
        &mut self,
        record_id: &str,
        options: gazette_core::store::ReleaseLeaseOptions,
    ) -> Result<gazette_core::store::ReleaseLeases, gazette_core::store::ReleaseWriteError>;

    /// Compare-and-consume the exact pending record plus every target in one transaction.
    /// The selected database must be the held writer's database, not an equal-header copy.
    ///
    /// # Errors
    /// Refuses prior consumption, stale leases, changed inputs or uncertain writes.
    fn consume_release_authorization(
        &mut self,
        database: &Path,
        input: gazette_core::store::ReleaseConsumptionInput<'_>,
        options: gazette_core::store::ReleaseConsumptionOptions,
    ) -> Result<
        gazette_core::store::ReleaseConsumptionReceipt,
        gazette_core::store::ReleaseWriteError,
    >;
}

/// Historical consumption lookup independent of active authority and destination access.
pub trait ReleaseConsumptionReader {
    /// Read original lease history without renewing custody or repairing state.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, partial, unsupported or over-limit history.
    fn inspect_release_leases(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<gazette_core::store::ReleaseLeaseInspection, Error>;

    /// Read the exact original batch and all immutable targets without repair.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, partial, unsupported or over-limit history.
    fn inspect_release_consumption(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<gazette_core::store::ReleaseConsumptionInspection, Error>;
}

/// Immutable pending authorization registration under an existing native writer.
/// Storage enforces custody/linkage, not signature validity, leases or consumption.
pub trait ReleaseAuthorizationWriter: JournalWriter {
    /// Read original history without granting new effect authority.
    ///
    /// # Errors
    /// Refuses stale, corrupt, unsupported or over-limit state and invalidates the lifetime.
    fn inspect_release_authorization(
        &mut self,
        record_id: &str,
    ) -> Result<gazette_core::store::ReleaseAuthorizationInspection, Error>;

    /// Register all received evidence and ordered preparation links atomically.
    /// Exact replay preserves original coordinates; changed record IDs/nonces refuse.
    ///
    /// # Errors
    /// Refuses missing permission/prerequisites, changed evidence, stale generation or native failure.
    fn record_release_authorization(
        &mut self,
        database: &Path,
        input: gazette_core::store::ReleaseAuthorizationInput<'_>,
        options: gazette_core::store::ReleaseAuthorizationWriteOptions,
    ) -> Result<
        gazette_core::store::ReleaseAuthorizationReceipt,
        gazette_core::store::ReleaseAuthorizationWriteError,
    >;
}

/// Historical authorization custody lookup without current-time verification or mutation.
pub trait ReleaseAuthorizationReader {
    /// Read exact original registration in one verified native snapshot.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, unsupported or over-limit history without repairing it.
    fn inspect_release_authorization(
        &self,
        path: &Path,
        record_id: &str,
        options: ReadOptions,
    ) -> Result<gazette_core::store::ReleaseAuthorizationInspection, Error>;
}

/// Explicit trusted-local coordinator enrollment and historical anchor observation.
/// Neither operation authenticates a principal, acquires destination leases or consumes authority.
pub trait LocalCoordinatorStore {
    /// Enroll an empty private anchor against a fresh unwritten journal only.
    ///
    /// # Errors
    /// Refuses permission, unsafe/shared roots, prior state or bounds; preserves uncertain anchor writes.
    fn enroll_local_coordinator(
        &self,
        anchor_root: &Path,
        journal_root: &Path,
        coordinator_id: &str,
        principal_scope: Sha256Digest,
        options: SessionOptions,
        allow_anchor_write: bool,
    ) -> Result<gazette_core::store::LocalCoordinatorSeal, Error>;

    /// Inspect exactly selected anchor state without acquiring a journal writer or repairing it.
    /// In-progress is returned as historical evidence, never made clean by inspection.
    ///
    /// # Errors
    /// Refuses permission, unsafe/corrupt/bounded state or independently expected binding mismatch.
    fn inspect_local_coordinator(
        &self,
        anchor_root: &Path,
        expected: &gazette_core::store::LocalCoordinatorBinding,
        options: SessionOptions,
    ) -> Result<gazette_core::store::LocalCoordinatorState, Error>;
}

/// One exact verified restore adoption into a fresh successor root.
pub struct RestoreAdoptionRequest<'a> {
    /// Independently selected coordinator anchor.
    pub anchor_root: &'a Path,
    /// Exact clean coordinator binding expected at the anchor.
    pub expected: &'a gazette_core::store::LocalCoordinatorBinding,
    /// Independently retained latest clean frontier.
    pub previous: &'a gazette_core::store::LocalCoordinatorSeal,
    /// Exact detached restore root.
    pub restore_root: &'a Path,
    /// Independently retained restore-receipt identity.
    pub receipt_sha256: Sha256Digest,
    /// Fresh successor root; never an existing or fallback store.
    pub successor_root: &'a Path,
}

/// One exact verified restore adoption into a fresh successor root.
pub trait RestoreAdopter {
    /// Verify the retained clean anchor and detached restore, advance exactly one fence in a
    /// fresh successor root, and persist replayable anchor evidence. Inputs are never replaced.
    ///
    /// # Errors
    ///
    /// Refuses invalid evidence, unsafe paths, missing permission, stale custody, nonfresh
    /// successor state or any failed native verification or persistence effect.
    fn adopt_restore(
        &self,
        request: RestoreAdoptionRequest<'_>,
        options: gazette_core::store::RestoreAdoptionOptions,
    ) -> Result<gazette_core::store::RestoreAdoptionReceipt, Error>;
}

/// Immutable destination observations under an existing writer lifetime.
pub trait RefObservationWriter: JournalWriter {
    /// Inspect history under the held generation without sampling any destination.
    ///
    /// # Errors
    /// Refuses stale, invalid or corrupt state and invalidates the writer lifetime.
    fn inspect_ref_observation(
        &mut self,
        id: Sha256Digest,
    ) -> Result<gazette_core::store::RefObservationInspection, Error>;

    /// Append a complete native result, or replay byte-equivalent stored evidence.
    /// The adapter validates storage, not provider truth or target admission.
    ///
    /// # Errors
    /// Refuses missing permission, changed payload, incomplete rows, bounds or native failure.
    fn record_ref_observation(
        &mut self,
        input: gazette_core::store::RefObservationInput<'_>,
        options: gazette_core::store::RefObservationWriteOptions,
    ) -> Result<
        gazette_core::store::RefObservationReceipt,
        gazette_core::store::RefObservationWriteError,
    >;
}

/// Read historical native ref evidence without implying current destination state.
pub trait RefObservationReader {
    /// Read one immutable complete observation in a verified journal snapshot.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, unsupported or over-limit state without repair.
    fn inspect_ref_observation(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<gazette_core::store::RefObservationInspection, Error>;
}

/// Immutable staging intent and retention evidence under one existing writer lifetime.
pub trait PreparationWriter: JournalWriter {
    /// Append check evidence after exact retention and before a new Git recipe.
    /// Equal replay preserves original history; checks cannot be attached retroactively.
    ///
    /// # Errors
    /// Refuses missing/changed prerequisites, conflicting evidence, permission or native failure.
    fn record_preparation_check(
        &mut self,
        input: gazette_core::store::PreparationCheckInput<'_>,
        options: gazette_core::store::PreparationWriteOptions,
    ) -> Result<gazette_core::store::PreparationReceipt, gazette_core::store::PreparationWriteError>;

    /// Inspect exact preparation under this held writer generation without appending.
    /// A failed identity/schema/record read invalidates the writer lifetime.
    ///
    /// # Errors
    /// Refuses changed, corrupt, unsupported or over-limit state without repair.
    fn inspect_preparation(
        &mut self,
        id: Sha256Digest,
    ) -> Result<gazette_core::store::PreparationInspection, Error>;

    /// Record an exact Git recipe only after the same stage intent and retention.
    /// Replaying it never permits repeating an uncertain filesystem effect.
    ///
    /// # Errors
    /// Refuses permission, missing/changed prerequisites, recipe conflict or native failure.
    fn record_preparation_worktree(
        &mut self,
        input: gazette_core::store::PreparationWorktreeInput<'_>,
        options: gazette_core::store::PreparationWriteOptions,
    ) -> Result<gazette_core::store::PreparationReceipt, gazette_core::store::PreparationWriteError>;

    /// Record a complete native Git result against the exact stored preparation.
    /// Historical completion is not current Git or filesystem verification.
    ///
    /// # Errors
    /// Refuses missing/changed prerequisites, mismatched results, permission or native failure.
    fn record_preparation_commit(
        &mut self,
        input: gazette_core::store::PreparationCommitInput<'_>,
        options: gazette_core::store::PreparationWriteOptions,
    ) -> Result<gazette_core::store::PreparationReceipt, gazette_core::store::PreparationWriteError>;

    /// Append intent before stage creation, or return an exactly equal stored intent.
    ///
    /// # Errors
    /// Refuses missing permission, conflict, invalid input, stale lifetime or native failure.
    fn record_preparation_intent(
        &mut self,
        input: gazette_core::store::PreparationInput<'_>,
        options: gazette_core::store::PreparationWriteOptions,
    ) -> Result<gazette_core::store::PreparationReceipt, gazette_core::store::PreparationWriteError>;
    /// Append a native retention observation only for the exact existing intent.
    ///
    /// # Errors
    /// Refuses absent/changed intent, mismatched output, stale lifetime or native failure.
    fn record_preparation_retention(
        &mut self,
        input: gazette_core::store::PreparationRetentionInput<'_>,
        options: gazette_core::store::PreparationWriteOptions,
    ) -> Result<gazette_core::store::PreparationReceipt, gazette_core::store::PreparationWriteError>;
}

/// Historical preparation lookup; native stage freshness is a separate observation.
pub trait PreparationReader {
    /// Read exact history in one verified snapshot without migration or stage effects.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, unsupported or over-limit state without repair.
    fn inspect_preparation(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<gazette_core::store::PreparationInspection, Error>;
}

/// Immutable local commit history under the existing journal writer lifetime.
pub trait CheckpointWriter: JournalWriter {
    /// Append exact evidence or read back an identical existing record without replacement.
    ///
    /// # Errors
    /// Refuses missing permission, changed intent, stale lifetime, bounds or native failure.
    /// An attempted write without complete read-back invalidates the writer.
    fn record_checkpoint(
        &mut self,
        input: gazette_core::store::CheckpointInput<'_>,
        options: gazette_core::store::CheckpointWriteOptions,
    ) -> Result<gazette_core::store::CheckpointReceipt, gazette_core::store::CheckpointWriteError>;
}

/// Read historical checkpoint evidence without migrating or verifying current Git objects.
pub trait CheckpointReader {
    /// Look up one exact coordinate in a schema/integrity-checked native snapshot.
    ///
    /// # Errors
    /// Refuses unsafe, corrupt, unsupported or over-limit state; never repairs records.
    fn inspect_checkpoint(
        &self,
        path: &Path,
        id: Sha256Digest,
        options: ReadOptions,
    ) -> Result<gazette_core::store::CheckpointInspection, Error>;
}

/// Complete-set private projection writes under an already acquired journal lifetime.
pub trait RegistryWriter: JournalWriter {
    /// Replace every registry-owned data row and head atomically, never journal history.
    /// Verify desired/native rows in the transaction and through independent read-back.
    ///
    /// # Errors
    /// Refuses missing effect permission, invalid input, stale lifetime or native failure.
    /// Failed mutation conservatively reports uncertainty and invalidates the lifetime.
    fn replace_registry(
        &mut self,
        input: gazette_core::store::RegistryInput<'_>,
        options: gazette_core::store::RegistryWriteOptions,
    ) -> Result<gazette_core::store::RegistryReceipt, gazette_core::store::RegistryWriteError>;
}

/// Read the complete native registry identity without migration or reconciliation.
pub trait RegistryReader {
    /// Verify schema, journal, foreign keys and complete row identity in one snapshot.
    ///
    /// # Errors
    /// Refuses unsupported, corrupt, mismatched or over-limit data without partial evidence.
    fn inspect_registry(
        &self,
        path: &Path,
        options: ReadOptions,
    ) -> Result<gazette_core::store::RegistryInspection, Error>;
}

/// Rebuildable publication-cache writes under an already acquired local writer lifetime.
pub trait BuildCacheWriter: JournalWriter {
    /// Atomically replace the complete cached site set, including stale-row removal.
    ///
    /// # Errors
    /// Refuses incomplete dependencies, invalid artifacts, missing permission or failed read-back.
    fn replace_build_cache(
        &mut self,
        input: gazette_core::store::BuildCacheInput<'_>,
        options: gazette_core::store::BuildCacheWriteOptions,
    ) -> Result<gazette_core::store::BuildCacheReceipt, Error>;
}

/// Read and verify rebuildable publication artifacts without mutating or migrating the store.
pub trait BuildCacheReader {
    /// Load a site only when its complete typed dependency set matches exactly.
    ///
    /// # Errors
    /// Refuses corrupt, unsupported or over-limit storage; a clean miss returns `None`.
    fn load_build_cache_site(
        &self,
        path: &Path,
        site: &gazette_core::publication::Key,
        dependencies: &gazette_core::store::BuildCacheDependencySet,
        options: ReadOptions,
    ) -> Result<Option<gazette_core::store::CachedBuildSite>, Error>;

    /// Inspect complete replaceable-cache identity without loading artifact payloads.
    ///
    /// # Errors
    /// Refuses corrupt, unsupported or over-limit storage.
    fn inspect_build_cache(
        &self,
        path: &Path,
        options: ReadOptions,
    ) -> Result<gazette_core::store::BuildCacheInspection, Error>;
}

/// Local lifetime-held generation, not a cross-host or publisher capability.
pub trait JournalWriter {
    /// Inspect a fresh snapshot against the exact acquired generation and native identities.
    ///
    /// # Errors
    /// Any mismatch or failed inspection permanently invalidates this lifetime.
    fn inspect(&mut self) -> Result<JournalInspection, Error>;

    /// Revalidate and release local coordination without claiming release settlement.
    ///
    /// # Errors
    /// Reports invalidated generation, changed paths or failed native read-back.
    fn close(self) -> Result<(), Error>;
}

/// Acquire a local writer by atomically advancing complete expected journal coordinates.
pub trait WriterConnector {
    /// Native lifetime ownership; serialization of its header cannot recreate it.
    type Writer: JournalWriter;

    /// Reserve an existing private journal namespace and verify committed advancement.
    ///
    /// # Errors
    /// Refuses permission, identity, schema, contention and resource failures. Failed
    /// acquisition reports whether generation mutation was attempted, never auto-retries.
    fn acquire_writer(
        &self,
        root: &Path,
        expected: JournalHeader,
        options: WriterOptions,
    ) -> Result<Self::Writer, WriterAcquireError>;
}

/// Restricted recovery of interrupted custody under an explicit native lifetime profile.
pub trait ReleaseRecoveryConnector {
    /// Held observation/reconciliation only; no generation, consumption or attempt capability.
    type Reconciler: JournalWriter + ReleaseReconciliationWriter + ReleaseRecoverySealer;

    /// Verify retained transaction continuity and the selected profile across the entire journal.
    /// Releasing the returned lifetime must retain interrupted state, never seal it clean.
    ///
    /// # Errors
    /// Refuses missing permission, uncertain continuity, unsupported starts or competing custody.
    fn recover_release(
        &self,
        root: &Path,
        expected: JournalHeader,
        profile: gazette_core::store::RecoveryProfile,
        options: SessionOptions,
    ) -> Result<Self::Reconciler, Error>;
}

/// Explicit completion of supported interrupted custody; never conversion to a writer.
pub trait ReleaseRecoverySealer: JournalWriter {
    /// Consume custody and seal only exact, child-free, wholly terminal release history.
    /// Ordinary close/drop must remain non-sealing. No generation or release effect follows.
    ///
    /// # Errors
    /// Refuses denied permission, incomplete history or uncertain continuity. A failed
    /// seal may have persisted; retain coordinates and independently inspect the anchor.
    fn seal_recovery(
        self,
        allow_seal: bool,
    ) -> Result<gazette_core::store::LocalCoordinatorSeal, Error>;
}

/// Read an existing journal under the adapter's exact supported schema and integrity checks.
pub trait JournalReader {
    /// Return one complete native snapshot, never a held writer or restored-history approval.
    ///
    /// # Errors
    /// Refuses missing permission, unsafe input, unsupported/partial schema, invalid
    /// coordinates, failed integrity, native recovery requirements or exhausted bounds.
    fn inspect_journal(
        &self,
        path: &Path,
        options: ReadOptions,
    ) -> Result<JournalInspection, Error>;
}

/// Initialize a fresh local journal without adopting or migrating an existing store.
pub trait StoreInitializer {
    /// Reserve an empty private directory and create, commit, read back and flush its journal.
    ///
    /// # Errors
    /// Refuses missing permission, existing output, contention, bounds or native failure.
    /// Any partially created database is retained with its exact location.
    fn initialize(
        &self,
        root: &Path,
        options: InitializeOptions,
    ) -> Result<InitializationReceipt, OutputError>;
}

/// A native connection whose local coordination reservation lasts until close or drop.
pub trait StoreSession {
    /// Observe one fresh read snapshot after revalidating native path ownership.
    ///
    /// # Errors
    /// Refuses changed native identities, resource exhaustion or failed provider reads.
    fn observe(&mut self) -> Result<Observation, Error>;

    /// Close the database before releasing its local reservation.
    ///
    /// # Errors
    /// Reports changed native identity or failed connection close; never deletes lock state.
    fn close(self) -> Result<(), Error>;
}

/// Connect to an existing database under an explicit local coordination namespace.
pub trait StoreConnector {
    /// Lifetime-owned connection and reservation, never a serializable write capability.
    type Session: StoreSession;

    /// Reserve a private existing directory, then open the selected store read-only.
    ///
    /// # Errors
    /// Refuses missing permission, contention, unsafe paths or native connection failure.
    fn connect(
        &self,
        path: &Path,
        coordination_root: &Path,
        options: SessionOptions,
    ) -> Result<Self::Session, Error>;
}

/// Inspect an existing selected database without creating, migrating or writing SQL data.
///
/// The implementation must enforce explicit coordination-file permission and
/// resource limits before connecting. Provider bookkeeping effects are allowed
/// only by that permission; they never authorize checkpoint/recovery/data writes.
pub trait StoreReader {
    /// Return one complete observation, not ownership, integrity or schema admission.
    ///
    /// # Errors
    ///
    /// Refuses missing permission, unsafe/missing/invalid stores, exhausted limits
    /// and failed native observations. Never substitutes an in-memory or new store.
    fn observe(&self, path: &Path, options: ReadOptions) -> Result<Observation, Error>;
}

/// Consistent backup into new private output, never a source write or restore.
pub trait StoreBackup {
    /// Copy an existing store into a fresh child of an existing trusted backup parent.
    ///
    /// # Errors
    /// Refuses unacknowledged effects, invalid sources/parents, exhausted limits
    /// and failed copy/read-back. Any created output is reported, not silently reused.
    fn backup(
        &self,
        source: &Path,
        parent: &Path,
        options: BackupOptions,
    ) -> Result<BackupReceipt, OutputError>;
}

/// Inspect an existing private backup against independently retained manifest bytes.
pub trait BackupReader {
    /// Verify exact inventory, manifest pin, copied bytes and native database integrity.
    ///
    /// # Errors
    /// Refuses unsafe/missing/changed input, unsupported manifests, bounds and native
    /// read-back failure. Never creates, repairs, restores, adopts or deletes a backup.
    fn verify_backup(
        &self,
        directory: &Path,
        manifest_sha256: Sha256Digest,
        options: BackupVerifyOptions,
    ) -> Result<BackupInspection, Error>;
}

/// Prepare an isolated recovery copy without replacing or activating existing state.
pub trait RestorePreparer {
    /// Reverify pinned backup evidence and copy exact bytes to a fresh private child.
    ///
    /// # Errors
    /// Refuses unacknowledged effects, invalid evidence/paths, changed input, limits
    /// or failed read-back/flush. Any created output is retained and identified.
    fn prepare_restore(
        &self,
        backup: &Path,
        manifest_sha256: Sha256Digest,
        parent: &Path,
        options: RestoreOptions,
    ) -> Result<RestoreReceipt, OutputError>;
}
