//! Explicit store initialization, observation and recovery through the selected `SQLite` adapter.

use gazette_adapter_sqlite::Sqlite;
use std::path::Path;

mod checkout;
mod checkout_set;
pub mod coordinator;
pub mod output;
mod reviewed;
pub use checkout::{CheckoutMaterialization, CheckoutMaterializationError};
pub use checkout_set::{
    CheckoutHistory, CheckoutSelection, CheckoutSetError, parse_output_checkouts,
};

pub use gazette_application::store::{
    BackupRequest, LifecycleProtectedRoots, PrepareRestoreRequest,
};
pub use gazette_core::store::{
    MaterializationCompletion, MaterializationEffect, MaterializationFrontier,
    MaterializationInspection, MaterializationIntent, MaterializationReceipt,
    MaterializationRecord, MaterializationWriteError, MaterializationWriteOptions,
};
pub use gazette_core::store::{
    ReleaseAttemptReference, ReleaseReconciliation, ReleaseReconciliationFrontier,
    ReleaseReconciliationInspection, ReleaseReconciliationMode, ReleaseReconciliationOptions,
    ReleaseReconciliationReceipt, ReleaseReconciliationRecord, ReleaseSettlement,
    ReleaseSettlementState, ReleaseTargetOutcome, ReleaseTargetSettlement,
};

/// Read complete native reconciliation history without resampling destinations or renewing effects.
///
/// # Errors
/// Refuses unsupported/corrupt/over-limit state and independently expected header mismatch.
pub fn inspect_release_reconciliations(
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<ReleaseReconciliationInspection, Error> {
    gazette_application::store::inspect_release_reconciliations(
        &Sqlite, path, record_id, expected, options,
    )
}

pub use gazette_core::store::{
    ReleaseAttempt, ReleaseAttemptInspection, ReleaseAttemptKey, ReleaseAttemptOptions,
    ReleaseAttemptReceipt, ReleaseAttemptResult, ReleaseAttemptStart,
};

/// Inspect immutable attempt history without current authority or provider access.
///
/// # Errors
/// Refuses unsafe, unsupported, corrupt or over-limit state; never repairs or redispatches.
pub fn inspect_release_attempts(
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<ReleaseAttemptInspection, Error> {
    gazette_application::store::inspect_release_attempts(
        &Sqlite, path, record_id, expected, options,
    )
}

pub use gazette_core::store::{
    ReleaseBatchSchema, ReleaseConsumption, ReleaseConsumptionInspection,
    ReleaseConsumptionOptions, ReleaseConsumptionReceipt, ReleaseCoordinator, ReleaseLease,
    ReleaseLeaseInspection, ReleaseLeaseOptions, ReleaseLeases, ReleaseTargetIntent,
    ReleaseWriteEffect,
};

/// Inspect original lease evidence without renewing custody or repairing state.
///
/// # Errors
/// Refuses unsafe, corrupt, partial or over-limit native history and header mismatch.
pub fn inspect_release_leases(
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<ReleaseLeaseInspection, Error> {
    gazette_application::store::inspect_release_leases(&Sqlite, path, record_id, expected, options)
}

/// Read exact original consumed intent without current signature or destination access.
///
/// # Errors
/// Refuses corrupt, partial, unsupported or over-limit native state; never repairs or reactivates.
pub fn inspect_release_consumption(
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<ReleaseConsumptionInspection, Error> {
    gazette_application::store::inspect_release_consumption(
        &Sqlite, path, record_id, expected, options,
    )
}

pub use gazette_core::store::{
    ReleaseAuthorizationEffect, ReleaseAuthorizationInspection, ReleaseAuthorizationReceipt,
    ReleaseAuthorizationRecord, ReleaseAuthorizationWriteOptions,
};

/// Read original pending authorization history, including after its approval expires.
/// Historical custody is not renewed authority, consumption or permission to dispatch.
///
/// # Errors
/// Refuses unsafe/unsupported/corrupt state, native bounds or expected-header mismatch.
pub fn inspect_release_authorization(
    path: &Path,
    record_id: &str,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<ReleaseAuthorizationInspection, Error> {
    gazette_application::store::inspect_release_authorization(
        &Sqlite, path, record_id, expected, options,
    )
}

use gazette_core::source::Sha256Digest;
pub use gazette_core::store::{
    BackupInspection, BackupOptions, BackupReceipt, BackupSnapshot, BackupVerifyOptions, Error,
    GenerationEffect, InitializationReceipt, InitializeOptions, JournalHeader, JournalInspection,
    Observation, OutputError, OutputLimits, ReadOptions, RestoreOptions, RestorePreparation,
    RestoreReceipt, SessionOptions, WriterAcquireError, WriterOptions,
};
pub use gazette_core::store::{
    BuildCacheHead, BuildCacheInspection, BuildCacheReceipt, BuildCacheWriteOptions,
    RegistryEffect, RegistryHead, RegistryInspection, RegistryReceipt, RegistryWriteError,
    RegistryWriteOptions,
};
pub use gazette_core::store::{
    PreparationCheck, PreparationEffect, PreparationInspection, PreparationIntent,
    PreparationReceipt, PreparationRecord, PreparationRetention, PreparationWriteError,
    PreparationWriteOptions,
};

/// Inspect the complete rebuildable cache identity without loading artifact payloads.
///
/// # Errors
/// Refuses unsupported/corrupt storage, native bounds and expected-header mismatch.
pub fn inspect_build_cache(
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<BuildCacheInspection, Error> {
    gazette_application::store::inspect_build_cache(&Sqlite, path, expected, options)
}

/// Read immutable preparation history without inspecting or changing its stage.
///
/// # Errors
/// Refuses unsupported/corrupt state or expected-header mismatch; never migrates or retries.
pub fn inspect_preparation(
    path: &Path,
    id: Sha256Digest,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<PreparationInspection, Error> {
    gazette_application::store::inspect_preparation(&Sqlite, path, id, expected, options)
}

/// Inspect complete private registry row identity without reconciliation or migration.
///
/// # Errors
/// Refuses unsafe/unsupported/corrupt rows, native bounds or expected journal mismatch.
pub fn inspect_registry(
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<RegistryInspection, Error> {
    gazette_application::store::inspect_registry(&Sqlite, path, expected, options)
}

/// Read completed or pending configured-output history without a writer or destination access.
/// This never reconciles effects, renews ownership or silently repairs an interrupted operation.
///
/// # Errors
/// Refuses unsafe, corrupt or over-limit storage and independently expected header mismatch.
pub fn inspect_materialization(
    path: &Path,
    root: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<gazette_core::store::MaterializationInspection, Error> {
    gazette_application::store::inspect_materialization(&Sqlite, path, root, expected, options)
}

/// Local journal-generation lifetime; no raw SQL, clone or serialized write capability.
pub struct Writer(gazette_application::store::Writer<gazette_adapter_sqlite::Writer>);

impl Writer {
    /// Plan the retained brand-owned documentation target under this same held journal.
    /// Relative coordinates require retained native workspace acquisition; no cwd fallback occurs.
    ///
    /// # Errors
    /// Refuses unsafe destinations, changed ownership, pending history or bounded evidence failure.
    pub fn plan_documentation_materialization<'a>(
        &mut self,
        assembly: &'a crate::build::DocumentationAssembly<'_>,
        protected: &[std::path::PathBuf],
        limits: crate::source::Limits,
        evidence_bytes: usize,
    ) -> Result<crate::build::MaterializationPlan<'a>, crate::build::MaterializationError> {
        self.0.plan_documentation_materialization(
            assembly,
            &gazette_adapter_fs::FileSystem,
            protected,
            limits,
            evidence_bytes,
        )
    }
    /// Plan the complete configured output set under this already-held journal.
    /// No partial set or imported ownership receipt grants a write capability.
    ///
    /// # Errors
    /// Refuses incomplete rendering, overlapping outputs, pending history or aggregate bounds.
    pub fn plan_materializations<'a>(
        &mut self,
        rendered: &'a crate::build::RenderedPublicationSet<'_>,
        limits: crate::source::Limits,
        evidence_bytes: usize,
    ) -> Result<crate::build::MaterializationSetPlan<'a>, crate::build::MaterializationSetPlanError>
    {
        self.0.plan_materializations(
            rendered,
            &gazette_adapter_fs::FileSystem,
            limits,
            evidence_bytes,
        )
    }

    /// Materialize original retained plans, preserving exact complete-set outcomes.
    ///
    /// # Errors
    /// Refuses permissions/IDs before effects; later failures retain completed and uncertain work.
    pub fn materialize_set(
        &mut self,
        plan: crate::build::MaterializationSetPlan<'_>,
        ids: &std::collections::BTreeMap<crate::workspace::Key, Sha256Digest>,
        options: crate::build::MaterializationOptions,
    ) -> Result<
        std::collections::BTreeMap<crate::workspace::Key, crate::build::MaterializationResult>,
        crate::build::MaterializationSetWriteError,
    > {
        self.0
            .materialize_set(plan, &gazette_adapter_fs::FileSystem, ids, options)
    }

    /// Inspect configured-output history without reading, rewriting or adopting its destination.
    ///
    /// # Errors
    /// Refuses stale, unsupported, corrupt or over-limit history.
    pub fn inspect_materialization(
        &mut self,
        root: &Path,
    ) -> Result<gazette_core::store::MaterializationInspection, Error> {
        self.0.inspect_materialization(root)
    }

    /// Plan a configured local output using actual completed journal ownership.
    /// No caller receipt, destination marker or copied matching bytes grants ownership.
    /// The held native journal roots and explicitly supplied peer destinations are protected.
    ///
    /// # Errors
    /// Refuses unsafe destinations, changed owned bytes, pending history or exhausted limits.
    pub fn plan_materialization<'a>(
        &mut self,
        rendered: &'a crate::build::PublicationRender<'_>,
        protected: &[std::path::PathBuf],
        limits: crate::source::Limits,
        evidence_bytes: usize,
    ) -> Result<crate::build::MaterializationPlan<'a>, crate::build::MaterializationError> {
        self.0.plan_materialization(
            rendered,
            &gazette_adapter_fs::FileSystem,
            protected,
            limits,
            evidence_bytes,
        )
    }

    /// Consume the original plan through intent, guarded write/read-back and completion.
    /// Neither history replay nor uncertainty causes automatic retry or release effects.
    ///
    /// # Errors
    /// Preserves exact pending intent, partial native effects or unverified completion.
    pub fn materialize(
        &mut self,
        plan: crate::build::MaterializationPlan<'_>,
        id: Sha256Digest,
        options: crate::build::MaterializationOptions,
    ) -> Result<crate::build::MaterializationResult, crate::build::MaterializationError> {
        self.0
            .materialize(plan, &gazette_adapter_fs::FileSystem, id, options)
    }

    /// Observe and settle the full original set under original or cleanly acquired later custody.
    /// This performs selected destination reads and local evidence writes only. It never signs,
    /// pushes, reactivates consumed authority or silently repairs an interrupted coordinator.
    ///
    /// # Errors
    /// Preserves acquired evidence and uncertain journal delivery; refuses changed history.
    pub fn reconcile_release(
        &mut self,
        git: &crate::release::GitSelection,
        request: crate::release::ReconcileReleaseRequest<'_>,
        options: crate::release::ReconcileReleaseOptions,
    ) -> Result<ReleaseReconciliationReceipt, crate::release::ReconcileReleaseError> {
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        self.0.reconcile_release(&reader, request, options)
    }

    /// Dispatch one original consumed target with durable start and independent native read-back.
    /// The selected local Git profile must remain under trusted exclusive custody. No automatic
    /// retry, network-provider admission, publisher approval or complete-set settlement is implied.
    ///
    /// # Errors
    /// Preserves uncertain starts/results and refuses changed authority, preparations or pre-state.
    pub fn dispatch_release_attempt(
        &mut self,
        authority: &mut impl crate::release::AuthoritySource,
        git: &crate::release::GitSelection,
        request: crate::release::DispatchAttemptRequest<'_>,
        options: crate::release::DispatchAttemptOptions,
    ) -> Result<ReleaseAttempt, crate::release::DispatchAttemptError> {
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        let dispatcher = gazette_adapter_git::Dispatcher {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        self.0.dispatch_release_attempt(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            &Sqlite,
            authority,
            &reader,
            &dispatcher,
            request,
            options,
        )
    }

    /// Acquire current authority twice, hold exact leases, read every pre-state and consume once.
    /// This requires the independently anchored coordinator writer. No push or retry occurs.
    /// The source must implement real current authority acquisition, never cached fallback.
    ///
    /// # Errors
    /// Preserves prior lease/read evidence and uncertain SQL writes; inspect original intent.
    pub fn consume_release_authorization(
        &mut self,
        authority: &mut impl crate::release::AuthoritySource,
        git: &crate::release::GitSelection,
        request: crate::release::ConsumeAuthorizationRequest<'_>,
        options: crate::release::ConsumeAuthorizationOptions,
    ) -> Result<ReleaseConsumptionReceipt, crate::release::ConsumeAuthorizationError> {
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        self.0.consume_release_authorization(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            &Sqlite,
            authority,
            &reader,
            request,
            options,
        )
    }

    /// Verify and register a pending authorization under this exact held generation.
    /// Never substitutes a newer fence, consumes approval, signs or dispatches.
    ///
    /// # Errors
    /// Refuses permission, wrong generation, signature/native mismatch or conflicting history.
    /// An uncertain write requires inspecting the original record; no automatic retry occurs.
    pub fn register_release_authorization(
        &mut self,
        request: crate::release::PreparedAuthorizationRequest<'_>,
        options: crate::release::RegisterAuthorizationOptions,
    ) -> Result<ReleaseAuthorizationReceipt, crate::release::RegisterAuthorizationError> {
        self.0.register_release_authorization(
            &gazette_adapter_fs::FileSystem,
            &gazette_adapter_git::Git,
            &Sqlite,
            request,
            options,
        )
    }

    /// Sample configured destinations through the selected Git executable and
    /// persist the complete result under this already acquired generation.
    /// The private empty isolation directory remains caller-owned. No push,
    /// authorization consumption or settlement occurs; existing IDs refuse.
    ///
    /// # Errors
    /// Preserves pre-read refusals, native results and uncertain append evidence.
    pub fn record_target_observation(
        &mut self,
        git: &crate::release::GitSelection,
        targets: &crate::release::TargetSet,
        id: Sha256Digest,
        options: crate::release::RecordObservationOptions,
    ) -> Result<
        gazette_core::store::RefObservationReceipt,
        gazette_application::release::RecordObservationError,
    > {
        let reader = gazette_adapter_git::RemoteReader {
            runner: &gazette_adapter_process::Runner,
            selection: git,
        };
        self.0
            .record_target_observation(&reader, targets, id, options)
    }

    /// Inspect fresh state against the acquired generation and private native namespace.
    ///
    /// # Errors
    /// Any failed read or changed identity permanently invalidates this lifetime.
    pub fn inspect(&mut self) -> Result<JournalInspection, Error> {
        self.0.inspect()
    }

    /// Revalidate before releasing local coordination; never reset the durable generation.
    ///
    /// # Errors
    /// Reports invalidated generation, changed native state or failed read-back.
    /// Drop releases resources without successful-close evidence.
    pub fn close(self) -> Result<(), Error> {
        self.0.close()
    }
}

/// Acquire one local writer generation in an existing initialized private journal directory.
///
/// Requires the complete independently retained expected header and explicit write
/// permission. Commits one generation increment, closes the native writer and reads
/// it back before returning a locally reserved lifetime. Currently accepts only the
/// standalone DELETE-mode journal with no sidecars or additional files. Never repairs,
/// checkpoints, migrates or initializes existing state. Connections use separate bounded
/// work budgets. Trusted private ancestors and cooperating local callers are required.
///
/// This is not cross-host/copy/restore fencing, store adoption, publisher authority or
/// release consumption. Non-cooperating SQL writes can invalidate it. A copied journal
/// can carry the same coordinates. Do not substitute newly observed coordinates for a
/// failed expected binding without resolving their history.
///
/// # Errors
/// Refuses missing permissions, contention, unsupported/unsafe state, mismatched
/// coordinates, overflow or native limits. Failed attempted advancement is explicitly
/// unverified. Interruption or lost readiness may leave an advanced generation: inspect,
/// do not blindly retry or reset it.
pub fn acquire_writer(
    root: &Path,
    expected: JournalHeader,
    options: WriterOptions,
) -> Result<Writer, WriterAcquireError> {
    gazette_application::store::acquire_writer(&Sqlite, root, expected, options).map(Writer)
}

/// Inspect an existing journal's exact supported schema, integrity and native coordinates.
///
/// Use a trusted private store path. No store is created, migrated, repaired or adopted.
/// Supplying an independently retained complete expected header detects a different
/// identity, version, epoch or generation. With `None`, this only observes current state.
/// Neither mode proves freshness against a copied/restored journal or grants write/release
/// authority; do not replace a failed expected binding with newly observed values to retry.
///
/// # Errors
/// Refuses unacknowledged connection effects, unsupported/partial schema, integrity errors,
/// unsafe/changed paths, exhausted limits and any expected-header mismatch.
pub fn inspect_journal(
    path: &Path,
    expected: Option<&JournalHeader>,
    options: ReadOptions,
) -> Result<JournalInspection, Error> {
    gazette_application::store::inspect_journal(&Sqlite, path, expected, options)
}

/// Initialize a journal in an explicitly selected, existing empty private directory.
///
/// Keep this directory outside source, staging and published output. The operation
/// reserves the directory locally, writes one fresh database and verifies the initial
/// transaction after closing its writer. It never overwrites, migrates, restores or
/// adopts an existing store. Journal coordinates do not grant release authority or
/// prove freshness against an older restore or a copy on another host.
///
/// # Errors
/// Refuses missing permissions, contention, nonempty/unsafe roots, exhausted bounds
/// or failed commit/read-back/flush. Partial output remains; never erase it to retry.
pub fn initialize(
    root: &Path,
    options: InitializeOptions,
) -> Result<InitializationReceipt, OutputError> {
    gazette_application::store::initialize(&Sqlite, root, options)
}

/// Lifetime-owned local connection; not cloneable or serializable release authority.
pub struct Session(gazette_application::store::Session<gazette_adapter_sqlite::Session>);

impl Session {
    /// Inspect a fresh SQL snapshot while retaining and rechecking local coordination.
    ///
    /// # Errors
    /// Reports changed identities, provider errors or exhausted lifetime connection limits.
    pub fn observe(&mut self) -> Result<Observation, Error> {
        self.0.observe()
    }

    /// Close the native connection before releasing the local reservation.
    ///
    /// # Errors
    /// Reports changed paths or native close failure. Drop releases resources without a receipt.
    pub fn close(self) -> Result<(), Error> {
        self.0.close()
    }
}

/// Connect read-only while exclusively reserving one explicitly selected private directory.
///
/// All cooperating callers must select the same stable local namespace. No database,
/// directory or lock file is created. Directory locks do not fence another host/copy,
/// non-cooperating SQL writers, restored history or release authority. Do not rename or
/// replace a live namespace. No schema adoption, SQL-write or publication permission follows.
///
/// # Errors
/// Refuses missing permissions, unsafe namespaces, contention or invalid existing stores.
pub fn connect(
    path: &Path,
    coordination_root: &Path,
    options: SessionOptions,
) -> Result<Session, Error> {
    gazette_application::store::connect(&Sqlite, path, coordination_root, options).map(Session)
}

/// Connect to an existing store for SQL read-only metadata observation.
///
/// Native coordination-file effects require explicit permission. No database
/// creation, initialization, migration, adoption or release action is available.
/// Use a local store in a trusted private directory, not an untrusted shared path.
///
/// # Errors
///
/// Returns permission, native-path, provider, busy or bounded-work failure.
pub fn observe(path: &Path, options: ReadOptions) -> Result<Observation, Error> {
    gazette_application::store::observe(&Sqlite, path, options)
}

/// Copy a consistent existing-store snapshot into a fresh private backup directory.
///
/// Select an existing local parent reserved for private backups, outside authored,
/// staging and publication output. This creates a backup, never a replacement,
/// migration, restore, Website adoption or release action.
///
/// # Errors
/// Returns explicit permission, source, copy, bound or read-back failures, including
/// any retained incomplete output. No failed backup is automatically resumed or deleted.
pub fn backup(request: BackupRequest<'_>) -> Result<BackupReceipt, OutputError> {
    gazette_application::store::backup(&Sqlite, &gazette_adapter_fs::FileSystem, request)
}

/// Check an existing private backup against the original receipt's manifest digest.
///
/// Retain that pin independently; hashing a suspect manifest does not authenticate it.
/// This returns point-in-time byte/integrity evidence, not a reusable write capability,
/// historical durability proof, Website adoption or permission to restore old authority.
///
/// # Errors
/// Refuses invalid policy, unsafe paths/inventory, changed bytes, unsupported manifests
/// and native read-back failure. Existing backups are never repaired or deleted.
pub fn verify_backup(
    directory: &Path,
    manifest_sha256: Sha256Digest,
    options: BackupVerifyOptions,
) -> Result<BackupInspection, Error> {
    gazette_application::store::verify_backup(&Sqlite, directory, manifest_sha256, options)
}

/// Prepare a fresh byte-exact recovery copy from independently pinned backup evidence.
///
/// Select an existing trusted private parent outside authored, staging and published
/// output. No live-store path is accepted: existing databases are never replaced,
/// selected, migrated or activated. A prepared copy cannot revive release authority;
/// Website adoption still requires fenced history reconciliation at its own boundary.
///
/// # Errors
/// Refuses missing permission, invalid/changed evidence, unsafe output, bounds and
/// failed read-back/flush. Incomplete output remains identified, never silently reused.
pub fn prepare_restore(request: PrepareRestoreRequest<'_>) -> Result<RestoreReceipt, OutputError> {
    gazette_application::store::prepare_restore(&Sqlite, &gazette_adapter_fs::FileSystem, request)
}
