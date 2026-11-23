//! Durable staging preparation consumes retained render bytes; it never compiles again.

mod check;
mod deployability;
mod evidence;
mod qualification;
pub(in crate::build) use check::record_retained_check;
pub use check::{PreparationCheckOptions, PreparationCheckRequest, PreparationChecks};
pub use deployability::{DeployabilityError, ensure_deployable_sites};
pub(crate) use evidence::{PreparationAdmissionEvidence, inspect_preparation_evidence};
pub use qualification::{PreparationQualification, TechnicalCertificate};
mod local;
mod set;
pub use local::{
    LocalPreparationError, LocalPreparationFailure, LocalPreparationOptions,
    LocalPreparationRequest,
};
pub use set::{
    LocalPreparationSetError, LocalPreparationSetFailure, SitePreparationReceipt,
    SitePreparationRequest,
};

use super::{PublicationRender, SiteRenderEvidence, SourceEvidence, serialization};
use gazette_core::{
    release::PublicationDestination,
    source::{self, CapturedSource, Sha256Digest},
    stage, store,
};
use gazette_ports::{
    stage::{StagePlanner, StageRetention, StageTarget},
    store::{JournalWriter, PreparationWriter, WriterConnector},
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
};

/// Explicit coordinates for local preparation, not a release destination or authorization.
#[derive(Clone, Copy, Debug)]
pub struct PreparationRequest<'a> {
    /// Existing private journal root, separate from sources and staging.
    pub journal: &'a Path,
    /// Independently retained expected journal generation.
    pub expected: store::JournalHeader,
    /// Existing trusted stage parent.
    pub parent: &'a Path,
    /// Exact new child, never a fallback name.
    pub name: &'a str,
    /// Opaque immutable preparation coordinate.
    pub id: Sha256Digest,
}

/// Independent permissions and phase limits for local preparation.
#[derive(Clone, Copy, Debug)]
pub struct PreparationOptions {
    /// Permit exact new stage creation and explicit native retention.
    pub allow_stage: bool,
    /// Permit preparation history separately from filesystem writes.
    pub record: store::PreparationWriteOptions,
    /// Existing local writer acquisition permissions and bounds.
    pub writer: store::WriterOptions,
    /// Bounds applied to the entire retained output before journal acquisition.
    pub output: source::Limits,
    /// Maximum private acquisition/render evidence bytes, excluding source payloads.
    pub evidence_bytes: usize,
}
impl Default for PreparationOptions {
    fn default() -> Self {
        Self {
            allow_stage: false,
            record: Default::default(),
            writer: Default::default(),
            output: Default::default(),
            evidence_bytes: 1 << 20,
        }
    }
}

impl PreparationOptions {
    fn validate(self) -> Result<(), PreparationError> {
        let before = PreparationError::before;
        if !self.allow_stage {
            return Err(before(PreparationFailure::Permission));
        }
        self.record
            .validate()
            .map_err(|e| before(PreparationFailure::Journal(e)))?;
        self.writer
            .validate()
            .map_err(|e| before(PreparationFailure::Journal(e)))
    }
}

/// Known failing phase; it never collapses native effects into a generic retry instruction.
#[derive(Debug)]
pub enum PreparationFailure {
    /// Development sources or local URLs cannot enter durable release preparation.
    Preview,
    /// Private unfiltered rendering has no explicit complete non-draft candidate selection.
    Selection,
    /// A site with an explicit delivery destination is not enabled for deployment mechanics.
    Deployability(DeployabilityError),
    /// Stage effects were not explicitly allowed.
    Permission,
    /// Private evidence encoding failed or exceeded its bound.
    Evidence(gazette_core::render::Error),
    /// Required retained-output decoding or literal disclosure check failed.
    Disclosure(super::DisclosureProbeError),
    /// Configured output checker failed after the required disclosure scan.
    Policy(super::PolicyCheckError),
    /// Native root/source separation failed before journal acquisition.
    Separation(gazette_core::destination::Error),
    /// Native staging, synchronization or read-back failed.
    Stage(stage::Error),
    /// Writer acquisition failed, possibly after advancing its generation.
    Writer(store::WriterAcquireError),
    /// Journal inspection, permission or final close failed.
    Journal(store::Error),
    /// Record insertion or independent read-back failed.
    Record(store::PreparationWriteError),
    /// Existing intent requires explicit reconciliation, never automatic stage recreation.
    Replay,
    /// Returned native/store evidence disagreed with the exact retained inputs.
    Mismatch,
    /// Independent technical-profile requirements do not match the retained observation.
    Qualification,
}

/// Preserve confirmed history and possible native residue across every failed later phase.
#[derive(Debug)]
pub struct PreparationError {
    /// Exact failing phase and its native effect classification.
    pub cause: Box<PreparationFailure>,
    /// Last fully returned independent journal read-back, if any.
    pub receipt: Option<Box<store::PreparationReceipt>>,
    /// Last independently observed acquired journal coordinates, even when no record returned.
    pub journal: Option<store::JournalHeader>,
    /// Acquisition returned successfully, even if the first inspection lost its report.
    /// This does not recreate the writer lifetime or invent unobserved coordinates.
    pub writer_acquired: bool,
    /// Intended native stage after durable intent; existence/completeness is not implied.
    pub stage: Option<PathBuf>,
}
impl PreparationError {
    pub(super) fn before(cause: PreparationFailure) -> Self {
        Self {
            cause: Box::new(cause),
            receipt: None,
            journal: None,
            writer_acquired: false,
            stage: None,
        }
    }
}
impl fmt::Display for PreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let phase = match self.cause.as_ref() {
            PreparationFailure::Preview => "development preview is not release output",
            PreparationFailure::Selection => {
                "publication preparation requires an explicit complete non-draft selection"
            }
            PreparationFailure::Deployability(_) => "destination-bearing site is not deployable",
            PreparationFailure::Permission => "stage permission missing",
            PreparationFailure::Evidence(_) => "private evidence encoding failed",
            PreparationFailure::Disclosure(_) => "required retained-output disclosure check failed",
            PreparationFailure::Policy(_) => "configured retained-output policy check failed",
            PreparationFailure::Separation(_) => "source/journal/stage separation failed",
            PreparationFailure::Stage(_) => "native stage operation failed",
            PreparationFailure::Writer(_) => "writer acquisition failed",
            PreparationFailure::Journal(_) => "journal verification or close failed",
            PreparationFailure::Record(_) => "preparation record failed",
            PreparationFailure::Replay => {
                "intent already exists; reconcile without recreating its stage"
            }
            PreparationFailure::Mismatch => "returned evidence mismatches retained inputs",
            PreparationFailure::Qualification => {
                "preparation_qualification: independent technical requirements mismatch"
            }
        };
        write!(f, "{phase}")?;
        if let Some(cause) = std::error::Error::source(self) {
            write!(f, ": {cause}")?;
        }
        write!(
            f,
            "; writer acquired {}; preserve exact journal and stage evidence; no release or rollback is implied",
            self.writer_acquired
        )
    }
}
impl std::error::Error for PreparationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self.cause.as_ref() {
            PreparationFailure::Evidence(e) => Some(e),
            PreparationFailure::Deployability(e) => Some(e),
            PreparationFailure::Disclosure(e) => Some(e),
            PreparationFailure::Policy(e) => Some(e),
            PreparationFailure::Separation(e) => Some(e),
            PreparationFailure::Stage(e) => Some(e),
            PreparationFailure::Writer(e) => Some(e),
            PreparationFailure::Journal(e) => Some(e),
            PreparationFailure::Record(e) => Some(e),
            _ => None,
        }
    }
}

#[derive(Serialize)]
struct Evidence<'a> {
    format: &'static str,
    repository_source_provenance: super::RepositorySourceProvenance,
    runtime_build_profile: super::RuntimeBuildProfile,
    configuration_sha256: Sha256Digest,
    site: &'a gazette_compiler::workspace::Key,
    sources: BTreeMap<&'a gazette_compiler::workspace::Key, SourceEvidence<'a>>,
    selection: &'a gazette_compiler::authored::Selection,
    candidate_check: Option<&'a super::PolicyCheck>,
    rendering: &'a SiteRenderEvidence,
}

/// Held read-only target and exact private evidence, before writer acquisition.
pub(super) struct PreparationPlan<T> {
    pub(super) target: T,
    pub(super) evidence: String,
}

/// Retained bytes and caller-owned evidence share custody mechanics, not admission meaning.
/// Callers validate their artifact class before construction; this cannot create a check,
/// certificate or authored selection. Evidence is bounded again before native planning.
pub(super) struct RetainedPreparation<'a> {
    pub(super) output: &'a CapturedSource,
    pub(super) protected_roots: &'a [PathBuf],
    pub(super) destination: Option<&'a PublicationDestination>,
    pub(super) evidence: String,
}

impl PublicationRender<'_> {
    fn ensure_preparation_mode(
        &self,
    ) -> Result<&gazette_compiler::authored::Selection, PreparationError> {
        if self.preview_urls().is_some()
            || self
                .inputs()
                .sources()
                .values()
                .any(crate::source::AcquiredInput::is_development)
        {
            return Err(PreparationError::before(PreparationFailure::Preview));
        }
        self.selection()
            .ok_or_else(|| PreparationError::before(PreparationFailure::Selection))
    }

    fn preparation_evidence(&self, maximum: usize) -> Result<Vec<u8>, PreparationError> {
        serialization::encode(
            &Evidence {
                format: "gazette.preparation-evidence/1",
                repository_source_provenance: super::repository_source_provenance(),
                runtime_build_profile: super::runtime_build_profile(),
                configuration_sha256: self.inputs().compilation().source_sha256,
                site: self.inputs().site(),
                sources: self.inputs().source_evidence(),
                selection: self.ensure_preparation_mode()?,
                candidate_check: self.candidate_check(),
                rendering: self.site().evidence(),
            },
            maximum,
            "preparation evidence",
        )
        .map_err(|e| PreparationError::before(PreparationFailure::Evidence(e)))
    }

    /// Validate the complete retained output and hold its native target without effects.
    pub(super) fn plan_preparation<S: StagePlanner>(
        &self,
        stage: &S,
        request: PreparationRequest<'_>,
        options: PreparationOptions,
    ) -> Result<PreparationPlan<S::Target>, PreparationError> {
        self.retained_preparation(options)?
            .plan(stage, request, options)
    }

    fn retained_preparation(
        &self,
        options: PreparationOptions,
    ) -> Result<RetainedPreparation<'_>, PreparationError> {
        self.ensure_preparation_mode()?;
        let workspace = &self.inputs().compilation().workspace;
        ensure_deployable_sites(
            workspace,
            std::iter::once((
                self.inputs().site(),
                workspace.destination(self.inputs().site()).is_some(),
            )),
        )
        .map_err(|error| PreparationError::before(PreparationFailure::Deployability(error)))?;
        options.validate()?;
        let encoded = self.preparation_evidence(options.evidence_bytes)?;
        let evidence = String::from_utf8(encoded)
            .map_err(|_| PreparationError::before(PreparationFailure::Mismatch))?;
        Ok(RetainedPreparation {
            output: self.site().output(),
            protected_roots: self.protected_roots(),
            destination: self
                .inputs()
                .compilation()
                .workspace
                .destination(self.inputs().site()),
            evidence,
        })
    }

    /// Record intent, stage the existing output bytes and durably record native retention.
    /// No source reread, compiler invocation, Git operation or network effect occurs.
    /// An existing intent returns reconciliation evidence, never automatic stage adoption.
    /// An explicit complete non-draft candidate selection is required before any effect;
    /// it is retained unchanged, not inferred from apparently publishable output bytes.
    ///
    /// # Errors
    /// Keeps record-attempt, writer-generation and filesystem failure evidence separate.
    /// Neither a successful receipt nor an exact replay is publication approval.
    pub fn prepare_stage<S, C>(
        &self,
        stage: &S,
        store: &C,
        request: PreparationRequest<'_>,
        options: PreparationOptions,
    ) -> Result<store::PreparationReceipt, PreparationError>
    where
        S: StagePlanner,
        C: WriterConnector,
        C::Writer: PreparationWriter,
    {
        self.retained_preparation(options)?
            .prepare_stage(stage, store, request, options)
    }
}

impl RetainedPreparation<'_> {
    /// Bound the whole retained tree and evidence before acquiring a native target.
    pub(super) fn plan<S: StagePlanner>(
        self,
        stage: &S,
        request: PreparationRequest<'_>,
        options: PreparationOptions,
    ) -> Result<PreparationPlan<S::Target>, PreparationError> {
        let before = PreparationError::before;
        options.validate()?;
        let mut bounded = source::InventoryBuilder::new(options.output);
        for entry in self.output.inventory().entries() {
            bounded
                .insert(entry.path.clone(), entry.content)
                .map_err(|e| before(PreparationFailure::Stage(e.into())))?;
        }
        if self.evidence.len() > options.evidence_bytes {
            return Err(before(PreparationFailure::Evidence(
                gazette_core::render::Error::Limit("preparation evidence"),
            )));
        }
        stage
            .check_root_separation(request.journal, self.protected_roots)
            .map_err(|e| before(PreparationFailure::Separation(e)))?;
        let mut protected = self.protected_roots.to_vec();
        protected.push(request.journal.to_owned());
        let target = stage
            .plan_stage(request.parent, request.name, &protected)
            .map_err(|e| before(PreparationFailure::Stage(e)))?;
        Ok(PreparationPlan {
            target,
            evidence: self.evidence,
        })
    }

    /// The sole intent, native stage and retention sequence for already retained bytes.
    pub(super) fn prepare_stage<S, C>(
        self,
        stage: &S,
        store: &C,
        request: PreparationRequest<'_>,
        options: PreparationOptions,
    ) -> Result<store::PreparationReceipt, PreparationError>
    where
        S: StagePlanner,
        C: WriterConnector,
        C::Writer: PreparationWriter,
    {
        let before = PreparationError::before;
        let output = self.output;
        let destination = self.destination;
        let PreparationPlan { target, evidence } = self.plan(stage, request, options)?;
        let inventory = output.inventory();
        let address = target.address().clone();
        let path = address.path();
        let input = store::PreparationInput {
            id: request.id,
            address: &address,
            destination,
            output: inventory.identity(),
            entries: inventory.entries().len() as u64,
            bytes: inventory.total_bytes(),
            evidence: &evidence,
            evidence_sha256: Sha256Digest::of(evidence.as_bytes()),
        };
        let mut writer = store
            .acquire_writer(request.journal, request.expected, options.writer)
            .map_err(|e| before(PreparationFailure::Writer(e)))?;
        let acquired = writer
            .inspect()
            .map_err(|e| PreparationError {
                cause: Box::new(PreparationFailure::Journal(e)),
                receipt: None,
                journal: None,
                writer_acquired: true,
                stage: None,
            })?
            .header;
        let mut receipt = writer
            .record_preparation_intent(input, options.record)
            .map_err(|e| PreparationError {
                cause: Box::new(PreparationFailure::Record(e)),
                receipt: None,
                journal: Some(acquired),
                writer_acquired: true,
                stage: Some(path.clone()),
            })?;
        let result = (|| {
            if receipt.record.intent.as_input() != input {
                return Err(PreparationFailure::Mismatch);
            }
            if !receipt.inserted || receipt.record.retention.is_some() {
                return Err(PreparationFailure::Replay);
            }
            writer.inspect().map_err(PreparationFailure::Journal)?;
            let retained = target
                .create(output.as_input(), options.output)
                .map_err(PreparationFailure::Stage)?
                .retain()
                .map_err(PreparationFailure::Stage)?;
            if retained.path() != path || retained.inventory() != inventory {
                return Err(PreparationFailure::Mismatch);
            }
            writer.inspect().map_err(PreparationFailure::Journal)?;
            let next = writer
                .record_preparation_retention(
                    store::PreparationRetentionInput {
                        intent: &receipt.record.intent,
                        output: retained.inventory().identity(),
                        entries: retained.inventory().entries().len() as u64,
                        bytes: retained.inventory().total_bytes(),
                    },
                    options.record,
                )
                .map_err(PreparationFailure::Record)?;
            if next.record.intent != receipt.record.intent
                || next.record.retention.as_ref().is_none_or(|r| {
                    r.intent_id != request.id
                        || r.output != input.output
                        || r.entries != input.entries
                        || r.bytes != input.bytes
                })
            {
                return Err(PreparationFailure::Mismatch);
            }
            receipt = next;
            writer.close().map_err(PreparationFailure::Journal)
        })();
        match result {
            Ok(()) => Ok(receipt),
            Err(cause) => Err(PreparationError {
                cause: Box::new(cause),
                journal: Some(receipt.journal),
                receipt: Some(Box::new(receipt)),
                writer_acquired: true,
                stage: Some(path),
            }),
        }
    }
}
