//! Complete private registry reconciliation over the existing writer and root guards.

use std::{fmt, path::Path};

use gazette_core::{
    destination,
    store::{
        self, JournalHeader, RegistryInput, RegistryReceipt, RegistryWriteOptions, WriterOptions,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    store::{JournalWriter, RegistryWriter, WriterConnector},
};

use super::{RegistryError, RegistryLimits};
use crate::build::ResolvedPublication;

/// Independent projection, registry-write and native writer permissions/bounds.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct RegistryReconcileOptions {
    /// Complete projection must fit before writer acquisition.
    pub projection: RegistryLimits,
    /// Explicit complete-set registry replacement permission.
    pub registry: RegistryWriteOptions,
    /// Existing native generation/reservation and connection policy.
    pub writer: WriterOptions,
}

/// Reconciliation failure retains the distinction between preparation and effects.
#[derive(Debug)]
pub enum RegistryReconcileError {
    /// Shared model preparation failed before any writer was acquired.
    Projection(RegistryError),
    /// Effect or bound refusal before writer acquisition.
    Policy(store::Error),
    /// A portable source root could not be resolved before writer acquisition.
    SourceRoot(super::super::CompositionError),
    /// Native separation failure; an already acquired generation is never reset.
    Separation {
        /// Native root relationship or identity refusal.
        cause: destination::Error,
        /// Whether this operation had already acquired and advanced a writer.
        writer_acquired: bool,
    },
    /// Native acquisition retains its own attempted-generation evidence.
    Acquire(store::WriterAcquireError),
    /// Registry mutation failed after successful writer acquisition.
    Write(store::RegistryWriteError),
    /// Registry commit/read-back succeeded, but final lifetime close did not.
    Close {
        /// Failed close does not erase this verified replacement evidence.
        receipt: Box<RegistryReceipt>,
        /// Native lifetime validation or close failure.
        cause: store::Error,
    },
}

impl RegistryReconcileError {
    /// Stable original failure classification; effect details remain in this value.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Projection(error) => error.code(),
            Self::Policy(error) => error.code(),
            Self::SourceRoot(error) => error.code(),
            Self::Separation { .. } => "registry_source_separation",
            Self::Acquire(error) => error.cause.code(),
            Self::Write(error) => error.cause.code(),
            Self::Close { cause, .. } => cause.code(),
        }
    }
}

impl fmt::Display for RegistryReconcileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Projection(error) => write!(f, "{error}; no writer acquired"),
            Self::Policy(error) => write!(f, "{error}; no writer acquired"),
            Self::SourceRoot(error) => write!(f, "{error}; no writer acquired"),
            Self::Separation {
                cause,
                writer_acquired,
            } => write!(
                f,
                "{cause}; writer acquired: {writer_acquired}; preserve the store and inspect before retrying"
            ),
            Self::Acquire(error) => fmt::Display::fmt(error, f),
            Self::Write(error) => write!(f, "{error}; writer generation was already advanced"),
            Self::Close { receipt, cause } => write!(
                f,
                "{cause}; registry {} was verified at writer generation {}, but close failed; inspect before retrying",
                receipt.registry.rows_sha256, receipt.journal.writer_fence
            ),
        }
    }
}

impl std::error::Error for RegistryReconcileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Projection(error) => Some(error),
            Self::Policy(error) => Some(error),
            Self::SourceRoot(error) => Some(error),
            Self::Separation { cause, .. } => Some(cause),
            Self::Acquire(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::Close { cause, .. } => Some(cause),
        }
    }
}

impl ResolvedPublication<'_, '_> {
    /// Replace the private registry from this complete retained structural model.
    ///
    /// Permission, complete projection bounds and native source separation are checked
    /// before acquiring a writer. The native generation is a separate durable effect;
    /// a later registry failure must never reset it. This writes no source/output tree,
    /// acquires no publication approval and erases no durable release history.
    ///
    /// # Errors
    /// Preserves preparation, separation, generation, registry and close failures.
    /// A failed return after an attempted effect is not permission for a blind retry.
    pub fn reconcile_registry<C: WriterConnector>(
        &self,
        connector: &C,
        separation: &impl RootSeparation,
        root: &Path,
        expected: JournalHeader,
        options: RegistryReconcileOptions,
    ) -> Result<RegistryReceipt, RegistryReconcileError>
    where
        C::Writer: RegistryWriter,
    {
        options
            .registry
            .validate()
            .map_err(RegistryReconcileError::Policy)?;
        options
            .writer
            .validate()
            .map_err(RegistryReconcileError::Policy)?;
        let projection = self
            .registry(options.projection)
            .map_err(RegistryReconcileError::Projection)?;
        let inputs = self.inputs();
        let roots = inputs
            .source_roots()
            .map_err(RegistryReconcileError::SourceRoot)?;
        separation
            .check_root_separation(root, &roots)
            .map_err(|cause| RegistryReconcileError::Separation {
                cause,
                writer_acquired: false,
            })?;
        let mut writer = connector
            .acquire_writer(root, expected, options.writer)
            .map_err(RegistryReconcileError::Acquire)?;
        separation
            .check_root_separation(root, &roots)
            .map_err(|cause| RegistryReconcileError::Separation {
                cause,
                writer_acquired: true,
            })?;
        let receipt = writer
            .replace_registry(
                RegistryInput {
                    model: &projection.model,
                    semantic_sha256: projection.semantic_sha256,
                    configuration_sha256: projection.configuration_sha256,
                    documents: &projection.documents,
                },
                options.registry,
            )
            .map_err(RegistryReconcileError::Write)?;
        if let Err(cause) = writer.close() {
            return Err(RegistryReconcileError::Close {
                receipt: Box::new(receipt),
                cause,
            });
        }
        Ok(receipt)
    }
}
