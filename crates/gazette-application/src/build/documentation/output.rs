//! Configured documentation destinations use the same retained output and history owner.

use std::path::PathBuf;

use gazette_core::{
    render,
    source::{Limits, TreeIdentity},
};
use gazette_ports::{destination::OutputReader, store::MaterializationWriter};
use serde::Serialize;

use super::{DocumentationArtifactEvidence, DocumentationAssembly, DocumentationManualEvidence};
use crate::build::{
    MaterializationError, MaterializationPlan, MaterializedOutput, OutputPlan, OutputPlanError,
    SourceEvidence,
    output::{PreviousOutput, RetainedOutput},
    serialization,
};

impl DocumentationAssembly<'_> {
    fn retained_output(&self) -> Result<RetainedOutput<'_>, OutputPlanError> {
        if self.preview_link_bases().is_some() {
            return Err(OutputPlanError::Preview);
        }
        let selected = self.repository.path.parse().map_err(|_| {
            OutputPlanError::Output(gazette_core::source::Error::InvalidInventory {
                path: self.repository.path.clone().into(),
                reason: "retained documentation destination no longer matches the admitted root grammar",
            })
        })?;
        Ok(RetainedOutput {
            site: self.inputs.site(),
            root: self
                .inputs
                .resolve_root(&selected)
                .map_err(OutputPlanError::Coordinate)?,
            generated: &self.output,
            protected: self.protected_roots().map_err(OutputPlanError::Output)?,
        })
    }

    /// Plan this complete retained documentation target at its brand-owned destination.
    /// The directory must exist. A fresh assembly or equal bytes grants no replacement rights.
    /// No generator, source reread, stage creation, certification or release occurs.
    ///
    /// # Errors
    /// Refuses detached relative coordinates, source overlap, changed native ownership,
    /// unowned collisions, unsafe paths or complete output bounds.
    pub fn plan_output<'a>(
        &'a self,
        reader: &impl OutputReader,
        previous: Option<&MaterializedOutput>,
        limits: Limits,
    ) -> Result<OutputPlan<'a>, OutputPlanError> {
        self.retained_output()?.plan(
            reader,
            previous.map(PreviousOutput::from_materialized),
            limits,
            &[],
        )
    }

    /// Bind documentation output to the held journal's independently completed ownership head.
    /// Actual configuration/manual/reference inputs and held journal roots remain protected.
    /// The eventual shared materializer never reruns generators or reopens authored sources.
    ///
    /// # Errors
    /// Refuses pending or changed ownership, unsafe destinations and bounded evidence failure.
    pub fn plan_materialization<'a>(
        &'a self,
        history: &mut impl MaterializationWriter,
        destination: &impl OutputReader,
        protected: &[PathBuf],
        limits: Limits,
        evidence_bytes: usize,
    ) -> Result<MaterializationPlan<'a>, MaterializationError> {
        self.retained_output()
            .map_err(MaterializationError::Plan)?
            .plan_materialization(history, destination, protected, limits, || {
                self.materialization_evidence(evidence_bytes)
            })
    }

    fn materialization_evidence(&self, maximum: usize) -> Result<Vec<u8>, render::Error> {
        #[derive(Serialize)]
        struct Evidence<'a> {
            format: &'static str,
            artifact_class: &'static str,
            configuration: &'a crate::configuration::Evidence,
            site: &'a gazette_core::publication::Key,
            brand: &'a gazette_core::publication::Key,
            repository: &'a gazette_compiler::workspace::DocumentationRepository,
            sources:
                std::collections::BTreeMap<&'a gazette_core::publication::Key, SourceEvidence<'a>>,
            manual: &'a [DocumentationManualEvidence<'a>],
            artifacts: &'a [DocumentationArtifactEvidence<'a>],
            output: TreeIdentity,
        }
        serialization::encode(
            &Evidence {
                format: "gazette.documentation-materialization/1",
                artifact_class: "documentation-build-input",
                configuration: self.inputs.configuration_evidence(),
                site: self.inputs.site(),
                brand: self.brand,
                repository: self.repository,
                sources: self.inputs.source_evidence(),
                manual: &self.manual,
                artifacts: &self.artifacts,
                output: self.output.inventory().identity(),
            },
            maximum,
            "documentation materialization evidence bytes",
        )
    }
}
