//! Owned local rendering choices shared by command and protocol interfaces.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{GraphRenderOptions, PublicationSetRenderOptions, ReviewSubmission, SiteRenderOptions};
use crate::{
    authored,
    configuration::WorkspaceLoadLimits,
    source::{EntryPath, Limits},
    workspace::Key,
};

/// One explicit workspace source. Neither variant performs discovery or fallback.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceInput {
    /// One complete caller-supplied workspace document.
    Supplied {
        /// Exact TOML bytes carried by the request.
        source: String,
    },
    /// One strict local root document plus its exact declared include set.
    Local {
        /// Explicit absolute native root; never inferred from the process directory.
        root: PathBuf,
        /// Root-relative workspace document selected within `root`.
        workspace: EntryPath,
        /// Independent native capture and complete document-set ceilings.
        #[serde(default)]
        limits: WorkspaceLoadLimits,
    },
}

impl WorkspaceInput {
    pub(super) fn resolve(
        &self,
        supplied_limits: crate::toml::Limits,
    ) -> Result<
        gazette_application::configuration::ResolvedWorkspace,
        crate::configuration::WorkspaceLoadError,
    > {
        match self {
            Self::Supplied { source } => {
                gazette_application::configuration::resolve(source, supplied_limits)
            }
            Self::Local {
                root,
                workspace,
                limits,
            } => gazette_application::configuration::load(
                &gazette_adapter_fs::FileSystem,
                root,
                workspace,
                *limits,
            )
            .map(gazette_application::configuration::LoadedWorkspace::into_resolved),
        }
    }
}

/// One site's output destinations, not publication admission.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SiteRecipe {
    /// Exact configured site identity.
    pub site: Key,
    /// Optional root-relative debug model artifact. The shared model exists even
    /// when no standalone serialization is requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_output: Option<EntryPath>,
    /// Optional graph output destination; the Theme owns its render entry.
    pub graph: Option<GraphRecipe>,
}

/// Optional graph rendering uses the same resolved publication model.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphRecipe {
    /// Exact root-relative graph destination.
    pub output: EntryPath,
}

/// Validated explicit site choices. Compiler-owned selection remains a candidate,
/// and preview addressing is deliberately not inferred by this local recipe.
#[derive(Clone, Debug)]
pub struct PublicationRecipe<Sites = Vec<SiteRecipe>> {
    sites: Sites,
    documentation_catalogs: BTreeMap<Key, Arc<super::DocumentationCatalog>>,
    documentation_generation: Option<super::DocumentationGenerationRequest>,
    render_year: Option<authored::RenderYear>,
    selection: Option<authored::Selection>,
    review_responses: Option<BTreeMap<Key, ReviewSubmission>>,
    math_generation: Option<super::MathGeneration>,
    diagram_generation: Option<super::DiagramGeneration>,
    social_card_generation: Option<super::SocialCardGeneration>,
    preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
}

impl<Sites> PublicationRecipe<Sites> {
    /// Select explicit retained staging and native documentation generation.
    ///
    /// # Errors
    /// Refuses invalid paths and bounds without acquiring or executing anything.
    pub fn with_documentation_generation(
        mut self,
        request: Option<super::DocumentationGenerationRequest>,
    ) -> Result<Self, &'static str> {
        if let Some(request) = &request {
            request.validate()?;
        }
        self.documentation_generation = request;
        Ok(self)
    }
    /// Explicit generation selection; configured recipes alone never imply execution.
    pub fn documentation_generation(&self) -> Option<&super::DocumentationGenerationRequest> {
        self.documentation_generation.as_ref()
    }
    pub(super) fn restrict_documentation_delivery(
        &mut self,
        maximum: usize,
        allow_execute: bool,
    ) -> Result<(), &'static str> {
        if let Some(request) = &mut self.documentation_generation {
            if maximum == 0 {
                return Err(
                    "documentation custody cannot fit the transport response bound; no stages created",
                );
            }
            request.response_bytes = request.response_bytes.min(maximum);
            request.allow_execute &= allow_execute;
        }
        Ok(())
    }
    pub(super) fn resolve_documentation_permission(mut self, allow_execute: bool) -> Self {
        if let Some(request) = &mut self.documentation_generation {
            request.allow_execute |= allow_execute;
        }
        self
    }
    pub(super) fn documentation_preview_links(
        &self,
    ) -> Option<&gazette_core::publication::routing::PreviewLinkBases> {
        self.preview_link_bases.as_ref()
    }
    /// Reuse validated results from the existing native documentation operation.
    /// These captures grant no new read, staging or execution permission.
    ///
    /// # Errors
    /// Rejects duplicate supplied owners before native acquisition; binding is checked after resolution.
    pub fn with_documentation_catalogs(
        mut self,
        catalogs: Vec<Arc<super::DocumentationCatalog>>,
    ) -> Result<Self, &'static str> {
        let mut selected = BTreeMap::new();
        for catalog in catalogs {
            if selected.insert(catalog.site().clone(), catalog).is_some() {
                return Err("duplicate supplied documentation catalog owner");
            }
        }
        self.documentation_catalogs = selected;
        Ok(self)
    }
    /// Explicit immutable documentation results retained for the exact configured owners.
    pub fn documentation_catalogs(&self) -> &BTreeMap<Key, Arc<super::DocumentationCatalog>> {
        &self.documentation_catalogs
    }
    /// Capture and assemble every configured documentation target before native publication rendering.
    /// Imported references need only read permission; configured generation requires a supplied result.
    ///
    /// # Errors
    /// Preserves native capture, missing generation, stale supplied catalog and complete-set bounds.
    pub fn acquire_documentation_catalogs(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        acquisition: crate::source::PreparationLimits,
        output: crate::source::Limits,
    ) -> Result<(), super::PublicationSetError> {
        super::documentation_catalog::acquire(
            inputs,
            &self.documentation_catalogs,
            acquisition,
            output,
            self.preview_link_bases.as_ref(),
        )
    }

    /// Select bounded browser capture independently from workspace executable pins.
    ///
    /// # Errors
    /// Rejects invalid effect limits before any acquisition or native process.
    pub fn with_social_card_generation(
        mut self,
        generation: Option<super::SocialCardGeneration>,
    ) -> Result<Self, &'static str> {
        if let Some(selected) = generation {
            selected.validate()?;
        }
        self.social_card_generation = generation;
        Ok(self)
    }

    pub(super) fn resolve_social_cards(
        mut self,
        workspace: &crate::workspace::Workspace,
        allow_execute: bool,
    ) -> Self {
        self.social_card_generation = super::SocialCardGeneration::resolve(
            workspace,
            self.social_card_generation,
            allow_execute,
        );
        self
    }

    /// Exact explicit browser acquisition choices, or no browser effects.
    pub fn social_card_generation(&self) -> Option<super::SocialCardGenerationOptions> {
        self.social_card_generation
            .map(super::SocialCardGeneration::options)
    }

    pub(super) fn add_preview_link_bases(
        self,
        bases: gazette_core::publication::routing::PreviewLinkBases,
    ) -> Result<Self, &'static str> {
        if self.preview_link_bases.is_some() {
            return Err("preview link map is already supplied in the request");
        }
        Ok(self.with_preview_link_bases(Some(bases)))
    }

    /// Select bounded ordinary hyperlinks for a local preview environment.
    pub fn with_preview_link_bases(
        mut self,
        bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
    ) -> Self {
        self.preview_link_bases = bases;
        self
    }
    /// Select explicit rendering context without deriving editorial chronology.
    pub fn with_render_year(mut self, year: Option<authored::RenderYear>) -> Self {
        self.render_year = year;
        self
    }
    /// Use a boundary-captured year only when the request supplied none.
    pub fn with_default_render_year(mut self, year: authored::RenderYear) -> Self {
        self.render_year.get_or_insert(year);
        self
    }
    pub(super) fn map_sites<T>(self, map: impl FnOnce(Sites) -> T) -> PublicationRecipe<T> {
        PublicationRecipe {
            sites: map(self.sites),
            documentation_catalogs: self.documentation_catalogs,
            documentation_generation: self.documentation_generation,
            render_year: self.render_year,
            selection: self.selection,
            review_responses: self.review_responses,
            math_generation: self.math_generation,
            diagram_generation: self.diagram_generation,
            social_card_generation: self.social_card_generation,
            preview_link_bases: self.preview_link_bases,
        }
    }

    /// Select bounded diagram effects independently from workspace renderer configuration.
    ///
    /// # Errors
    /// Refuses invalid whole-operation bounds before acquisition or execution.
    pub fn with_diagram_generation(
        mut self,
        selection: Option<super::DiagramGeneration>,
    ) -> Result<Self, &'static str> {
        if let Some(options) = selection {
            options.validate()?;
        }
        self.diagram_generation = selection;
        Ok(self)
    }
    /// Selected native effects; omission never triggers a renderer.
    pub fn diagram_generation(&self) -> Option<super::DiagramGenerationOptions> {
        self.diagram_generation
            .map(super::DiagramGeneration::options)
    }
    /// Acquire complete selected resources before pure compilation.
    ///
    /// # Errors
    /// Preserves original capture failures without retries.
    pub fn acquire_diagrams(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        corpus: crate::corpus::Limits,
    ) -> Result<(), super::DiagramGenerationError> {
        if let Some(options) = self.diagram_generation() {
            super::acquire_diagrams(inputs, corpus, options)?;
        }
        Ok(())
    }
}

impl PublicationRecipe {
    /// Validate site-set identity before any native acquisition.
    ///
    /// # Errors
    /// Refuses empty, oversized or duplicate site choices and partial response sets.
    /// Omitted selection is resolved by checked rendering before matching responses.
    /// Corpus membership and candidate closure remain the compiler's responsibility
    /// after acquisition; response identity still binds the complete resolved review.
    pub fn new(
        sites: Vec<SiteRecipe>,
        selection: Option<authored::Selection>,
        review_responses: Option<BTreeMap<Key, ReviewSubmission>>,
        maximum: usize,
    ) -> Result<Self, &'static str> {
        if sites.is_empty() || sites.len() > maximum {
            return Err("explicit site set is empty or exceeds its site-count ceiling");
        }
        let mut identities = BTreeSet::new();
        if sites.iter().any(|site| !identities.insert(&site.site)) {
            return Err("duplicate site identity in explicit local publication set");
        }
        if let Some(responses) = &review_responses
            && (responses.is_empty() || responses.keys().collect::<BTreeSet<_>>() != identities)
        {
            return Err("review responses require exactly the complete explicit site set");
        }
        Ok(Self {
            sites,
            documentation_catalogs: BTreeMap::new(),
            documentation_generation: None,
            render_year: None,
            selection,
            review_responses,
            math_generation: None,
            diagram_generation: None,
            social_card_generation: None,
            preview_link_bases: None,
        })
    }
}

impl<Sites> PublicationRecipe<Sites> {
    /// Select explicit acquisition effects separately from workspace provider configuration.
    ///
    /// # Errors
    /// Rejects unbounded execution or capture options before any native reads or writes.
    pub fn with_math_generation(
        mut self,
        selection: Option<super::MathGeneration>,
    ) -> Result<Self, &'static str> {
        if let Some(options) = selection {
            options.validate()?;
        }
        self.math_generation = selection;
        Ok(self)
    }

    pub(super) fn resolve_math(
        mut self,
        workspace: &crate::workspace::Workspace,
        selected: Option<bool>,
        allow_execute: bool,
    ) -> Self {
        self.math_generation = super::MathGeneration::resolve(
            workspace,
            selected,
            self.math_generation,
            allow_execute,
        );
        self
    }

    /// Selected native acquisition options; omission performs no acquisition.
    pub fn math_generation(&self) -> Option<super::MathGenerationOptions> {
        self.math_generation.map(super::MathGeneration::options)
    }

    /// Acquire selected native typesetting before pure rendering, never on an omitted effect.
    ///
    /// # Errors
    /// Preserves complete native generation failures without retry or fallback.
    pub fn acquire_math(
        &self,
        inputs: &mut super::AcquiredPublicationSet,
        corpus: crate::corpus::Limits,
    ) -> Result<(), super::MathGenerationError> {
        if let Some(options) = self.math_generation() {
            super::acquire_math(inputs, corpus, options)?;
        }
        Ok(())
    }
}

impl PublicationRecipe {
    /// Exact selected identities, preserving the request's order.
    pub fn sites(&self) -> impl Iterator<Item = &Key> {
        self.sites.iter().map(|site| &site.site)
    }

    pub(super) fn check_response_bounds(
        &self,
        limits: super::ReviewResponseLimits,
        maximum: usize,
    ) -> Result<(), &'static str> {
        if let Some(responses) = &self.review_responses {
            if limits.bytes == 0 || limits.decisions == 0 || maximum == 0 {
                return Err("review response limits must be nonzero");
            }
            let mut remaining = maximum;
            for submission in responses.values() {
                let bytes = submission.response_json.len();
                if bytes > limits.bytes {
                    return Err("review response exceeds its per-response byte ceiling");
                }
                remaining = remaining
                    .checked_sub(bytes)
                    .ok_or("review responses exceed the aggregate byte ceiling")?;
            }
        }
        Ok(())
    }

    /// Project owned choices into the one shared renderer API. Graph semantics
    /// use native defaults; file/output ceilings are supplied by the caller.
    pub fn options(&self, limits: Limits) -> PublicationSetRenderOptions<'_> {
        PublicationSetRenderOptions {
            derive_selection: false,
            sites: self
                .sites
                .iter()
                .map(|site| {
                    (
                        site.site.clone(),
                        SiteRenderOptions {
                            render_year: self.render_year,
                            model_output: site.model_output.as_ref(),
                            graph: site.graph.as_ref().map(|graph| GraphRenderOptions {
                                output: &graph.output,
                                limits: Default::default(),
                            }),
                            limits,
                        },
                    )
                })
                .collect(),
            selection: self.selection.clone(),
            review_responses: self.review_responses.as_ref(),
            preview_urls: None,
            preview_link_bases: self.preview_link_bases.clone(),
        }
    }
}

impl PublicationRecipe<Option<Vec<SiteRecipe>>> {
    pub(super) fn configured(selection: Option<authored::Selection>) -> Self {
        Self {
            sites: None,
            documentation_catalogs: BTreeMap::new(),
            documentation_generation: None,
            render_year: None,
            selection,
            review_responses: None,
            math_generation: None,
            diagram_generation: None,
            social_card_generation: None,
            preview_link_bases: None,
        }
    }

    pub(super) fn invocation(&self) -> Option<&[SiteRecipe]> {
        self.sites.as_deref()
    }

    pub(super) fn resolve_sites(
        &self,
        selection: &crate::workspace::BuildSelection,
    ) -> PublicationRecipe {
        self.clone().map_sites(|sites| {
            sites.unwrap_or_else(|| {
                selection
                    .sites
                    .iter()
                    .map(|site| SiteRecipe {
                        site: site.clone(),
                        model_output: None,
                        graph: None,
                    })
                    .collect()
            })
        })
    }
}

/// Shared wire shape check; response bytes remain a string for the native matcher.
pub(super) fn review_response_shape(value: &serde_json::Value) -> bool {
    value.get("review_responses").is_none_or(|responses| {
        responses.as_object().is_some_and(|responses| {
            responses.values().all(|submission| {
                submission.is_object()
                    && submission.get("binding").is_some_and(|binding| {
                        binding.is_object()
                            && binding
                                .get("policy")
                                .is_some_and(serde_json::Value::is_object)
                    })
            })
        })
    })
}

pub(super) fn review_responses_schema(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    schemars::json_schema!({
        "type": "object",
        "propertyNames": generator.subschema_for::<Key>(),
        "additionalProperties": generator.subschema_for::<ReviewSubmission>()
    })
}
