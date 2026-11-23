//! Capability status is semantic provider evidence, never a resource-selection shortcut.

use std::collections::BTreeMap;

use gazette_compiler::workspace::{CapabilitySelection, Requirement, SelectionOrigin};
use gazette_core::publication::{
    CapabilityId,
    capability_runtime::{
        CapabilityAbsence, CapabilityResultOutcome, CapabilityValue, ProviderEvidence, ProviderRef,
        ProviderRuntimeIdentity, SearchIndexFormat,
    },
};
use serde::Serialize;

use super::{CompositionMetadata, PublicationRenderError};

/// One explicit provider result. No package resource or semantic value is inferred.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CapabilityProjection {
    /// The provider returned a typed value admitted by Gazette.
    Available {
        /// Gazette-owned value; provider evidence remains private.
        value: CapabilityValue,
    },
    /// The provider was selected, but no admitted runtime produced a value.
    Unavailable {
        /// Exact configured absence policy matching the missing runtime.
        reason: CapabilityAbsence,
    },
}

/// Private evidence for one unavailable provider selection.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct CapabilityEvidence {
    /// Exact owner-qualified provider selection.
    pub provider: ProviderRef,
    /// Exact admitted runtime distribution identity; never public Theme/model data.
    pub runtime: ProviderRuntimeIdentity,
    /// Winning workspace/site declaration atom.
    pub selection: SelectionOrigin,
    /// Requested failure policy.
    pub requirement: Requirement,
    /// Exact unavailable result; a package declaration is not execution evidence.
    #[serde(flatten)]
    pub outcome: CapabilityOutcome,
}

/// Capability execution outcome retained separately from resource compilation.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CapabilityOutcome {
    /// Provider-native coordinates retained outside public Theme/model data.
    Available {
        /// Exact source/revision evidence from the admitted snapshot.
        evidence: ProviderEvidence,
    },
    /// No admitted Gazette or plugin provider runtime exists for this build.
    Unavailable {
        /// Exact configured and observed unavailable condition.
        reason: CapabilityAbsence,
    },
}

#[derive(Default)]
pub(super) struct ResolvedCapabilities {
    pub model: BTreeMap<CapabilityId, CapabilityProjection>,
    pub evidence: BTreeMap<CapabilityId, CapabilityEvidence>,
}

impl CompositionMetadata<'_> {
    pub(super) fn resolve_capabilities(
        &self,
        metadata: &mut gazette_core::render::LimitedWriter<std::io::Sink>,
    ) -> Result<ResolvedCapabilities, PublicationRenderError> {
        let composition = self
            .acquired
            .compilation()
            .workspace
            .composition_for(self.acquired.site())
            .ok_or_else(|| PublicationRenderError::Binding(self.acquired.site().clone()))?;
        let mut resolved = ResolvedCapabilities::default();
        for (capability, selected) in composition.capabilities() {
            let CapabilitySelection::Enabled {
                provider,
                requirement,
                allow_absence,
            } = selected.value()
            else {
                continue;
            };
            let outcome = match self.acquired.capability_capture() {
                Some(capture) => capture
                    .result(provider, capability)
                    .ok_or_else(|| PublicationRenderError::Capability(capability.clone()))?
                    .outcome
                    .clone(),
                None => CapabilityResultOutcome::Unavailable {
                    reason: CapabilityAbsence::NoNativeAcquisition,
                },
            };
            let (projection, evidence) = match outcome {
                CapabilityResultOutcome::Available { value, evidence } => {
                    charge_metadata(
                        metadata,
                        &(
                            capability,
                            provider,
                            selected.origin(),
                            requirement,
                            &value,
                            &evidence,
                        ),
                    )?;
                    (
                        CapabilityProjection::Available { value },
                        CapabilityOutcome::Available { evidence },
                    )
                }
                CapabilityResultOutcome::Unavailable { reason } => {
                    if *requirement != Requirement::Optional || *allow_absence != Some(reason) {
                        return Err(PublicationRenderError::Capability(capability.clone()));
                    }
                    charge_metadata(
                        metadata,
                        &(capability, provider, selected.origin(), requirement, reason),
                    )?;
                    (
                        CapabilityProjection::Unavailable { reason },
                        CapabilityOutcome::Unavailable { reason },
                    )
                }
            };
            resolved.model.insert(capability.clone(), projection);
            resolved.evidence.insert(
                capability.clone(),
                CapabilityEvidence {
                    provider: provider.clone(),
                    runtime: self
                        .provider_runtime_identity(provider)
                        .ok_or_else(|| PublicationRenderError::Capability(capability.clone()))?,
                    selection: selected.origin().clone(),
                    requirement: *requirement,
                    outcome: evidence,
                },
            );
        }
        Ok(resolved)
    }
}

impl ResolvedCapabilities {
    pub(super) fn validate_search(
        &self,
        search: Option<&super::search::Search>,
    ) -> Result<(), PublicationRenderError> {
        let capability: CapabilityId = "publication.search/v1"
            .parse()
            .expect("static capability identity");
        let Some(CapabilityProjection::Available {
            value:
                CapabilityValue::PublicationSearchV1 {
                    index,
                    format: SearchIndexFormat::GazetteDocumentsV1,
                },
        }) = self.model.get(&capability)
        else {
            return Ok(());
        };
        if search.is_some_and(|search| search.route == *index) {
            Ok(())
        } else {
            Err(PublicationRenderError::Capability(capability))
        }
    }
}

pub(super) fn charge_metadata(
    metadata: &mut gazette_core::render::LimitedWriter<std::io::Sink>,
    facts: &impl Serialize,
) -> Result<(), PublicationRenderError> {
    let encoded = serde_json::to_writer(&mut *metadata, facts);
    if metadata.exceeded() {
        return Err(PublicationRenderError::Resource(
            gazette_core::resource::Error::Limit("resource metadata bytes"),
        ));
    }
    encoded.map_err(|error| {
        PublicationRenderError::Resource(gazette_core::resource::Error::Compile(error.to_string()))
    })
}
