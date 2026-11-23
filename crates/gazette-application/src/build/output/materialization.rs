//! Configured-output intent precedes effects; exact completion alone advances ownership.

use std::{collections::BTreeMap, fmt, path::PathBuf};

use gazette_core::{
    destination::OutputWriteError,
    publication::{Key, routing::PublicBaseUrl},
    source::{Limits, Sha256Digest},
    store::{self, MaterializationCompletionInput, MaterializationInput},
};
use gazette_ports::{
    destination::{OutputReader, OutputWriter},
    store::MaterializationWriter,
};
use serde::Serialize;

use super::{MaterializedOutput, OutputPlan, OutputPlanError, PreviousOutput, RetainedOutput};
use crate::build::{PublicationRender, SourceEvidence, serialization};

mod inspection;
mod set;
pub use inspection::{
    MaterializationErrorInspection, MaterializationEvidenceInspection, MaterializationInspection,
    MaterializationPlanCauseInspection, MaterializationSetPlanInspection,
    MaterializationSetWriteInspection, MaterializationStoreInspection,
    MaterializedOutputInspection, OutputDestinationInspection, OutputPlanCauseInspection,
    OutputSourceInspection,
};
pub use set::{
    MaterializationSetPlan, MaterializationSetPlanError, MaterializationSetPlanFailure,
    MaterializationSetWriteError,
};

/// Independent permissions and bounded private build evidence.
#[derive(Clone, Copy, Debug)]
pub struct MaterializationOptions {
    /// Allow this original plan's guarded local filesystem effects.
    pub allow_write: bool,
    /// Permit durable local history, never release effects.
    pub history: store::MaterializationWriteOptions,
    /// Maximum encoded private acquisition/composition/render evidence.
    pub evidence_bytes: usize,
}

impl Default for MaterializationOptions {
    fn default() -> Self {
        Self {
            allow_write: false,
            history: Default::default(),
            evidence_bytes: 4 << 20,
        }
    }
}

/// Write-free original plan, bound to the inspected preceding completed head.
#[derive(Debug)]
pub struct MaterializationPlan<'a> {
    plan: OutputPlan<'a>,
    previous: Option<Sha256Digest>,
    evidence: String,
}

/// Historical replay and actual current write/read-back remain different results.
#[derive(Debug)]
pub struct MaterializationResult {
    /// Independently read durable completion or replay history.
    pub history: store::MaterializationReceipt,
    /// Present only when this invocation actually wrote/read back the original plan.
    /// Absence means historical replay, not proof of current destination contents.
    pub output: Option<MaterializedOutput>,
}

impl MaterializationPlan<'_> {
    /// Exact original filesystem transitions, available before granting effects.
    pub fn output(&self) -> &OutputPlan<'_> {
        &self.plan
    }

    /// Original preceding completed ownership coordinate, or an unowned destination.
    pub fn previous(&self) -> Option<Sha256Digest> {
        self.previous
    }

    /// Persist intent, apply the original plan once, then atomically retain completion.
    /// An equal recorded intent never dispatches another filesystem effect.
    ///
    /// # Errors
    /// Preserves durable pending history and any actual completed/partial native result.
    /// Neither failure nor process interruption automatically retries or rolls back.
    pub fn materialize(
        self,
        history: &mut impl MaterializationWriter,
        destination: &impl OutputWriter,
        id: Sha256Digest,
        options: MaterializationOptions,
    ) -> Result<MaterializationResult, MaterializationError> {
        if !options.allow_write {
            return Err(MaterializationError::Permission);
        }
        options
            .history
            .validate()
            .map_err(MaterializationError::Store)?;
        if self.evidence.len() > options.evidence_bytes {
            return Err(MaterializationError::Evidence(
                gazette_core::render::Error::Limit("materialization evidence"),
            ));
        }
        let input = MaterializationInput {
            id,
            site: &self.plan.site,
            binding: self.plan.observed.binding(),
            previous: self.previous,
            observed: self.plan.observed.inventory(),
            expected: &self.plan.expected,
            generated: self.plan.generated.inventory(),
            protected: &self.plan.protected,
            evidence: &self.evidence,
            evidence_sha256: Sha256Digest::of(self.evidence.as_bytes()),
        };
        let original = history
            .record_materialization_intent(input, options.history)
            .map_err(|cause| MaterializationError::History { id, cause })?;
        if original.record.intent.as_input() != input
            || original.inserted && original.record.completion.is_some()
        {
            return Err(MaterializationError::History {
                id,
                cause: store::MaterializationWriteError {
                    cause: store::Error::MaterializationVerification,
                    effect: store::MaterializationEffect::Unverified,
                },
            });
        }
        if !original.inserted {
            original
                .record
                .completion
                .as_ref()
                .ok_or(MaterializationError::Store(
                    store::Error::MaterializationPending,
                ))?
                .validate(&original.record.intent)
                .map_err(MaterializationError::Store)?;
            return Ok(MaterializationResult {
                history: original,
                output: None,
            });
        }
        let output = self
            .plan
            .materialize(destination, true)
            .map_err(|cause| MaterializationError::Write { id, cause })?;
        let completion = match history.record_materialization_completion(
            MaterializationCompletionInput {
                intent: &original.record.intent,
                read_back: &output.observation,
            },
            options.history,
        ) {
            Ok(completion) => completion,
            Err(cause) => {
                return Err(MaterializationError::Completion {
                    id,
                    cause,
                    output: Box::new(output),
                });
            }
        };
        if completion.record.intent != original.record.intent
            || completion
                .record
                .completion
                .as_ref()
                .is_none_or(|completed| completed.validate(&original.record.intent).is_err())
        {
            return Err(MaterializationError::Completion {
                id,
                cause: store::MaterializationWriteError {
                    cause: store::Error::MaterializationVerification,
                    effect: store::MaterializationEffect::Unverified,
                },
                output: Box::new(output),
            });
        }
        Ok(MaterializationResult {
            history: completion,
            output: Some(output),
        })
    }
}

impl<'a> RetainedOutput<'a> {
    pub(in crate::build) fn plan_materialization(
        self,
        history: &mut impl MaterializationWriter,
        destination: &impl OutputReader,
        protected: &[PathBuf],
        limits: Limits,
        evidence: impl FnOnce() -> Result<Vec<u8>, gazette_core::render::Error>,
    ) -> Result<MaterializationPlan<'a>, MaterializationError> {
        let root = self.root.clone();
        let mut selected_protected = self.protected.clone();
        selected_protected.extend_from_slice(protected);
        let mut lifecycle = history
            .materialization_roots()
            .map_err(MaterializationError::Store)?;
        lifecycle.extend_from_slice(protected);
        lifecycle.sort();
        lifecycle.dedup();
        selected_protected.extend_from_slice(&lifecycle);
        let observed = destination
            .read_output(&root, &selected_protected, limits)
            .map_err(|error| MaterializationError::Plan(OutputPlanError::Observation(error)))?;
        let frontier = history
            .inspect_materialization(observed.binding().canonical_root())
            .map_err(MaterializationError::Store)?
            .frontier;
        if frontier.root != observed.binding().canonical_root() {
            return Err(MaterializationError::Store(
                store::Error::MaterializationVerification,
            ));
        }
        if frontier.active.is_some() {
            return Err(MaterializationError::Store(
                store::Error::MaterializationPending,
            ));
        }
        let previous = frontier
            .head
            .as_ref()
            .map(|record| {
                if record
                    .completion
                    .as_ref()
                    .is_none_or(|completion| completion.validate(&record.intent).is_err())
                {
                    return Err(MaterializationError::Store(
                        store::Error::MaterializationVerification,
                    ));
                }
                Ok(PreviousOutput {
                    site: &record.intent.site,
                    binding: &record.intent.binding,
                    generated: &record.intent.generated,
                    // Historical source locators are provenance, not current native
                    // read permissions. Protect this build's actual selected roots
                    // and the held journal; do not reopen retired source locations.
                    protected: &[],
                    configured_root: None,
                })
            })
            .transpose()?;
        let plan = self
            .plan(destination, previous, limits, &lifecycle)
            .map_err(MaterializationError::Plan)?;
        if plan.observed != observed {
            return Err(MaterializationError::Plan(OutputPlanError::Observation(
                gazette_core::source::Error::Changed(root).into(),
            )));
        }
        let evidence = evidence().map_err(MaterializationError::Evidence)?;
        let evidence = String::from_utf8(evidence)
            .map_err(|_| MaterializationError::Store(store::Error::MaterializationVerification))?;
        Ok(MaterializationPlan {
            plan,
            previous: frontier.head.as_ref().map(|record| record.intent.id),
            evidence,
        })
    }
}

impl PublicationRender<'_> {
    /// Plan using the actual held journal's completed head, not an imported receipt.
    /// Actual held lifecycle roots and current selected inputs are protected.
    /// Historical source locators stay evidence, not a renewed native-read capability.
    ///
    /// # Errors
    /// Refuses unresolved history, changed native ownership, unsafe destinations or bounds.
    /// No output or history write occurs during planning.
    pub fn plan_materialization<'a>(
        &'a self,
        history: &mut impl MaterializationWriter,
        destination: &impl OutputReader,
        protected: &[PathBuf],
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationPlan<'a>, MaterializationError> {
        let root = self
            .inputs()
            .output_directory()
            .map_err(|error| MaterializationError::Plan(OutputPlanError::Coordinate(error)))?
            .ok_or(MaterializationError::Plan(OutputPlanError::Unconfigured))?;
        RetainedOutput {
            site: self.inputs().site(),
            root,
            generated: self.site().output(),
            protected: self.protected_roots().to_vec(),
        }
        .plan_materialization(history, destination, protected, limits, || {
            #[derive(Serialize)]
            struct Evidence<'a> {
                configuration: &'a crate::configuration::Evidence,
                site: &'a Key,
                sources: BTreeMap<&'a Key, SourceEvidence<'a>>,
                selection: Option<&'a gazette_compiler::authored::Selection>,
                preview_urls: Option<&'a BTreeMap<Key, PublicBaseUrl>>,
                rendering: &'a crate::build::SiteRenderEvidence,
            }
            serialization::encode(
                &Evidence {
                    configuration: self.inputs().configuration_evidence(),
                    site: self.inputs().site(),
                    sources: self.inputs().source_evidence(),
                    selection: self.selection(),
                    preview_urls: self.preview_urls(),
                    rendering: self.site().evidence(),
                },
                evidence_bytes,
                "materialization evidence",
            )
        })
    }
}

/// Exact failed phase; a successful prefix is never whole-operation success.
#[derive(Debug)]
pub enum MaterializationError {
    /// Missing filesystem permission; no history append attempted.
    Permission,
    /// Write-free destination or prior ownership validation failed.
    Plan(OutputPlanError),
    /// Held history observation or record permission failed.
    Store(store::Error),
    /// Private build evidence could not be encoded within its ceiling.
    Evidence(gazette_core::render::Error),
    /// Intent append did not produce independently verified history; no FS dispatch followed.
    History {
        /// Original attempted operation, even when its durable receipt is unverified.
        id: Sha256Digest,
        /// Independently observed storage effects and native cause.
        cause: store::MaterializationWriteError,
    },
    /// Intent was retained, then the actual FS writer failed; preserve pending history.
    Write {
        /// Original operation whose durable intent may remain unresolved.
        id: Sha256Digest,
        /// Actual attempted paths, residue and native cause.
        cause: OutputWriteError,
    },
    /// FS read-back succeeded but completion is uncertain; do not start another operation.
    Completion {
        /// Original operation, not a new authorization to repeat its write.
        id: Sha256Digest,
        /// Exact uncertain journal phase and storage effect evidence.
        cause: store::MaterializationWriteError,
        /// Successful native read-back retained independently of journal delivery.
        output: Box<MaterializedOutput>,
    },
}

impl MaterializationError {
    /// Stable phase diagnostic shared by every interface.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Permission => "materialization_permission",
            Self::Plan(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::Evidence(_) => "materialization_evidence",
            Self::History { .. } => "materialization_intent_unverified",
            Self::Write { .. } => "materialization_write_failed",
            Self::Completion { .. } => "materialization_completion_unverified",
        }
    }
}

impl fmt::Display for MaterializationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Permission => f.write_str("configured-output write permission required; no history append or filesystem write attempted"),
            Self::Plan(error) => error.fmt(f),
            Self::Store(error) => error.fmt(f),
            Self::Evidence(error) => error.fmt(f),
            Self::History { id, cause } => write!(f, "{cause}; output intent {id:?} is unverified; preserve and inspect before any further operation"),
            Self::Write { id, cause } => write!(f, "{cause}; output intent {id:?} remains pending; preserve and inspect; do not retry"),
            Self::Completion { id, cause, .. } => write!(f, "{cause}; native output read-back succeeded for {id:?}, but durable completion is unverified; preserve both and reconcile without rewriting"),
        }
    }
}

impl std::error::Error for MaterializationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Permission => None,
            Self::Plan(error) => Some(error),
            Self::Store(error) => Some(error),
            Self::Evidence(error) => Some(error),
            Self::History { cause, .. } => Some(cause),
            Self::Write { cause, .. } => Some(cause),
            Self::Completion { cause, .. } => Some(cause),
        }
    }
}
