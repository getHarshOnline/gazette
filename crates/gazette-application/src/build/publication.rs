//! Connect retained native inputs to complete local output without hidden rereads.

use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
};

use gazette_compiler::{authored, corpus, toml};
use gazette_core::{
    publication::{CapabilityId, Key, content, routing::PublicBaseUrl},
    render, resource, source, stage,
};
use gazette_ports::{render::TemplateCompiler, resource::ResourceCompiler, stage::StageWriter};

use super::{
    AcquiredInputs, CompositionError, MetadataError, PublicationInputError,
    PublicationResolutionError, RenderedSite, ResolvedPublication, SiteRenderError,
    SiteRenderOptions, TemplateError,
};

/// Independent limits for each retained compiler phase; output limits live in render options.
#[derive(Clone, Copy, Debug, Default)]
pub struct PublicationRenderLimits<C = corpus::Limits> {
    /// Complete authored syntax corpus limits.
    pub corpus: C,
    /// Shared content graph and route limits.
    pub graph: content::Limits,
    /// Limits for compilation after the retained semantic phase.
    pub output: PublicationOutputLimits,
}

/// Presentation/resource compilation limits after syntax and semantics have been checked.
/// Native output inventory limits remain in the explicit render options.
#[derive(Clone, Copy, Debug)]
pub struct PublicationOutputLimits {
    /// Presentation metadata parser limits.
    pub metadata: toml::Limits,
    /// Template compilation, model and individual render limits.
    pub render: render::Limits,
    /// Exact retained resource compilation, separate from template evaluation.
    pub resources: resource::Limits,
    /// Aggregate site/page resource facts before per-selection evidence/model expansion.
    pub resource_metadata_bytes: usize,
}

impl Default for PublicationOutputLimits {
    fn default() -> Self {
        Self {
            metadata: Default::default(),
            render: Default::default(),
            resources: Default::default(),
            resource_metadata_bytes: 4 << 20,
        }
    }
}

/// Complete local tree bound to the exact acquired native inputs that produced it.
/// No public-admission or release authority is implied by construction or staging.
#[derive(Debug)]
pub struct PublicationRender<'a> {
    inputs: &'a AcquiredInputs,
    site: RenderedSite,
    protected_roots: Vec<PathBuf>,
    pub(super) selection: Option<Arc<authored::Selection>>,
    pub(super) candidate: Option<super::publication_model::CandidateEvidence>,
    pub(super) preview_urls: Option<Arc<BTreeMap<Key, PublicBaseUrl>>>,
}

impl PublicationRender<'_> {
    pub(super) fn into_site(self) -> RenderedSite {
        self.site
    }

    /// Session-only serving map used by this output. Such output is not eligible
    /// for durable release preparation; the configured public bases remain intact.
    pub fn preview_urls(&self) -> Option<&BTreeMap<Key, PublicBaseUrl>> {
        self.preview_urls.as_deref()
    }

    /// Retained native source and configuration roots that local output setup must protect.
    /// These private coordinates are not public page metadata.
    pub fn protected_roots(&self) -> &[PathBuf] {
        &self.protected_roots
    }
    /// Complete candidate selection retained outside the output tree. None is
    /// the unfiltered private path; Some is still not publication approval.
    pub fn selection(&self) -> Option<&authored::Selection> {
        self.selection.as_deref()
    }

    /// Present only when the exact selected owner passed the required captured
    /// candidate-admission transition. This remains neither certification nor authority.
    pub fn candidate_admission(&self) -> Option<&super::CandidateAdmission> {
        self.candidate
            .as_ref()
            .and_then(|evidence| evidence.admission())
    }

    pub(super) fn candidate_check(&self) -> Option<&super::PolicyCheck> {
        self.candidate.as_ref().map(|evidence| evidence.check())
    }

    /// Exact captured source/configuration evidence, separate from the output tree.
    pub fn inputs(&self) -> &AcquiredInputs {
        self.inputs
    }
    /// Complete local site output and its private render evidence.
    pub fn site(&self) -> &RenderedSite {
        &self.site
    }

    /// Write a fresh stage with every consumed native repository protected.
    /// Retention remains a separate explicit operation on the returned native guard.
    /// The complete canonical directory-byte ceiling is enforced before creation.
    ///
    /// # Errors
    /// Rejects output/source overlap, inconsistent bytes, tighter bounds or native write/read-back failure.
    pub fn stage<S: StageWriter>(
        &self,
        writer: &S,
        parent: &Path,
        limits: source::Limits,
        directory_bytes: usize,
    ) -> Result<S::Stage, stage::Error> {
        writer.stage(
            stage::Location::FreshBounded {
                parent,
                directory_bytes,
            },
            &self.protected_roots,
            self.site.output().as_input(),
            limits,
        )
    }
}

/// A complete local publication could not be compiled; no partial output escapes.
#[derive(Debug)]
pub enum PublicationRenderError {
    /// A site view was paired with a different retained acquisition.
    CaptureMismatch,
    /// Inward binding or source association is missing.
    Binding(Key),
    /// A selected logical resource has no admitted package declaration or output binding.
    ResourceBinding(resource::ResourceId),
    /// Configured source roots could not be resolved from this acquisition.
    Composition(CompositionError),
    /// Source provenance or declared publication home could not be bound.
    Source(authored::SourceBindingError),
    /// A selected provider needs capability compilation; it cannot be silently omitted.
    Capability(CapabilityId),
    /// Selected native resource compilation or returned evidence failed.
    Resource(resource::Error),
    /// Retained input mapping or syntax parsing failed.
    Input(PublicationInputError),
    /// Whole-corpus semantics failed.
    Authored(authored::Error),
    /// Retained presentation declarations failed.
    Metadata(MetadataError),
    /// Selected template compilation failed.
    Template(TemplateError),
    /// Whole-site assembly failed.
    Site(SiteRenderError),
}

impl PublicationRenderError {
    /// Stable diagnostic class; exact lower-layer failures remain available.
    pub fn code(&self) -> &'static str {
        match self {
            Self::CaptureMismatch => "publication_capture_mismatch",
            Self::Binding(_) => "publication_binding",
            Self::ResourceBinding(_) => "publication_resource_binding",
            Self::Composition(error) => error.code(),
            Self::Source(error) => error.code(),
            Self::Capability(_) => "publication_uncompiled_capability",
            Self::Resource(error) => error.code(),
            Self::Input(error) => error.code(),
            Self::Authored(_) => "publication_authored",
            Self::Metadata(error) => error.code(),
            Self::Template(error) => error.code(),
            Self::Site(error) => error.code(),
        }
    }
}
impl fmt::Display for PublicationRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CaptureMismatch => f.write_str("site view and semantic model do not share one acquisition; resolve from the selected input set"),
            Self::Binding(key) => write!(f, "publication input binding missing: {key}"),
            Self::ResourceBinding(resource) => {
                write!(f, "publication resource binding missing: {resource}")
            }
            Self::Composition(error) => error.fmt(f),
            Self::Source(error) => error.fmt(f),
            Self::Capability(key) => write!(
                f,
                "selected capability {key} needs compilation; no partial site produced"
            ),
            Self::Input(error) => error.fmt(f),
            Self::Resource(error) => error.fmt(f),
            Self::Authored(error) => error.fmt(f),
            Self::Metadata(error) => error.fmt(f),
            Self::Template(error) => error.fmt(f),
            Self::Site(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PublicationRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Resource(error) => Some(error),
            Self::Source(error) => Some(error),
            Self::Composition(error) => Some(error),
            Self::Authored(error) => Some(error),
            Self::Metadata(error) => Some(error),
            Self::Template(error) => Some(error),
            Self::Site(error) => Some(error),
            _ => None,
        }
    }
}

impl From<PublicationResolutionError> for PublicationRenderError {
    fn from(error: PublicationResolutionError) -> Self {
        match error {
            PublicationResolutionError::Authored(error) => Self::Authored(error),
            PublicationResolutionError::Source(error) => Self::Source(error),
        }
    }
}

impl AcquiredInputs {
    /// Parse once, resolve once, compile the retained Theme, then assemble every local page.
    ///
    /// This draft-inclusive operation is not public admission. Unsupported selected
    /// capabilities fail explicitly unless their exact observed absence was selected by an
    /// optional capability contract. Configured project docs share the corpus; native sources
    /// are never reopened.
    ///
    /// # Errors
    /// Rejects incomplete bindings, wrong source/home associations, unsupported requested
    /// outputs, parser/semantic/template failures, collisions and resource ceilings.
    pub fn render_publication(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PublicationRender<'_>, PublicationRenderError> {
        self.render_publication_with_host_files(
            compiler, resources, raster, options, limits, markdown, None,
        )
    }

    /// Render with an explicit pure hosting file provider.
    ///
    /// # Errors
    /// Preserves complete-output failures and rejects configured hosting without a provider.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn render_publication_with_host_files(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<PublicationRender<'_>, PublicationRenderError> {
        let parsed = self
            .parse_publication(limits.corpus, markdown)
            .map_err(PublicationRenderError::Input)?;
        let resolved = parsed.resolve(limits.graph)?;
        resolved.render_local_with_host_files(
            compiler,
            resources,
            raster,
            options,
            limits.output,
            markdown,
            host_files,
        )
    }
    /// Render with explicit bounded social-card capture after resolving this input once.
    /// # Errors
    /// Preserves render and native capture failures without retrying an effect.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn render_publication_with_generations(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
        generation: super::SocialCardGeneration<'_>,
    ) -> Result<PublicationRender<'_>, PublicationRenderError> {
        let parsed = self
            .parse_publication(limits.corpus, markdown)
            .map_err(PublicationRenderError::Input)?;
        let resolved = parsed.resolve(limits.graph)?;
        let mut budget = super::social_cards::Budget::new(generation.options);
        super::social_cards::resume(
            Some(&generation),
            &mut budget,
            raster,
            |social_card, social_card_limits| {
                resolved.render_site_view(
                    resolved.inputs(),
                    compiler,
                    resources,
                    super::page::ProjectionInputs {
                        raster,
                        markdown,
                        discovery: None,
                        social_card,
                        social_card_limits,
                    },
                    options.clone(),
                    limits.output,
                    host_files,
                )
            },
        )
    }
}

impl<'s> ResolvedPublication<'_, 's> {
    /// Compile presentation and render locally from this exact checked graph and syntax.
    ///
    /// No source acquisition, content parsing or semantic resolution is repeated. The
    /// output owns its bytes and borrows only acquisition evidence, so it may outlive
    /// this semantic phase. Checking is not public admission: drafts and private
    /// structural records remain in this explicitly local output.
    ///
    /// # Errors
    /// Rejects missing bindings, presentation/resource failures, unresolved output
    /// obligations and all render limits. No partial output or native effect is returned.
    pub fn render_local(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationOutputLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PublicationRender<'s>, PublicationRenderError> {
        self.render_local_with_host_files(
            compiler, resources, raster, options, limits, markdown, None,
        )
    }

    /// Render with an explicit pure hosting file provider.
    ///
    /// # Errors
    /// Preserves complete-output failures and rejects configured hosting without a provider.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn render_local_with_host_files(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationOutputLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<PublicationRender<'s>, PublicationRenderError> {
        self.render_site_view(
            self.inputs(),
            compiler,
            resources,
            super::page::ProjectionInputs {
                social_card: None,
                social_card_limits: Default::default(),
                raster,
                markdown,
                discovery: None,
            },
            options,
            limits,
            host_files,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Shared rendering services and bounds"
    )]
    pub(super) fn render_site_view(
        &self,
        inputs: &'s AcquiredInputs,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        projection_inputs: super::page::ProjectionInputs<'_>,
        options: SiteRenderOptions<'_>,
        limits: PublicationOutputLimits,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<PublicationRender<'s>, PublicationRenderError> {
        if !self.inputs().shares_capture(inputs) {
            return Err(PublicationRenderError::CaptureMismatch);
        }
        let protected_roots = inputs
            .source_roots()
            .map_err(PublicationRenderError::Composition)?;
        let metadata = inputs
            .check_metadata(limits.metadata)
            .map_err(PublicationRenderError::Metadata)?;
        let theme = metadata
            .compile_templates(compiler, limits.render)
            .map_err(PublicationRenderError::Template)?;
        let mut resource_metadata =
            render::LimitedWriter::new(std::io::sink(), limits.resource_metadata_bytes);
        let base = self.authored().content().base_url(inputs.site());
        let modules = super::resources::links(&metadata, base, &mut resource_metadata)?;
        let capabilities = metadata.resolve_capabilities(&mut resource_metadata)?;
        let chrome = self
            .authored()
            .site_chrome_with_composition(inputs.site(), metadata.composition())
            .map_err(|error| {
                PublicationRenderError::Site(super::SiteRenderError::Page(
                    super::PageRenderError::Projection(error),
                ))
            })?;
        let search_control = chrome
            .as_ref()
            .and_then(|chrome| chrome.shell.search.as_ref());
        let search =
            super::search::binding(self.authored().content(), inputs.site(), search_control)
                .map_err(|error| {
                    PublicationRenderError::Site(super::SiteRenderError::Render(error))
                })?;
        capabilities.validate_search(search.as_ref())?;
        let page_resources = super::page_resources::compile(
            inputs,
            self.authored(),
            resources,
            &modules,
            limits.resources,
            &mut resource_metadata,
            options.limits.total_bytes,
        )?;
        let page_bytes: u64 = page_resources
            .outputs
            .values()
            .map(|bytes| bytes.len() as u64)
            .sum();
        let discovery = self.authored().discovery().map_err(|error| {
            PublicationRenderError::Site(super::SiteRenderError::Discovery(error))
        })?;
        let selection = self
            .authored()
            .project_site_pages(inputs.site())
            .map_err(|error| PublicationRenderError::Site(super::SiteRenderError::Pages(error)))?;
        let mut pages = std::collections::BTreeMap::new();
        let mut prepared_bytes = render::LimitedWriter::new(
            std::io::sink(),
            usize::try_from(options.limits.total_bytes).unwrap_or(usize::MAX),
        );
        for selected in selection.pages() {
            let projection = self
                .authored()
                .project_page_with_renditions(
                    &selected.node.id,
                    projection_inputs.raster,
                    projection_inputs.markdown,
                    Some(&discovery),
                    Some(gazette_compiler::markdown::ComponentPresentation {
                        definitions: &theme.components,
                        renderer: &theme,
                    }),
                    &page_resources.dimensions(&selected.node.id),
                )
                .map_err(|error| {
                    PublicationRenderError::Site(super::SiteRenderError::Page(
                        super::PageRenderError::Projection(error),
                    ))
                })?;
            super::capability_status::charge_metadata(&mut prepared_bytes, &projection)?;
            pages.insert(selected.node.id.clone(), projection);
        }
        let islands = super::islands::compile(
            &pages,
            resources,
            &modules,
            base.ok_or_else(|| PublicationRenderError::Binding(inputs.site().clone()))?,
            limits.resources,
            options.limits.total_bytes.saturating_sub(page_bytes),
        )?;
        let island_bytes: u64 = islands
            .outputs
            .values()
            .map(|bytes| bytes.len() as u64)
            .sum();
        super::capability_status::charge_metadata(&mut resource_metadata, &islands.evidence)?;
        let global_roots = metadata
            .effective_theme()
            .resource_requirements()
            .iter()
            .map(|resource| {
                modules
                    .get(resource)
                    .cloned()
                    .ok_or_else(|| PublicationRenderError::ResourceBinding(resource.clone()))
            })
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;
        let mut page_roots: std::collections::BTreeMap<_, std::collections::BTreeSet<_>> = pages
            .keys()
            .cloned()
            .map(|page| (page, std::collections::BTreeSet::new()))
            .collect();
        let mut page_eager_roots = page_roots.clone();
        for (page, resource) in page_resources
            .model
            .iter()
            .flat_map(|(page, resources)| resources.iter().map(move |resource| (page, resource)))
            .chain(islands.model.iter())
        {
            page_roots
                .entry(page.clone())
                .or_default()
                .extend(resource.references.keys().cloned());
            page_eager_roots.entry(page.clone()).or_default().extend(
                resource
                    .references
                    .keys()
                    .filter(|url| !resource.deferred_references.contains(*url))
                    .cloned(),
            );
        }
        let mut aliases = std::collections::BTreeMap::new();
        for (page, evidence) in &page_resources.evidence {
            let resources = page_resources
                .model
                .get(page)
                .ok_or_else(|| PublicationRenderError::Binding(page.clone()))?;
            if evidence.len() != resources.len() {
                return Err(PublicationRenderError::Binding(page.clone()));
            }
            for (evidence, resource) in evidence.iter().zip(resources) {
                super::resources::collect_aliases(
                    &mut aliases,
                    &evidence.recipe,
                    &resource.references,
                    &modules,
                )?;
            }
        }
        let compiled_resources = super::resources::compile(
            &metadata,
            self.authored(),
            resources,
            super::resources::Compilation {
                links: &modules,
                global_roots,
                page_roots,
                page_eager_roots,
                aliases,
                limits: limits.resources,
                budget: &mut resource_metadata,
                output_limit: options
                    .limits
                    .total_bytes
                    .saturating_sub(page_bytes)
                    .saturating_sub(island_bytes),
            },
        )?;
        let site = theme
            .render_site_with_resources(
                self.authored(),
                options,
                super::site::SiteRenderContext {
                    social_card: projection_inputs.social_card,
                    social_card_limits: projection_inputs.social_card_limits,
                    documentation_catalog: inputs.documentation_catalog(),
                    capabilities: &capabilities,
                    resources: &compiled_resources,
                    page_resources: &page_resources,
                    raster: projection_inputs.raster,
                    markdown: projection_inputs.markdown,
                    host_files,
                    prepared: Some(super::islands::PreparedPages { pages, islands }),
                },
            )
            .map_err(PublicationRenderError::Site)?;
        Ok(PublicationRender {
            selection: None,
            candidate: None,
            preview_urls: self
                .authored()
                .content()
                .preview_urls()
                .map(|urls| Arc::new(urls.clone())),
            inputs,
            site,
            protected_roots,
        })
    }
}
