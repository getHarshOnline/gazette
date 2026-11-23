//! Complete local rendering request shared by CLI, preview and protocol callers.

use super::{PublicationRecipe, ReviewSubmission, SiteRecipe, WorkspaceInput};
use crate::{authored, source, workspace::Key};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

mod documentation;
pub use documentation::DocumentationRenderExecution;

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
enum Format {
    #[serde(rename = "gazette-local-publication-set/1")]
    Local,
}

/// Serving-address projection, independent from private-input consent and listener effects.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum PublicationAddressing {
    /// Use the exact configured public addresses; a local render is still private output.
    #[default]
    Public,
    /// Project every selected site to its configured loopback port without starting servers.
    Local,
}

#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    format: Format,
    workspace: WorkspaceInput,
    /// Omission selects the workspace's declared build defaults after resolution.
    /// An explicit site list is an invocation override, never a fallback trigger.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Vec<SiteRecipe>")]
    sites: Option<Vec<SiteRecipe>>,
    #[serde(default)]
    addressing: PublicationAddressing,
    /// Preview-only public hyperlink bases for a separately held local output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "authored::RenderYear")]
    render_year: Option<authored::RenderYear>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "authored::Selection")]
    selection: Option<authored::Selection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Reserved checked-render transport; this unguarded local operation refuses its presence.
    #[schemars(schema_with = "super::recipe::review_responses_schema")]
    review_responses: Option<BTreeMap<Key, ReviewSubmission>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::MathGeneration")]
    math_generation: Option<super::MathGeneration>,
    /// Override workspace math selection without granting execution permission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "bool")]
    math: Option<bool>,
    /// Permission for this invocation's configured generation processes; never workspace authority.
    #[serde(default)]
    allow_execute: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::DiagramGeneration")]
    diagram_generation: Option<super::DiagramGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::SocialCardGeneration")]
    social_card_generation: Option<super::SocialCardGeneration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::DocumentationGenerationRequest")]
    documentation_generation: Option<super::DocumentationGenerationRequest>,
}

/// Complete owned local request selecting source acquisition and rendering.
/// Explicit documentation generation can retain private native stages through the delivery callback.
/// The recipe does not grant admission, preview listeners or release effects.
#[derive(Debug)]
pub struct LocalPublicationRecipe {
    workspace: WorkspaceInput,
    publication: PublicationRecipe<Option<Vec<SiteRecipe>>>,
    maximum_sites: usize,
    addressing: PublicationAddressing,
    math: Option<bool>,
    allow_execute: bool,
}

/// One retained workspace and its exact effective local build intent.
/// Resolution reads only the selected configuration; content acquisition remains separate.
#[derive(Debug)]
pub struct ResolvedLocalPublicationRecipe {
    pub(crate) configuration: gazette_application::configuration::ResolvedWorkspace,
    publication: PublicationRecipe,
    build_selection: crate::workspace::BuildSelection,
    addressing: PublicationAddressing,
}

impl LocalPublicationRecipe {
    /// Select the existing documentation operation's explicit staging and execution request.
    ///
    /// # Errors
    /// Rejects invalid native selection without source or stage effects.
    pub fn with_documentation_generation(
        mut self,
        request: Option<super::DocumentationGenerationRequest>,
    ) -> Result<Self, &'static str> {
        self.publication = self.publication.with_documentation_generation(request)?;
        Ok(self)
    }
    /// Explicit documentation generation request, before inherited invocation execution permission.
    pub fn documentation_generation(&self) -> Option<&super::DocumentationGenerationRequest> {
        self.publication.documentation_generation()
    }
    /// Carry validated native documentation results into the resolved default or explicit site set.
    ///
    /// # Errors
    /// Rejects duplicate supplied owners; acquisition verifies exact configuration and sources.
    pub fn with_documentation_catalogs(
        mut self,
        catalogs: Vec<std::sync::Arc<super::DocumentationCatalog>>,
    ) -> Result<Self, &'static str> {
        self.publication = self.publication.with_documentation_catalogs(catalogs)?;
        Ok(self)
    }

    /// Apply the math switch and shared native generation permission.
    /// An omitted math switch preserves the request's choice.
    ///
    /// # Errors
    /// Rejects an off switch paired with an explicit generation request.
    pub fn with_math_execution(
        mut self,
        math: Option<bool>,
        allow_execute: bool,
    ) -> Result<Self, &'static str> {
        if math == Some(false) && self.publication.math_generation().is_some() {
            return Err("math=false conflicts with explicit math_generation");
        }
        self.math = math.or(self.math);
        self.allow_execute |= allow_execute;
        Ok(self)
    }

    /// Whether this request permits a configured math process; opens no workspace.
    pub fn allows_math_execution(&self) -> bool {
        self.allow_execute
            || self
                .publication
                .math_generation()
                .is_some_and(|options| options.execution.allow_execute)
    }

    /// Whether this invocation permits any configured generation process.
    /// This classifies effects without loading the workspace or granting permission.
    pub fn allows_generation_execution(&self) -> bool {
        self.allows_math_execution()
            || self
                .publication
                .documentation_generation()
                .is_some_and(|request| request.allow_execute)
            || self
                .publication
                .social_card_generation()
                .is_some_and(|options| options.execution.allow_execute)
    }

    /// Add an explicit local documentation link map without editing the input request.
    ///
    /// # Errors
    /// Refuses a second map rather than silently replacing the request's destinations.
    pub fn with_preview_link_bases(
        mut self,
        bases: gazette_core::publication::routing::PreviewLinkBases,
    ) -> Result<Self, &'static str> {
        self.publication = self.publication.add_preview_link_bases(bases)?;
        Ok(self)
    }

    /// Apply boundary-captured context only when the caller supplied no year.
    pub fn with_default_render_year(mut self, year: authored::RenderYear) -> Self {
        self.publication = self.publication.with_default_render_year(year);
        self
    }
    /// Explicit native diagram consent and limits, not provider configuration.
    pub fn diagram_generation(&self) -> Option<super::DiagramGenerationOptions> {
        self.publication.diagram_generation()
    }
    /// Capture selected native diagrams before rendering retained inputs.
    ///
    /// # Errors
    /// Preserves original process/capture evidence without retry.
    pub fn acquire_diagrams(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        corpus: crate::corpus::Limits,
    ) -> Result<(), super::DiagramGenerationError> {
        self.publication.acquire_diagrams(inputs, corpus)
    }
    /// Discover the native named request, including omission versus selection.
    /// Duplicate keys, count/byte ceilings and semantic closure remain native checks.
    pub fn input_schema() -> schemars::Schema {
        schemars::generate::SchemaSettings::draft2020_12()
            .with(|settings| settings.inline_subschemas = true)
            .into_generator()
            .into_root_schema_for::<Input>()
    }

    /// Decode the entire private request before opening any configured source.
    /// Omitted selection means unfiltered local rendering; explicit null does not.
    /// Review responses require a checked operation; this local renderer has no checker hook.
    /// Omitted sites select configured defaults later; explicit null or empty sites fail.
    ///
    /// # Errors
    /// Rejects excess bytes, duplicate keys, positional/unknown fields, invalid
    /// native identities/paths and empty, duplicated or oversized site selections.
    pub fn parse_json(bytes: &[u8], maximum: usize, sites: usize) -> Result<Self, &'static str> {
        if bytes.len() > maximum {
            return Err("local publication request exceeds input byte ceiling");
        }
        if sites == 0 {
            return Err("local publication site-count ceiling must be nonzero");
        }
        let invalid = "expected exact gazette-local-publication-set/1 JSON with workspace, site options and optional selection; inspect private input";
        let value = crate::json::parse(bytes).map_err(|_| invalid)?;
        // Serde structs also accept positional arrays. Reject them at each named
        // object boundary before the existing typed validators consume the values.
        if !value.is_object()
            || !super::recipe::review_response_shape(&value)
            || value
                .get("render_year")
                .is_some_and(|value| !value.is_u64())
            || !value.get("format").is_some_and(Value::is_string)
            || value.get("selection").is_some_and(|v| !v.is_object())
            || value.get("math").is_some_and(|v| !v.is_boolean())
            || value.get("math_generation").is_some_and(|v| !v.is_object())
            || value
                .get("diagram_generation")
                .is_some_and(|v| !v.is_object())
            || value
                .get("documentation_generation")
                .is_some_and(|value| !value.is_object())
            || value
                .get("social_card_generation")
                .is_some_and(|v| !v.is_object())
            || value.get("sites").is_some_and(|sites| {
                !sites.as_array().is_some_and(|values| {
                    values.iter().all(|site| {
                        site.is_object()
                            && !site
                                .get("graph")
                                .is_some_and(|v| !v.is_null() && !v.is_object())
                    })
                })
            })
        {
            return Err(invalid);
        }
        let input: Input = serde_json::from_value(value).map_err(|_| invalid)?;
        if input.review_responses.is_some() {
            return Err(
                "local rendering has no candidate checker; review responses require checked preparation",
            );
        }
        if input.math == Some(false) && input.math_generation.is_some() {
            return Err("math=false conflicts with explicit math_generation");
        }
        let publication = match input.sites {
            Some(selected) => {
                PublicationRecipe::new(selected, input.selection, input.review_responses, sites)?
                    .map_sites(Some)
            }
            None => PublicationRecipe::configured(input.selection),
        }
        .with_render_year(input.render_year)
        .with_math_generation(input.math_generation)?
        .with_diagram_generation(input.diagram_generation)?
        .with_social_card_generation(input.social_card_generation)?
        .with_documentation_generation(input.documentation_generation)?
        .with_preview_link_bases(input.preview_link_bases);
        Ok(Self {
            workspace: input.workspace,
            addressing: input.addressing,
            publication,
            maximum_sites: sites,
            math: input.math,
            allow_execute: input.allow_execute,
        })
    }

    /// Exact supplied or local workspace selection; this getter performs no reads.
    pub fn workspace(&self) -> &WorkspaceInput {
        &self.workspace
    }

    /// Selected address projection; this grants no source, output or listener permission.
    pub fn addressing(&self) -> PublicationAddressing {
        self.addressing
    }

    /// Resolve defaults or an exact override against one retained workspace.
    /// Caller permission must precede this operation when a native workspace was selected.
    ///
    /// # Errors
    /// Refuses invalid configuration, absent defaults and invalid or oversized effective sets.
    pub fn resolve(
        &self,
        limits: crate::toml::Limits,
    ) -> Result<ResolvedLocalPublicationRecipe, super::PublicationSetError> {
        let configuration = self
            .workspace
            .resolve(limits)
            .map_err(super::PublicationSetError::Configuration)?;
        let invocation = self.publication.invocation().map(|sites| {
            sites
                .iter()
                .map(|site| site.site.clone())
                .collect::<Vec<_>>()
        });
        let build_selection = configuration
            .compilation()
            .workspace
            .build_selection(invocation.as_deref())
            .map_err(super::PublicationSetError::Selection)?;
        if build_selection.sites.len() > self.maximum_sites {
            return Err(super::PublicationSetError::Selection(
                "effective site set exceeds its site-count ceiling",
            ));
        }
        Ok(ResolvedLocalPublicationRecipe {
            publication: self
                .publication
                .resolve_sites(&build_selection)
                .resolve_math(
                    &configuration.compilation().workspace,
                    self.math,
                    self.allow_execute,
                )
                .resolve_social_cards(&configuration.compilation().workspace, self.allow_execute)
                .resolve_documentation_permission(self.allow_execute),
            configuration,
            build_selection,
            addressing: self.addressing,
        })
    }

    /// Explicit override only. Absence is unresolved workspace intent, not an empty site set.
    /// This getter opens no configuration or content sources.
    pub fn invocation_sites(&self) -> Option<&[SiteRecipe]> {
        self.publication.invocation()
    }

    /// Explicit native typesetting effects, independent of workspace command selection.
    pub fn math_generation(&self) -> Option<super::MathGenerationOptions> {
        self.publication.math_generation()
    }

    /// Selected browser capture permission and bounds, separate from executable pins.
    pub fn social_card_generation(&self) -> Option<super::SocialCardGenerationOptions> {
        self.publication.social_card_generation()
    }

    /// Capture selected native typesetting before rendering this retained input set.
    ///
    /// # Errors
    /// Preserves complete native generation failures without retry or fallback.
    pub fn acquire_math(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        corpus: crate::corpus::Limits,
    ) -> Result<(), super::MathGenerationError> {
        self.publication.acquire_math(inputs, corpus)
    }

    /// Acquire publication and documentation captures, render locally, then consume the
    /// borrowed result before releasing its retained inputs and generated bytes.
    /// Caller permission and transport encoding remain outside this operation.
    ///
    /// # Errors
    /// Returns shared acquisition or rendering failures. Site-local failures stay
    /// in the result; consumers must inspect `is_complete`, not silently shrink it.
    pub fn with_rendered<T>(
        &self,
        syntax: crate::toml::Limits,
        acquisition: super::PublicationSetAcquisitionLimits,
        output: source::Limits,
        render: super::LocalRenderLimits,
        consume: impl FnOnce(&super::RenderedPublicationSet<'_>) -> T,
    ) -> Result<T, super::PublicationSetError> {
        self.resolve(syntax)?
            .with_rendered(acquisition, output, render, consume)
    }
}

impl ResolvedLocalPublicationRecipe {
    /// Validated captures supplied independently by the existing documentation operation.
    pub fn documentation_catalogs(
        &self,
    ) -> &BTreeMap<Key, std::sync::Arc<super::DocumentationCatalog>> {
        self.publication.documentation_catalogs()
    }
    /// Capture and assemble configured documentation without rereading configuration.
    ///
    /// # Errors
    /// Preserves native documentation failure and required generation without an incomplete success.
    pub fn acquire_documentation_catalogs(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        acquisition: crate::source::PreparationLimits,
        output: crate::source::Limits,
    ) -> Result<(), super::PublicationSetError> {
        self.publication
            .acquire_documentation_catalogs(inputs, acquisition, output)
    }

    /// Resolve invocation limits against this already loaded configuration.
    pub fn render_limits(
        &self,
        limits: super::LocalRenderLimits,
    ) -> super::PublicationSetRenderLimits {
        super::settings::resolve(limits, &self.configuration.compilation().workspace)
    }

    /// Effective math selection and per-call permission after workspace resolution.
    pub fn math_generation(&self) -> Option<super::MathGenerationOptions> {
        self.publication.math_generation()
    }

    /// Selected browser capture permission and bounds, separate from executable pins.
    pub fn social_card_generation(&self) -> Option<super::SocialCardGenerationOptions> {
        self.publication.social_card_generation()
    }

    /// Acquire configured math from the exact retained inputs.
    ///
    /// # Errors
    /// Preserves execution refusals and incomplete capture failures.
    pub fn acquire_math(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        corpus: crate::corpus::Limits,
    ) -> Result<(), super::MathGenerationError> {
        self.publication.acquire_math(inputs, corpus)
    }

    /// Exact selected identities in declared order after configuration resolution.
    pub fn sites(&self) -> impl Iterator<Item = &Key> {
        self.publication.sites()
    }

    /// Selection owner and local draft policy, retained without invocation relabelling.
    pub fn build_selection(&self) -> &crate::workspace::BuildSelection {
        &self.build_selection
    }

    /// Borrow the one resolved set's native render choices.
    pub fn options(&self, limits: source::Limits) -> super::PublicationSetRenderOptions<'_> {
        self.publication.options(limits)
    }

    /// Acquire and render the exact resolved set; configuration is not read again.
    ///
    /// # Errors
    /// Preserves acquisition/provider failures and explicit per-site rendering outcomes.
    pub fn with_rendered<T>(
        &self,
        acquisition: super::PublicationSetAcquisitionLimits,
        output: source::Limits,
        render: super::LocalRenderLimits,
        consume: impl FnOnce(&super::RenderedPublicationSet<'_>) -> T,
    ) -> Result<T, super::PublicationSetError> {
        if self.documentation_generation().is_some() {
            return Err(super::PublicationSetError::Selection(
                "explicit documentation generation requires the native delivery callback",
            ));
        }
        self.with_rendered_documentation(
            acquisition,
            output,
            render,
            |execution| match execution {
                DocumentationRenderExecution::Rendered { rendered, .. } => Ok(consume(rendered)),
                DocumentationRenderExecution::Refused {
                    error: super::DocumentationPublicationFailure::Publication(error),
                    ..
                } => Err(error),
                _ => Err(super::PublicationSetError::Selection(
                    "documentation operation requires its native delivery consumer",
                )),
            },
        )?
    }
}
