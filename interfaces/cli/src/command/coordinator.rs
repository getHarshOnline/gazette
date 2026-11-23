//! Coordinator and registry commands shared by CLI and MCP.

use std::{fmt, path::PathBuf};

use gazette_sdk::{
    build, corpus,
    publication::content,
    source::{self, Sha256Digest},
    store::{self, coordinator},
    toml,
    workspace::Key,
};

use super::{Command, Kernel};

/// Enroll one fresh journal under an independently retained local coordinator anchor.
#[derive(Debug)]
pub struct Enroll {
    /// Existing empty private anchor directory.
    pub anchor_root: PathBuf,
    /// Existing fresh Gazette journal directory.
    pub journal_root: PathBuf,
    /// Exact selected native coordinator identifier.
    pub coordinator_id: String,
    /// Independently selected principal-scope binding.
    pub principal_scope: Sha256Digest,
    /// Complete protected source, output, staging and backup roots.
    pub protected_roots: Vec<PathBuf>,
    /// Explicit native read and local-reservation options.
    pub options: store::SessionOptions,
    /// Permit fresh bounded anchor persistence.
    pub allow_anchor_write: bool,
}

impl Command for Enroll {
    type Outcome = coordinator::LocalCoordinatorSeal;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        coordinator::enroll(
            coordinator::EnrollmentRequest {
                anchor_root: &self.anchor_root,
                journal_root: &self.journal_root,
                coordinator_id: &self.coordinator_id,
                principal_scope: self.principal_scope,
                protected_roots: &self.protected_roots,
            },
            self.options,
            self.allow_anchor_write,
        )
    }
}

/// Inspect exact retained coordinator state without repair or resampling.
#[derive(Debug)]
pub struct Inspect {
    /// Existing private anchor directory.
    pub anchor_root: PathBuf,
    /// Independently retained exact native binding.
    pub binding: coordinator::LocalCoordinatorBinding,
    /// Explicit native read and local-reservation options.
    pub options: store::SessionOptions,
}

impl Command for Inspect {
    type Outcome = coordinator::LocalCoordinatorState;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        coordinator::inspect(&self.anchor_root, &self.binding, self.options)
    }
}

/// Adopt one exact latest-clean detached restore into a fresh successor root.
#[derive(Debug)]
pub struct AdoptRestore {
    /// Existing private anchor directory.
    pub anchor_root: PathBuf,
    /// Independently retained exact native binding.
    pub binding: coordinator::LocalCoordinatorBinding,
    /// Independently retained prior clean coordinator seal.
    pub previous: coordinator::LocalCoordinatorSeal,
    /// Exact latest-clean detached restore directory.
    pub restore_root: PathBuf,
    /// Independently retained restore receipt identity.
    pub receipt_sha256: Sha256Digest,
    /// Existing fresh successor root.
    pub successor_root: PathBuf,
    /// Complete protected source, output, staging and backup roots.
    pub protected_roots: Vec<PathBuf>,
    /// Explicit adoption, anchor-write and native-read options.
    pub options: coordinator::RestoreAdoptionOptions,
}

impl Command for AdoptRestore {
    type Outcome = coordinator::RestoreAdoptionReceipt;
    type Failure = store::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        coordinator::adopt(
            coordinator::AdoptionRequest {
                anchor_root: &self.anchor_root,
                binding: &self.binding,
                previous: &self.previous,
                restore_root: &self.restore_root,
                receipt_sha256: self.receipt_sha256,
                successor_root: &self.successor_root,
                protected_roots: &self.protected_roots,
            },
            self.options,
        )
    }
}

/// Failure from the complete registry-reconciliation use case.
#[derive(Debug)]
pub enum ReconcileFailure {
    /// Refusal before the registry writer is acquired.
    Preparation {
        /// Stable redacted failure classification.
        code: &'static str,
        /// Operator-facing explanation without private input bytes.
        message: String,
    },
    /// Exact native reconciliation failure, including conservative effect evidence.
    Effect(build::RegistryReconcileError),
}

impl ReconcileFailure {
    fn native(code: &'static str, error: &impl fmt::Display) -> Self {
        Self::Preparation {
            code,
            message: error.to_string(),
        }
    }

    /// Stable native failure classification.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Preparation { code, .. } => code,
            Self::Effect(error) => error.code(),
        }
    }

    /// Separate an effect-aware native result from preparation refusal.
    ///
    /// # Errors
    /// Returns the unchanged preparation refusal when no native effect started.
    pub fn into_effect(self) -> Result<build::RegistryReconcileError, Self> {
        match self {
            Self::Effect(error) => Ok(error),
            preparation => Err(preparation),
        }
    }
}

impl fmt::Display for ReconcileFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preparation { message, .. } => formatter.write_str(message),
            Self::Effect(error) => error.fmt(formatter),
        }
    }
}

/// One complete retained-publication to private-registry reconciliation.
pub struct ReconcileRegistry {
    /// Complete supplied workspace TOML.
    pub source: build::WorkspaceSource,
    /// Exact configured site used for source acquisition.
    pub site: Key,
    /// Independently permit configured native source reads.
    pub allow_native_reads: bool,
    /// Workspace parser limits.
    pub syntax: toml::Limits,
    /// Complete selected-source capture limits.
    pub acquisition: source::PreparationLimits,
    /// Retained corpus parsing limits.
    pub corpus: corpus::Limits,
    /// Resolved publication graph limits.
    pub graph: content::Limits,
    /// Existing private store root.
    pub root: PathBuf,
    /// Independently retained expected journal coordinates.
    pub expected: store::JournalHeader,
    /// Complete registry projection and writer policy.
    pub options: build::RegistryReconcileOptions,
}

impl Command for ReconcileRegistry {
    type Outcome = store::RegistryReceipt;
    type Failure = ReconcileFailure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(ReconcileFailure::Preparation {
                code: "publication_permission",
                message: "Explicit configured-source read permission required; no source acquisition started".into(),
            });
        }
        self.options
            .registry
            .validate()
            .and_then(|_| self.options.writer.validate())
            .map_err(|error| ReconcileFailure::native(error.code(), &error))?;
        let acquired = build::acquire_selected_publication(
            &self.source,
            &self.site,
            self.syntax,
            self.acquisition,
        )
        .map_err(|error| ReconcileFailure::native(error.code(), &error))?;
        let parsed = build::parse_publication(&acquired, self.corpus)
            .map_err(|error| ReconcileFailure::native(error.code(), &error))?;
        let resolved = parsed
            .resolve(self.graph)
            .map_err(|error| ReconcileFailure::native(error.code(), &error))?;
        build::reconcile_registry(&resolved, &self.root, self.expected, self.options)
            .map_err(ReconcileFailure::Effect)
    }
}
