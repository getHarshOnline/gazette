//! Explicit store operations; fresh initialization is separate from observation or recovery.

use gazette_core::source::Sha256Digest;
use gazette_core::store::{
    BackupInspection, BackupOptions, BackupReceipt, BackupVerifyOptions, Error,
    InitializationReceipt, InitializeOptions, JournalHeader, JournalInspection, Observation,
    OutputError, ReadOptions, RestoreOptions, RestoreReceipt, SessionOptions, WriterAcquireError,
    WriterOptions,
};
use gazette_ports::destination::RootSeparation;
use gazette_ports::store::{
    BackupReader, BuildCacheReader, JournalReader, JournalWriter, RestorePreparer, StoreBackup,
    StoreConnector, StoreInitializer, StoreReader, StoreSession, WriterConnector,
};
use std::path::{Path, PathBuf};

pub mod coordinator;
mod materialization;
mod reviewed;
pub use materialization::inspect_materialization;

/// Complete caller-declared Website roots that private store output must never overlap.
#[derive(Debug)]
pub struct LifecycleProtectedRoots {
    /// Authored source roots.
    pub authored: Vec<PathBuf>,
    /// Ephemeral or retained staging roots.
    pub staged: Vec<PathBuf>,
    /// Published/generated output roots.
    pub published: Vec<PathBuf>,
    /// Selected live release-journal roots.
    pub live_journals: Vec<PathBuf>,
}

impl LifecycleProtectedRoots {
    fn collect(&self) -> Result<Vec<PathBuf>, Error> {
        if self.authored.is_empty()
            || self.staged.is_empty()
            || self.published.is_empty()
            || self.live_journals.is_empty()
        {
            return Err(Error::UnsafePath);
        }
        Ok(self
            .authored
            .iter()
            .chain(&self.staged)
            .chain(&self.published)
            .chain(&self.live_journals)
            .cloned()
            .collect())
    }
}

/// One private backup request with its complete Website lifecycle placement boundary.
#[derive(Debug)]
pub struct BackupRequest<'a> {
    /// Existing selected `SQLite` database.
    pub source: &'a Path,
    /// Existing private parent for one fresh backup directory.
    pub output_parent: &'a Path,
    /// Complete explicitly selected roots that must remain disjoint from output.
    pub protected: LifecycleProtectedRoots,
    /// Explicit read, creation and output bounds.
    pub options: BackupOptions,
}

/// One detached restore-preparation request with its complete placement boundary.
#[derive(Debug)]
pub struct PrepareRestoreRequest<'a> {
    /// Independently pinned backup directory.
    pub backup: &'a Path,
    /// Independently retained backup manifest identity.
    pub manifest_sha256: Sha256Digest,
    /// Existing private parent for one fresh detached recovery directory.
    pub output_parent: &'a Path,
    /// Complete explicitly selected roots that must remain disjoint from output.
    pub protected: LifecycleProtectedRoots,
    /// Explicit verification, preparation and output bounds.
    pub options: RestoreOptions,
}

fn check_output_placement(
    filesystem: &impl RootSeparation,
    parent: &Path,
    protected: &LifecycleProtectedRoots,
) -> Result<(), OutputError> {
    let roots = protected.collect()?;
    filesystem
        .check_root_separation(parent, &roots)
        .map_err(|_| Error::UnsafePath.into())
}

/// Inspect complete immutable reconciliation history without destination reads or renewal.
///
/// # Errors
/// Refuses invalid, corrupt or over-limit history and independently expected header mismatch.
pub fn inspect_release_reconciliations(
    reader: &impl gazette_ports::store::ReleaseReconciliationReader,
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::ReleaseReconciliationInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_release_reconciliations(path, record_id, options)?;
    if expected.is_some_and(|header| *header != inspected.history.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

/// Inspect immutable attempts without current authority, destination access or repair.
///
/// # Errors
/// Refuses invalid/corrupt/over-limit history or independently expected header mismatch.
pub fn inspect_release_attempts(
    reader: &impl gazette_ports::store::ReleaseAttemptReader,
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::ReleaseAttemptInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_release_attempts(path, record_id, options)?;
    if expected.is_some_and(|header| *header != inspected.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

/// Inspect original lease evidence without renewing current custody.
///
/// # Errors
/// Refuses corrupt history, exceeded bounds or independently expected header mismatch.
pub fn inspect_release_leases(
    reader: &impl gazette_ports::store::ReleaseConsumptionReader,
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::ReleaseLeaseInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_release_leases(path, record_id, options)?;
    if expected.is_some_and(|header| *header != inspected.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

/// Inspect original consumed history without current authority or provider access.
///
/// # Errors
/// Refuses corrupt/partial native history, bounds or independently expected header mismatch.
pub fn inspect_release_consumption(
    reader: &impl gazette_ports::store::ReleaseConsumptionReader,
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::ReleaseConsumptionInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_release_consumption(path, record_id, options)?;
    if expected.is_some_and(|header| *header != inspected.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

/// Inspect original pending authorization custody without reacquiring signatures or consuming it.
///
/// # Errors
/// Refuses native corruption/bounds or independently expected header mismatch; never migrates.
pub fn inspect_release_authorization(
    reader: &impl gazette_ports::store::ReleaseAuthorizationReader,
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::ReleaseAuthorizationInspection, Error> {
    options.validate()?;
    let inspected = reader.inspect_release_authorization(path, record_id, options)?;
    if expected.is_some_and(|header| *header != inspected.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspected)
}

/// Inspect local preparation history without touching its selected stage.
///
/// # Errors
/// Preserves schema/integrity/bounds failures and refuses expected-header mismatch.
pub fn inspect_preparation(
    reader: &impl gazette_ports::store::PreparationReader,
    path: &Path,
    id: Sha256Digest,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::PreparationInspection, Error> {
    options.validate()?;
    let inspection = reader.inspect_preparation(path, id, options)?;
    if expected.is_some_and(|header| *header != inspection.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspection)
}

/// Inspect verified registry rows and optionally compare an independently retained journal header.
///
/// # Errors
/// Preserves native schema/row/bound failures and refuses any expected-header mismatch.
pub fn inspect_registry(
    reader: &impl gazette_ports::store::RegistryReader,
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::RegistryInspection, Error> {
    options.validate()?;
    let inspection = reader.inspect_registry(path, options)?;
    if expected.is_some_and(|header| *header != inspection.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspection)
}

/// Inspect verified rebuildable cache rows and optionally compare a retained journal header.
///
/// # Errors
/// Preserves native schema/row/bound failures and refuses expected-header mismatch.
pub fn inspect_build_cache(
    reader: &impl BuildCacheReader,
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::BuildCacheInspection, Error> {
    options.validate()?;
    let inspection = reader.inspect_build_cache(path, options)?;
    if expected.is_some_and(|header| *header != inspection.journal.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspection)
}

/// Application-owned local generation lifetime over the admitted native capability.
pub struct Writer<W: JournalWriter>(W);

impl<W: gazette_ports::store::ReleaseRecoverySealer> Writer<W> {
    /// Explicitly seal terminal restricted recovery, without acquiring another generation.
    ///
    /// # Errors
    /// Preserves denied permission, unsupported history and uncertain native sealing.
    pub fn seal_recovery(
        self,
        allow_seal: bool,
    ) -> Result<gazette_core::store::LocalCoordinatorSeal, Error> {
        self.0.seal_recovery(allow_seal)
    }
}

impl<W: JournalWriter + gazette_ports::store::ReleaseReconciliationWriter> Writer<W> {
    /// Reconcile the complete original release under held custody without new dispatch authority.
    ///
    /// # Errors
    /// Preserves acquired observations and uncertain durable delivery; never silently retries.
    pub fn reconcile_release(
        &mut self,
        reader: &impl gazette_ports::release::RemoteReader,
        request: crate::release::ReconcileReleaseRequest<'_>,
        options: crate::release::ReconcileReleaseOptions,
    ) -> Result<
        gazette_core::store::ReleaseReconciliationReceipt,
        crate::release::ReconcileReleaseError,
    > {
        crate::release::reconcile_release(&mut self.0, reader, request, options)
    }
}

impl<
    W: gazette_ports::store::ReleaseReconciliationWriter
        + gazette_ports::store::ReleaseAttemptWriter
        + gazette_ports::store::PreparationWriter,
> Writer<W>
{
    /// Dispatch one exact consumed target through durable start and independent native read-back.
    /// A returned attempt is not complete-set settlement; replay never invokes the provider.
    ///
    /// # Errors
    /// Preserves exact known start/transport boundaries and refuses altered or unresolved intent.
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_release_attempt<F, G, S, A, R, D>(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        authority: &mut A,
        reader: &R,
        dispatcher: &D,
        request: crate::release::DispatchAttemptRequest<'_>,
        options: crate::release::DispatchAttemptOptions,
    ) -> Result<gazette_core::store::ReleaseAttempt, crate::release::DispatchAttemptError>
    where
        F: gazette_ports::destination::RootSeparation + gazette_ports::release::WorktreeReader,
        G: gazette_ports::release::ReleaseWorktrees,
        S: gazette_ports::store::PreparationReader,
        A: gazette_ports::release::AuthoritySource,
        R: gazette_ports::release::RemoteReader,
        D: gazette_ports::release::ReleaseDispatcher,
    {
        crate::release::dispatch_prepared_attempt(
            filesystem,
            git,
            store,
            &mut self.0,
            authority,
            reader,
            dispatcher,
            request,
            options,
        )
    }
}

impl<W: gazette_ports::store::ReleaseConsumptionWriter> Writer<W> {
    /// Consume one exact pending authorization using current independently acquired bindings.
    /// Native leases and complete pre-state reads precede one atomic local transition.
    ///
    /// # Errors
    /// Preserves complete prior lease/read evidence and any uncertain native write boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn consume_release_authorization<F, G, S, A, R>(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        authority: &mut A,
        reader: &R,
        request: crate::release::ConsumeAuthorizationRequest<'_>,
        options: crate::release::ConsumeAuthorizationOptions,
    ) -> Result<
        gazette_core::store::ReleaseConsumptionReceipt,
        crate::release::ConsumeAuthorizationError,
    >
    where
        F: gazette_ports::destination::RootSeparation + gazette_ports::release::WorktreeReader,
        G: gazette_ports::release::ReleaseWorktrees,
        S: gazette_ports::store::PreparationReader,
        A: gazette_ports::release::AuthoritySource,
        R: gazette_ports::release::RemoteReader,
    {
        crate::release::consume_prepared_authorization(
            filesystem,
            git,
            store,
            &mut self.0,
            authority,
            reader,
            request,
            options,
        )
    }
}

impl<W: gazette_ports::store::ReleaseAuthorizationWriter> Writer<W> {
    /// Register an exact verified pending authorization under this already-held generation.
    /// The shared verifier/native join runs before an immutable append; no consume or dispatch.
    ///
    /// # Errors
    /// Preserves binding, native preparation, conflict and uncertain-write failures.
    pub fn register_release_authorization<F, G, S>(
        &mut self,
        filesystem: &F,
        git: &G,
        store: &S,
        request: crate::release::PreparedAuthorizationRequest<'_>,
        options: crate::release::RegisterAuthorizationOptions,
    ) -> Result<
        gazette_core::store::ReleaseAuthorizationReceipt,
        crate::release::RegisterAuthorizationError,
    >
    where
        F: gazette_ports::destination::RootSeparation + gazette_ports::release::WorktreeReader,
        G: gazette_ports::release::ReleaseWorktrees,
        S: gazette_ports::store::PreparationReader,
    {
        crate::release::register_prepared_authorization(
            filesystem,
            git,
            store,
            &mut self.0,
            request,
            options,
        )
    }
}

impl<W: gazette_ports::store::RefObservationWriter> Writer<W> {
    /// Read native destinations and retain their complete result under this held
    /// generation. Existing coordinates refuse before any new provider read.
    ///
    /// # Errors
    /// Preserves policy, native observation and uncertain local append evidence.
    pub fn record_target_observation(
        &mut self,
        reader: &impl gazette_ports::release::RemoteReader,
        targets: &gazette_compiler::release::TargetSet,
        id: Sha256Digest,
        options: crate::release::RecordObservationOptions,
    ) -> Result<gazette_core::store::RefObservationReceipt, crate::release::RecordObservationError>
    {
        crate::release::record_target_observation(reader, &mut self.0, targets, id, options)
    }
}

impl<W: JournalWriter> Writer<W> {
    /// Revalidate acquired coordinates and return a fresh complete snapshot.
    ///
    /// # Errors
    /// Preserves native failures and permanent lifetime invalidation.
    pub fn inspect(&mut self) -> Result<JournalInspection, Error> {
        self.0.inspect()
    }

    /// Validate before ending the local lifetime; no release settlement is implied.
    ///
    /// # Errors
    /// Preserves native close/identity/read-back failures.
    pub fn close(self) -> Result<(), Error> {
        self.0.close()
    }
}

/// Advance exactly the expected local generation under exclusive coordination.
///
/// # Errors
/// Checks explicit permissions and preserves native refusal and uncertain-effect evidence.
pub fn acquire_writer<C: WriterConnector>(
    connector: &C,
    root: &Path,
    expected: JournalHeader,
    options: WriterOptions,
) -> Result<Writer<C::Writer>, WriterAcquireError> {
    options.validate()?;
    connector
        .acquire_writer(root, expected, options)
        .map(Writer)
}

/// Recover observation-only custody under the selected native release lifetime profile.
/// Closing this lifetime releases custody without clearing its interrupted state.
///
/// # Errors
/// Preserves native continuity, permission, namespace and prior-dispatch refusals.
pub fn recover_release<C: gazette_ports::store::ReleaseRecoveryConnector>(
    connector: &C,
    root: &Path,
    expected: JournalHeader,
    profile: gazette_core::store::RecoveryProfile,
    options: SessionOptions,
) -> Result<Writer<C::Reconciler>, Error> {
    options.validate()?;
    connector
        .recover_release(root, expected, profile, options)
        .map(Writer)
}

/// Inspect a journal and, when supplied, compare every expected coordinate without writes.
///
/// # Errors
/// Preserves native schema/integrity refusals and rejects any expected-header mismatch.
pub fn inspect_journal(
    reader: &impl JournalReader,
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<JournalInspection, Error> {
    options.validate()?;
    let inspection = reader.inspect_journal(path, options)?;
    if expected.is_some_and(|header| *header != inspection.header) {
        return Err(Error::JournalMismatch);
    }
    Ok(inspection)
}

/// Initialize fresh journal storage; no existing store or publication source is selected.
///
/// # Errors
/// Validates explicit permissions and preserves native refusal or partial-output evidence.
pub fn initialize(
    provider: &impl StoreInitializer,
    root: &Path,
    options: InitializeOptions,
) -> Result<InitializationReceipt, OutputError> {
    options.validate()?;
    provider.initialize(root, options)
}

/// Lifetime-bound application session over its admitted native capability.
pub struct Session<S: StoreSession>(S);

impl<S: StoreSession> Session<S> {
    /// Revalidate local coordination and inspect one fresh store snapshot.
    ///
    /// # Errors
    /// Preserves native identity, provider and resource failures.
    pub fn observe(&mut self) -> Result<Observation, Error> {
        self.0.observe()
    }

    /// Close before releasing the local reservation; never infer durable settlement.
    ///
    /// # Errors
    /// Preserves failed native identity/connection-close evidence.
    pub fn close(self) -> Result<(), Error> {
        self.0.close()
    }
}

/// Open an explicitly reserved local session without adopting a schema or write authority.
///
/// # Errors
/// Requires both reservation and connection permissions and preserves native refusals.
pub fn connect<C: StoreConnector>(
    connector: &C,
    path: &Path,
    coordination_root: &Path,
    options: SessionOptions,
) -> Result<Session<C::Session>, Error> {
    options.validate()?;
    connector
        .connect(path, coordination_root, options)
        .map(Session)
}

/// Observe a selected store through its native reader; no automatic adoption occurs.
///
/// # Errors
///
/// Requires explicit coordination effects and propagates the reader's refusal.
pub fn observe(
    reader: &impl StoreReader,
    path: &Path,
    options: ReadOptions,
) -> Result<Observation, Error> {
    options.validate()?;
    reader.observe(path, options)
}

/// Produce a fresh provider backup without modifying or adopting the selected store.
///
/// # Errors
/// Validates explicit effects; preserves native copy/read-back failure and residue evidence.
pub fn backup(
    provider: &impl StoreBackup,
    filesystem: &impl RootSeparation,
    request: BackupRequest<'_>,
) -> Result<BackupReceipt, OutputError> {
    request.options.validate()?;
    check_output_placement(filesystem, request.output_parent, &request.protected)?;
    provider.backup(request.source, request.output_parent, request.options)
}

/// Inspect retained backup bytes without conferring restoration or adoption authority.
///
/// # Errors
/// Requires explicit read policy and propagates manifest/native verification failure.
pub fn verify_backup(
    reader: &impl BackupReader,
    directory: &Path,
    manifest_sha256: Sha256Digest,
    options: BackupVerifyOptions,
) -> Result<BackupInspection, Error> {
    options.validate()?;
    reader.verify_backup(directory, manifest_sha256, options)
}

/// Prepare detached recovery bytes; replacing a live store is never an implicit next step.
///
/// # Errors
/// Requires explicit preparation effects and returns native validation/residue evidence.
pub fn prepare_restore(
    provider: &impl RestorePreparer,
    filesystem: &impl RootSeparation,
    request: PrepareRestoreRequest<'_>,
) -> Result<RestoreReceipt, OutputError> {
    request.options.validate()?;
    check_output_placement(filesystem, request.output_parent, &request.protected)?;
    provider.prepare_restore(
        request.backup,
        request.manifest_sha256,
        request.output_parent,
        request.options,
    )
}
