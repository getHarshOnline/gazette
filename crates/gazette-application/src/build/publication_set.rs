//! Exact local site sets share native acquisition and semantic resolution, not release authority.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use gazette_compiler::{authored, toml, workspace};
use gazette_core::{
    publication::{Key, routing::PublicBaseUrl},
    source::{Inventory, Limits},
};
use gazette_ports::{render::TemplateCompiler, resource::ResourceCompiler};
use serde::Serialize;

use super::{
    AcquiredInputs, CompositionError, PublicationInputError, PublicationRender,
    PublicationRenderError, PublicationRenderLimits, PublicationResolutionError,
    SiteRenderEvidence, SiteRenderOptions, composition, publication_inputs,
};
use crate::source::PreparationLimits;

mod acquisition;
pub use acquisition::{
    acquire_configured_publications, acquire_development_publications,
    acquire_development_resolved_publications, acquire_development_selected_publications,
    acquire_publications, acquire_resolved_publications, acquire_selected_publications,
};

/// Aggregate source acquisition and requested-site bounds, before any native read.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PublicationSetAcquisitionLimits {
    /// Maximum distinct requested sites; an empty or repeated selection is invalid.
    #[schemars(range(max = usize::MAX))]
    pub sites: usize,
    /// One budget for the union of content and selected presentation source keys.
    pub sources: PreparationLimits,
}

impl Default for PublicationSetAcquisitionLimits {
    fn default() -> Self {
        Self {
            sites: 128,
            sources: PreparationLimits {
                sources: 256,
                entries: Limits::default().entries,
                bytes: Limits::default().total_bytes,
                per_source: Default::default(),
            },
        }
    }
}

/// Whole-operation retention ceilings, separate from each site's output limits.
#[derive(Clone, Copy, Debug)]
pub struct PublicationSetRenderLimits<C = gazette_compiler::corpus::Limits> {
    /// Parse/resolve once; presentation and resource limits apply separately to each site.
    pub publication: PublicationRenderLimits<C>,
    /// Combined bytes retained across successful sites; each next render receives the remainder.
    pub bytes: u64,
    /// Combined file/directory inventory entries retained across successful sites.
    pub entries: usize,
}

impl<C: Default> Default for PublicationSetRenderLimits<C> {
    fn default() -> Self {
        Self {
            publication: Default::default(),
            bytes: 1 << 30,
            entries: 100_000,
        }
    }
}

/// Exact site recipes and optional complete candidate intent over the shared corpus.
/// Selection is applied once before any site renderer or resource compiler; it is
/// not public-field, rights, evidence or publisher approval.
#[derive(Clone)]
pub struct PublicationSetRenderOptions<'a> {
    /// Exactly the acquired site set, including sites whose presentation failed.
    pub sites: BTreeMap<Key, SiteRenderOptions<'a>>,
    /// Without a candidate checker or derived intent, none preserves private rendering.
    /// Candidate checking derives all non-draft content and required assets by default.
    /// Some supplies one complete explicit,
    /// closed non-draft selection for all content and assets, not one per site.
    pub selection: Option<authored::Selection>,
    /// Derive complete non-draft intent when selection is omitted, including unchecked
    /// preparation. Ordinary local previews leave this false and retain their draft policy.
    /// Explicit selection always takes precedence; this does not grant release approval.
    pub derive_selection: bool,
    /// Optional exact per-site evaluator responses. Requires checked non-preview
    /// rendering; omission retains the private candidate path.
    pub review_responses: Option<&'a BTreeMap<Key, super::ReviewSubmission>>,
    /// Session-only serving bases for exactly this site set. None uses configured
    /// public addresses. Some produces non-release developer output, never a
    /// configuration/registry update. The preview owner supplies held listener URLs.
    pub preview_urls: Option<BTreeMap<Key, PublicBaseUrl>>,
    /// Explicit ordinary hyperlink projections into other local preview outputs.
    /// Requires local serving-address projection; never accepted for candidate checking.
    pub preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
}

/// Nonempty exact site outcomes over one immutable configuration/source acquisition.
/// Successful views share payload ownership; failed sources retain no stale payloads.
#[derive(Debug)]
pub struct AcquiredPublicationSet {
    build_selection: workspace::BuildSelection,
    content_sources: BTreeSet<Key>,
    presentation_sources: BTreeMap<Key, BTreeSet<Key>>,
    shared: Arc<composition::CapturedInputs>,
    content: AcquiredInputs,
    source_failures: BTreeMap<Key, Arc<CompositionError>>,
    sites: BTreeMap<Key, Result<AcquiredInputs, Arc<CompositionError>>>,
}

impl AcquiredPublicationSet {
    /// Attach provider-validated documentation to its exact acquired publication.
    /// The shared authored corpus remains unchanged; each site retains its own catalog.
    ///
    /// # Errors
    /// Rejects an unselected or failed site, replacement, or mismatched source/configuration.
    pub fn install_documentation_catalog(
        &mut self,
        catalog: Arc<super::DocumentationCatalog>,
    ) -> Result<(), super::DocumentationError> {
        let site = catalog.site().clone();
        let Some(Ok(inputs)) = self.sites.get_mut(&site) else {
            return Err(super::DocumentationError::Target(
                gazette_core::documentation::Error::Invalid {
                    context: "documentation catalog".into(),
                    reason: "catalog site is not a successfully acquired publication",
                },
            ));
        };
        inputs.install_documentation_catalog(catalog)
    }
    fn source_bytes(&self) -> PublicationSourceBytes<'_> {
        let bytes = |keys: &BTreeSet<Key>| {
            keys.iter()
                .filter_map(|key| self.sources().get(key))
                .map(|source| source.inventory().total_bytes())
                .sum()
        };
        PublicationSourceBytes {
            captured_total: self
                .sources()
                .values()
                .map(|source| source.inventory().total_bytes())
                .sum(),
            shared_content: bytes(&self.content_sources),
            sites: self
                .presentation_sources
                .iter()
                .map(|(site, presentation)| {
                    let sources = self.content_sources.union(presentation).cloned().collect();
                    let presentation_only = presentation
                        .difference(&self.content_sources)
                        .cloned()
                        .collect();
                    (
                        site,
                        SiteSourceBytes {
                            captured: bytes(&sources),
                            presentation: bytes(&presentation_only),
                            missing_sources: presentation
                                .iter()
                                .filter(|key| !self.sources().contains_key(*key))
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }

    /// Exact local selection and configured draft intent, separate from publication approval.
    pub fn build_selection(&self) -> &workspace::BuildSelection {
        &self.build_selection
    }
    /// Freeze direct-local capability results for every successfully captured site.
    /// Site association is retained on the first exact acquisition refusal.
    ///
    /// # Errors
    ///
    /// Returns the exact site and capability-acquisition failure without installing a
    /// partial capture for that site or silently changing another site's outcome.
    pub fn acquire_direct_local_capabilities(
        &mut self,
        metadata_limits: toml::Limits,
        limits: super::CapabilityAcquisitionLimits,
    ) -> Result<(), PublicationSetError> {
        for (site, outcome) in &mut self.sites {
            let Ok(inputs) = outcome else {
                continue;
            };
            inputs
                .acquire_direct_local_capabilities(metadata_limits, limits)
                .map_err(|error| PublicationSetError::Capability {
                    site: site.clone(),
                    error: Box::new(error),
                })?;
        }
        Ok(())
    }

    /// Capture every selected DOT resource once before rendering, sharing exact results across sites.
    ///
    /// # Errors
    /// Refuses incomplete selection, denied execution, diagnostics or invalid PNG without retry.
    pub fn acquire_diagrams(
        &mut self,
        runner: &dyn gazette_ports::process::IsolatedCaptureRunner,
        raster: &gazette_core::raster::Inspect<'_>,
        limits: gazette_compiler::corpus::Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        options: super::DiagramGenerationOptions,
    ) -> Result<(), super::DiagramGenerationError> {
        let capture =
            super::DiagramCapture::generate(self, runner, raster, limits, markdown, options)?;
        self.content.diagrams = Some(Arc::new(capture));
        for inputs in self
            .sites
            .values_mut()
            .filter_map(|site| site.as_mut().ok())
        {
            inputs.diagrams.clone_from(&self.content.diagrams);
        }
        Ok(())
    }

    /// Private original DOT/PNG capture shared by the selected site set.
    pub fn diagram_capture(&self) -> Option<&super::DiagramCapture> {
        self.content.diagram_capture()
    }
    /// Capture native typesetting once for the shared content universe, then share
    /// that exact result with every successful site view before rendering.
    ///
    /// # Errors
    /// Refuses incomplete or failed generation without replacing site captures.
    pub fn acquire_math(
        &mut self,
        runner: &dyn gazette_ports::process::RetainedCaptureRunner,
        limits: gazette_compiler::corpus::Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        options: super::MathGenerationOptions,
    ) -> Result<(), super::MathGenerationError> {
        self.content
            .acquire_math(runner, limits, markdown, options)?;
        for inputs in self
            .sites
            .values_mut()
            .filter_map(|site| site.as_mut().ok())
        {
            inputs.math.clone_from(&self.content.math);
        }
        Ok(())
    }

    /// Private complete typesetter capture shared across all site outcomes.
    pub fn math_capture(&self) -> Option<&super::MathCapture> {
        self.content.math_capture()
    }

    /// Configured roots of the exact selected source union, including failed sources.
    /// Shared with render-time protection; this performs no native path observation.
    ///
    /// # Errors
    /// Refuses a missing source binding rather than silently dropping its protected root.
    pub fn source_roots(&self) -> Result<Vec<std::path::PathBuf>, CompositionError> {
        let mut roots = self.shared.source_roots()?;
        for input in self.sites.values().filter_map(|site| site.as_ref().ok()) {
            if let Some(catalog) = input.documentation_catalog() {
                roots.extend(catalog.protected_roots().iter().cloned());
            }
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }
    /// Every requested site in stable key order, including failed presentation acquisition.
    /// A failed site reports its first failed source key; all failures remain in `source_failures`.
    pub fn sites(&self) -> &BTreeMap<Key, Result<AcquiredInputs, Arc<CompositionError>>> {
        &self.sites
    }

    /// The single complete configuration that selected every source and site outcome.
    pub fn compilation(&self) -> &workspace::Compilation {
        self.shared.compilation()
    }

    /// The same retained configuration and bound commands shared by every site view.
    pub fn configuration(&self) -> &crate::configuration::ResolvedWorkspace {
        self.content.configuration()
    }

    /// Exact supplied document or selected native document-set evidence.
    pub fn configuration_evidence(&self) -> &crate::configuration::Evidence {
        self.shared.configuration_evidence()
    }

    /// Successfully captured inputs, retained once per exact source key.
    /// Failed presentation sources are absent, never replaced by stale values.
    pub fn sources(&self) -> &BTreeMap<Key, crate::source::AcquiredInput> {
        self.shared.sources()
    }

    /// Shared content capture for parsing without requiring a successful presentation.
    /// This view retains no Theme or plugin selection, including when every site failed.
    pub fn content_inputs(&self) -> &AcquiredInputs {
        &self.content
    }

    /// Every failed presentation-only source, including failures shared by several sites.
    /// Sharing a failure does not retry its source or weaken any site's requirement.
    pub fn source_failures(&self) -> &BTreeMap<Key, Arc<CompositionError>> {
        &self.source_failures
    }

    /// Resolve one complete content graph, then render each site independently from it.
    ///
    /// Acquisition and rendering failures remain in the exact set while other sites continue.
    /// Shared syntax/semantic failures stop this operation before any site output.
    /// Candidate selection validates shared content even if all presentation inputs
    /// failed. Without selection, that all-failed case returns failure evidence only,
    /// without resolving or claiming a valid model. Neither case calls a renderer.
    /// Remaining aggregate bounds constrain each render before allocation/retention;
    /// successful outputs consume them in site-key order, never request insertion order.
    /// No staging, publication admission, release transaction or native effects occur.
    ///
    /// # Errors
    /// Rejects an options/set mismatch and shared content failures. Site-local failures
    /// are values in the returned set, not silently omitted entries.
    pub fn render_local(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
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
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        self.render_with_policy(
            compiler, resources, raster, options, limits, markdown, None, host_files, None,
        )
    }

    /// Check every selected site's candidate before invoking any Theme or resource compiler.
    /// Requests share one resolved model and are all bounded before the first checker runs.
    /// Checker effects are explicit and not sandboxed; zero exits are not publication approval.
    /// No candidate request, command or checker result is embedded into public output.
    ///
    /// # Errors
    /// Preserves completed and failing invocation evidence. Requires complete captured
    /// presentation, a non-preview candidate and the configured checker; never retries.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit independent native ports; SDK supplies concrete composition"
    )]
    pub fn check_and_render_local(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        policy: super::CandidateCheckOptions,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        self.check_and_render_local_with_host_files(
            compiler, resources, raster, options, limits, markdown, runner, policy, None,
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
    pub fn check_and_render_local_with_host_files(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        policy: super::CandidateCheckOptions,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        self.render_with_policy(
            compiler,
            resources,
            raster,
            options,
            limits,
            markdown,
            Some((runner, runner, policy)),
            host_files,
            None,
        )
    }

    /// Render with explicitly selected social-card effects; semantic resolution runs once.
    /// # Errors
    /// Preserves complete output and bounded capture failures.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn render_local_with_generations(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
        generation: super::SocialCardGeneration<'_>,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        self.render_with_policy(
            compiler,
            resources,
            raster,
            options,
            limits,
            markdown,
            None,
            host_files,
            Some(generation),
        )
    }
    /// Check candidates once, then capture cards while assembling the same admitted selection.
    /// # Errors
    /// Preserves check and capture evidence; a capture never repeats candidate checks.
    #[expect(
        clippy::too_many_arguments,
        reason = "Explicit rendering services and bounds"
    )]
    pub fn check_and_render_local_with_generations(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        runner: &(impl gazette_ports::process::Runner + gazette_ports::process::CaptureRunner),
        policy: super::CandidateCheckOptions,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
        generation: super::SocialCardGeneration<'_>,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        self.render_with_policy(
            compiler,
            resources,
            raster,
            options,
            limits,
            markdown,
            Some((runner, runner, policy)),
            host_files,
            Some(generation),
        )
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "One implementation for pure and explicitly checked rendering"
    )]
    fn render_with_policy(
        &self,
        compiler: &impl TemplateCompiler,
        resources: &impl ResourceCompiler,
        raster: &gazette_core::raster::Inspect<'_>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
        markdown: &dyn gazette_ports::markdown::Markdown,
        policy: Option<(
            &dyn gazette_ports::process::Runner,
            &dyn gazette_ports::process::CaptureRunner,
            super::CandidateCheckOptions,
        )>,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
        generation: Option<super::SocialCardGeneration<'_>>,
    ) -> Result<RenderedPublicationSet<'_>, PublicationSetError> {
        if options
            .review_responses
            .is_some_and(|responses| policy.is_none() || !self.sites.keys().eq(responses.keys()))
        {
            return Err(PublicationSetError::Selection(
                "review responses require checked rendering and exactly the acquired site set",
            ));
        }
        if policy.is_some()
            && (options.preview_urls.is_some() || self.sites.values().any(Result::is_err))
        {
            return Err(PublicationSetError::Selection(
                "candidate checking requires non-preview output and complete presentation acquisition",
            ));
        }
        if !self.sites.keys().eq(options.sites.keys()) {
            return Err(PublicationSetError::Selection(
                "render options must name exactly the acquired site set",
            ));
        }
        if options
            .preview_urls
            .as_ref()
            .is_some_and(|urls| !self.sites.keys().eq(urls.keys()))
        {
            return Err(PublicationSetError::Selection(
                "preview URLs must name exactly the acquired site set",
            ));
        }
        if options.preview_link_bases.is_some() && options.preview_urls.is_none() {
            return Err(PublicationSetError::Selection(
                "preview link bases require local preview URLs",
            ));
        }
        let preview_urls = options.preview_urls.map(Arc::new);
        let exclude_drafts =
            self.build_selection.local_drafts == Some(workspace::LocalDrafts::Exclude);
        let first = if options.selection.is_some()
            || options.derive_selection
            || preview_urls.is_some()
            || policy.is_some()
            || exclude_drafts
        {
            Some(&self.content)
        } else {
            self.sites.values().find_map(|site| site.as_ref().ok())
        };
        let parsed = first
            .map(|inputs| inputs.parse_publication(limits.publication.corpus, markdown))
            .transpose()
            .map_err(PublicationSetError::Input)?;
        let resolved = parsed
            .as_ref()
            .map(|parsed| parsed.resolve(limits.publication.graph))
            .transpose()
            .map_err(PublicationSetError::Resolution)?;
        let resolved = match (resolved, &preview_urls) {
            (Some(resolved), Some(urls)) => Some(
                resolved
                    .with_preview_urls(urls.as_ref().clone())
                    .map_err(PublicationSetError::Preview)?,
            ),
            (resolved, _) => resolved,
        };
        let mut resolved = resolved.map(|resolved| match options.preview_link_bases.clone() {
            Some(bases) => resolved.with_preview_link_bases(bases),
            None => resolved,
        });
        let selection = match options.selection {
            Some(selection) => Some(selection),
            None if policy.is_some() || options.derive_selection => Some(
                resolved
                    .as_ref()
                    .ok_or(PublicationSetError::Selection(
                        "candidate has no resolved model",
                    ))?
                    .authored()
                    .non_draft_selection()
                    .map_err(PublicationSetError::Candidate)?,
            ),
            None if exclude_drafts => Some(
                resolved
                    .as_ref()
                    .ok_or(PublicationSetError::Selection(
                        "local build has no resolved model",
                    ))?
                    .authored()
                    .non_draft_selection()
                    .map_err(PublicationSetError::Candidate)?,
            ),
            None => None,
        };
        let mut selected = match selection.as_ref() {
            Some(selection) => Some(
                resolved
                    .take()
                    .ok_or(PublicationSetError::Selection(
                        "candidate has no resolved model",
                    ))?
                    .into_selected(selection.clone())
                    .map_err(PublicationSetError::Candidate)?,
            ),
            None => None,
        };
        let checked = match policy {
            Some((runner, capture, policy)) => Some(
                super::policy::check_sites(
                    selected
                        .as_ref()
                        .ok_or(PublicationSetError::Selection("candidate has no selection"))?,
                    self.sites.iter().filter_map(|(site, inputs)| {
                        inputs.as_ref().ok().map(|inputs| (site.clone(), inputs))
                    }),
                    options.review_responses,
                    runner,
                    capture,
                    policy,
                    raster,
                    markdown,
                )
                .map_err(PublicationSetError::Policy)?,
            ),
            None => None,
        };
        let admitted = match checked
            .as_ref()
            .and_then(|checked| checked.admissions.clone())
        {
            Some(admissions) => Some(
                selected
                    .take()
                    .ok_or(PublicationSetError::Selection("candidate has no selection"))?
                    .into_admitted(admissions, self.sites.keys().cloned())
                    .map_err(|error| {
                        PublicationSetError::Policy(Box::new(super::CandidateCheckFailure {
                            completed: checked
                                .as_ref()
                                .expect("admissions require completed checks")
                                .checks
                                .iter()
                                .map(|(site, check)| (site.clone(), check.invocation()))
                                .collect(),
                            completed_responses: checked
                                .as_ref()
                                .expect("admissions require completed checks")
                                .checks
                                .iter()
                                .filter_map(|(site, check)| {
                                    check
                                        .response()
                                        .cloned()
                                        .map(|response| (site.clone(), response))
                                })
                                .collect(),
                            site: self
                                .sites
                                .keys()
                                .next()
                                .expect("nonempty acquired set")
                                .clone(),
                            cause: super::PolicyCheckError::Admission(error),
                        }))
                    })?,
            ),
            None => None,
        };
        let selection = admitted
            .as_ref()
            .map(|admitted| admitted.selected().selection())
            .or_else(|| selected.as_ref().map(super::SelectedPublication::selection))
            .map(|selection| Arc::new(selection.clone()));
        let candidate_evidence = checked
            .as_ref()
            .map(|checked| {
                checked
                    .checks
                    .iter()
                    .map(|(site, check)| {
                        let evidence = admitted
                            .as_ref()
                            .and_then(|admitted| admitted.admission(site))
                            .map(|admission| {
                                super::publication_model::CandidateEvidence::Admitted(Arc::clone(
                                    admission,
                                ))
                            })
                            .unwrap_or_else(|| {
                                super::publication_model::CandidateEvidence::Checked(Arc::clone(
                                    check,
                                ))
                            });
                        (site.clone(), evidence)
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();
        let mut capture_budget = super::social_cards::Budget::new(
            generation
                .as_ref()
                .map_or_else(Default::default, |value| value.options),
        );
        let mut bytes = limits.bytes;
        let mut entries = limits.entries;
        let mut sites = BTreeMap::new();
        for (site, mut options) in options.sites {
            options.limits.total_bytes = options.limits.total_bytes.min(bytes);
            options.limits.entries = options.limits.entries.min(entries);
            let result = match &self.sites[&site] {
                Err(error) => Err(PublicationSiteError::Acquisition(error)),
                Ok(inputs) => {
                    let rendered = super::social_cards::resume(
                        generation.as_ref(),
                        &mut capture_budget,
                        raster,
                        |social_card, mut social_card_limits| {
                            social_card_limits.total = bytes;
                            social_card_limits.assets = social_card_limits.assets.min(bytes);
                            social_card_limits.bytes = social_card_limits
                                .bytes
                                .min(usize::try_from(bytes).unwrap_or(usize::MAX));
                            social_card_limits.entries =
                                social_card_limits.entries.min(entries.saturating_sub(1));
                            let projection_inputs = super::page::ProjectionInputs {
                                social_card,
                                social_card_limits,
                                raster,
                                markdown,
                                discovery: None,
                            };
                            if let Some(admitted) = &admitted {
                                admitted.render_site_view(
                                    inputs,
                                    compiler,
                                    resources,
                                    projection_inputs,
                                    options.clone(),
                                    limits.publication.output,
                                    host_files,
                                )
                            } else if let Some(selected) = &selected {
                                selected.render_site_view(
                                    inputs,
                                    compiler,
                                    resources,
                                    projection_inputs,
                                    options.clone(),
                                    limits.publication.output,
                                    host_files,
                                )
                            } else {
                                resolved
                                    .as_ref()
                                    .ok_or(super::PublicationRenderError::Site(
                                        super::SiteRenderError::Configuration,
                                    ))?
                                    .render_site_view(
                                        inputs,
                                        compiler,
                                        resources,
                                        projection_inputs,
                                        options.clone(),
                                        limits.publication.output,
                                        host_files,
                                    )
                            }
                        },
                    );
                    rendered
                        .map(|mut rendered| {
                            rendered.selection.clone_from(&selection);
                            rendered.preview_urls.clone_from(&preview_urls);
                            rendered.candidate =
                                candidate_evidence
                                    .get(&site)
                                    .map(|evidence| match evidence {
                                        super::publication_model::CandidateEvidence::Checked(
                                            check,
                                        ) => super::publication_model::CandidateEvidence::Checked(
                                            Arc::clone(check),
                                        ),
                                        super::publication_model::CandidateEvidence::Admitted(
                                            admission,
                                        ) => super::publication_model::CandidateEvidence::Admitted(
                                            Arc::clone(admission),
                                        ),
                                    });
                            rendered
                        })
                        .map_err(PublicationSiteError::Render)
                }
            };
            if let Ok(rendered) = &result {
                let inventory = rendered.site().output().inventory();
                bytes -= inventory.total_bytes();
                entries -= inventory.entries().len();
            }
            if let Err(PublicationSiteError::Render(super::PublicationRenderError::Site(
                super::SiteRenderError::SocialCardCaptureRequired(request),
            ))) = &result
            {
                bytes = bytes.saturating_sub(request.retained_bytes());
                entries = entries.saturating_sub(request.retained_entries());
            }
            sites.insert(site, result);
        }
        Ok(RenderedPublicationSet {
            acquisition: self,
            selection,
            preview_urls,
            preview_link_bases: options.preview_link_bases,
            candidate_evidence,
            sites,
        })
    }
}

/// Payload bytes from the retained source inventories, excluding generated output.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationSourceBytes<'a> {
    /// Unique captured source-key total for the complete build.
    pub captured_total: u64,
    /// Common content graph input, shared by every selected site.
    pub shared_content: u64,
    /// Per-site input footprints overlap; summing them is not a disk-read total.
    pub sites: BTreeMap<&'a Key, SiteSourceBytes<'a>>,
}

/// Captured inputs used by one site, including the common content graph.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SiteSourceBytes<'a> {
    /// Available content and presentation bytes; consult `missing_sources` for completeness.
    pub captured: u64,
    /// Presentation bytes beyond the shared content graph, including package dependencies.
    pub presentation: u64,
    /// Failed source captures have unknown sizes, rather than a claimed zero-byte input.
    pub missing_sources: Vec<&'a Key>,
}

/// Complete membership with independent local outcomes. Failed sites contain no partial tree.
#[derive(Debug)]
pub struct RenderedPublicationSet<'a> {
    acquisition: &'a AcquiredPublicationSet,
    selection: Option<Arc<authored::Selection>>,
    preview_urls: Option<Arc<BTreeMap<Key, PublicBaseUrl>>>,
    preview_link_bases: Option<gazette_core::publication::routing::PreviewLinkBases>,
    candidate_evidence: BTreeMap<Key, super::publication_model::CandidateEvidence>,
    sites: BTreeMap<Key, Result<PublicationRender<'a>, PublicationSiteError<'a>>>,
}

impl RenderedPublicationSet<'_> {
    pub(super) fn compilation(&self) -> &workspace::Compilation {
        self.acquisition.compilation()
    }

    /// Actual zero-exit candidate invocations, including those whose later rendering failed.
    /// Empty means this operation did not run candidate checks, never implicit approval.
    pub fn candidate_checks(&self) -> BTreeMap<Key, super::PolicyInvocation> {
        self.candidate_evidence
            .iter()
            .map(|(site, evidence)| (site.clone(), evidence.check().invocation()))
            .collect()
    }
    pub(crate) fn into_preview_sites(
        self,
    ) -> Result<BTreeMap<Key, super::RenderedSite>, gazette_core::preview::Error> {
        if !self.is_complete() {
            return Err(gazette_core::preview::Error::Refused(
                "preview generation requires the exact complete rendered site set",
            ));
        }
        self.sites
            .into_iter()
            .map(|(key, result)| {
                result
                    .map(|rendered| (key, rendered.into_site()))
                    .map_err(|_| gazette_core::preview::Error::Refused("incomplete preview site"))
            })
            .collect()
    }

    /// Exact ephemeral serving map. Presence marks developer-local output and
    /// never changes the retained configuration or establishes a production URL.
    pub fn preview_urls(&self) -> Option<&BTreeMap<Key, PublicBaseUrl>> {
        self.preview_urls.as_deref()
    }

    /// Exact optional public hyperlink projection used by this local generation.
    pub fn preview_link_bases(
        &self,
    ) -> Option<&gazette_core::publication::routing::PreviewLinkBases> {
        self.preview_link_bases.as_ref()
    }

    /// Complete candidate intent, shared with successful site receipts and retained
    /// even when every site failed. None denotes unfiltered private rendering.
    pub fn selection(&self) -> Option<&authored::Selection> {
        self.selection.as_deref()
    }

    /// Requested membership is preserved even when some or all site renders fail.
    pub fn sites(&self) -> &BTreeMap<Key, Result<PublicationRender<'_>, PublicationSiteError<'_>>> {
        &self.sites
    }
    /// True only when every requested site produced local output; never release settlement.
    pub fn is_complete(&self) -> bool {
        self.sites.values().all(Result::is_ok)
    }
    /// Private source observations and output evidence, not a certified artifact identity.
    /// Native failure messages and paths may be confidential or observation-dependent.
    pub fn inspect(&self) -> PublicationSetInspection<'_> {
        PublicationSetInspection {
            build_selection: self.acquisition.build_selection(),
            configuration: self.acquisition.configuration_evidence(),
            math: self.acquisition.math_capture(),
            diagrams: self.acquisition.diagram_capture(),
            candidate_checks: self.candidate_checks(),
            selection: self.selection(),
            preview_urls: self.preview_urls(),
            preview_link_bases: self.preview_link_bases(),
            compilation: self.acquisition.compilation(),
            sources: self.acquisition.shared.source_evidence(),
            source_bytes: self.acquisition.source_bytes(),
            source_failures: self
                .acquisition
                .source_failures
                .iter()
                .map(|(key, error)| {
                    (
                        key,
                        PublicationSourceFailure {
                            code: error.code(),
                            message: error.to_string(),
                        },
                    )
                })
                .collect(),
            status: if self.is_complete() {
                PublicationSetStatus::Rendered
            } else {
                PublicationSetStatus::Incomplete
            },
            sites: self
                .sites
                .iter()
                .map(|(key, outcome)| {
                    (
                        key,
                        match outcome {
                            Ok(rendered) => PublicationSetSiteInspection::Rendered {
                                inventory: rendered.site().output().inventory(),
                                evidence: rendered.site().evidence(),
                            },
                            Err(error) => PublicationSetSiteInspection::Failed {
                                code: error.code(),
                                message: error.to_string(),
                            },
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Closed structural result of one requested local publication set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum PublicationSetStatus {
    /// Every requested local render succeeded.
    #[serde(rename = "local_set_rendered")]
    Rendered,
    /// At least one requested local render failed.
    #[serde(rename = "local_set_incomplete")]
    Incomplete,
}

/// Private result shared by interfaces; not an authorized release set or certification.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PublicationSetInspection<'a> {
    /// Effective site-set owner and local draft policy, not an authorization receipt.
    pub build_selection: &'a workspace::BuildSelection,
    /// Exact supplied document or selected native document-set evidence.
    pub configuration: &'a crate::configuration::Evidence,
    /// Exact retained native diagram results, separate from authored source facts.
    pub diagrams: Option<&'a super::DiagramCapture>,
    /// Exact private generation evidence, excluded from public output projections.
    pub math: Option<&'a super::MathCapture>,
    /// Private pre-render invocation evidence, separate from artifacts and output checks.
    pub candidate_checks: BTreeMap<Key, super::PolicyInvocation>,
    /// Exact local URL projection, or none for configured public-link rendering.
    pub preview_urls: Option<&'a BTreeMap<Key, PublicBaseUrl>>,
    /// Exact preview-only ordinary hyperlink bases; omitted from public page models.
    pub preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    /// Exact candidate intent, including exclusions; never publication permission.
    pub selection: Option<&'a authored::Selection>,
    /// The single compiled configuration from which the requested site views were selected.
    pub compilation: &'a workspace::Compilation,
    /// Shared exact repository/gitlink and captured inventory evidence, without payloads.
    pub sources: BTreeMap<&'a Key, super::SourceEvidence<'a>>,
    /// Actual retained payload sizes; shared inputs are captured once, not reread for each site.
    pub source_bytes: PublicationSourceBytes<'a>,
    /// All failed presentation sources, even when one site depends on several failures.
    pub source_failures: BTreeMap<&'a Key, PublicationSourceFailure>,
    /// All requested local renders succeeded, or at least one failed.
    pub status: PublicationSetStatus,
    /// Exact requested keys including failures, in stable order.
    pub sites: BTreeMap<&'a Key, PublicationSetSiteInspection<'a>>,
}

/// Private evidence of one failed exact source; never a successful input or stale fallback.
#[derive(Serialize, schemars::JsonSchema)]
pub struct PublicationSourceFailure {
    /// Original native or aggregate failure class.
    pub code: &'static str,
    /// Source-key-bound private diagnostic.
    pub message: String,
}

/// Failure at the actual site phase, retaining its original native diagnostic.
#[derive(Debug)]
pub enum PublicationSiteError<'a> {
    /// At least one required presentation source did not acquire successfully.
    Acquisition(&'a CompositionError),
    /// Presentation validation, compilation or output construction failed.
    Render(PublicationRenderError),
}

impl PublicationSiteError<'_> {
    /// Original failure class; inspecting this error never retries native work.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Acquisition(error) => error.code(),
            Self::Render(error) => error.code(),
        }
    }
}
impl fmt::Display for PublicationSiteError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Acquisition(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PublicationSiteError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Acquisition(error) => Some(*error),
            Self::Render(error) => Some(error),
        }
    }
}

/// Either complete immutable local output evidence or the site's original failure class.
#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PublicationSetSiteInspection<'a> {
    /// This site's complete output, not public admission.
    Rendered {
        /// Exact local tree identity and inventory.
        inventory: &'a Inventory,
        /// Render bindings used for this tree.
        evidence: &'a SiteRenderEvidence,
    },
    /// No partial output for this site is retained.
    Failed {
        /// Existing application-owned stable error code.
        code: &'static str,
        /// Private native diagnostic, never public Website copy.
        message: String,
    },
}

/// A shared prerequisite failed; no rendered subset is returned as a successful set.
#[derive(Debug)]
pub enum PublicationSetError {
    /// One selected publication's configured documentation could not be captured or assembled.
    Documentation {
        /// Exact consuming publication.
        site: Key,
        /// Native documentation refusal, including required generation.
        error: Box<super::DocumentationError>,
    },
    /// One exact site's direct-local capability capture failed after source acquisition.
    Capability {
        /// Exact consuming site; set errors never erase association.
        site: Key,
        /// Application-owned acquisition refusal.
        error: Box<super::CapabilityAcquisitionError>,
    },
    /// Explicit supplied or selected-document workspace resolution failed.
    Configuration(crate::configuration::Error),
    /// Explicit pre-render native diagram acquisition failed; no automatic retry.
    Diagram(Box<super::DiagramGenerationError>),
    /// Explicit pre-render native math generation failed; no automatic retry.
    Math(Box<super::MathGenerationError>),
    /// Candidate projection or checker failure before rendering; may retain external effects.
    Policy(Box<super::CandidateCheckFailure>),
    /// Session serving-map validation failed; no local projection was returned.
    Preview(gazette_core::publication::content::Error),
    /// Empty, duplicate, over-limit or mismatched exact site selection.
    Selection(&'static str),
    /// Configuration or required shared content acquisition failed.
    Acquisition(CompositionError),
    /// Shared syntax/source mapping failed.
    Input(PublicationInputError),
    /// Shared content semantics failed.
    Resolution(PublicationResolutionError),
    /// Complete candidate selection failed before any site program ran.
    Candidate(authored::SelectionError),
}

impl PublicationSetError {
    /// Preserve the native failure class for shared prerequisites.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Documentation { error, .. } => error.code(),
            Self::Capability { error, .. } => error.code(),
            Self::Configuration(error) => error.code(),
            Self::Math(error) => error.code(),
            Self::Diagram(error) => error.code(),
            Self::Policy(error) => error.cause.code(),
            Self::Preview(_) => "publication_preview_urls",
            Self::Selection(_) => "publication_set_selection",
            Self::Acquisition(error) => error.code(),
            Self::Input(error) => error.code(),
            Self::Resolution(error) => error.code(),
            Self::Candidate(error) => error.code(),
        }
    }
}
impl fmt::Display for PublicationSetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Documentation { site, error } => write!(f, "site {site}: {error}"),
            Self::Capability { site, error } => write!(f, "site {site}: {error}"),
            Self::Configuration(error) => error.fmt(f),
            Self::Math(error) => error.fmt(f),
            Self::Diagram(error) => error.fmt(f),
            Self::Policy(error) => error.fmt(f),
            Self::Preview(error) => error.fmt(f),
            Self::Selection(reason) => write!(
                f,
                "{reason}; select an exact nonempty bounded site set; no output produced"
            ),
            Self::Acquisition(error) => error.fmt(f),
            Self::Input(error) => error.fmt(f),
            Self::Resolution(error) => error.fmt(f),
            Self::Candidate(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for PublicationSetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Documentation { error, .. } => Some(error.as_ref()),
            Self::Capability { error, .. } => Some(error.as_ref()),
            Self::Configuration(error) => Some(error),
            Self::Math(error) => Some(error.as_ref()),
            Self::Diagram(error) => Some(error.as_ref()),
            Self::Policy(error) => Some(error.as_ref()),
            Self::Preview(error) => Some(error),
            Self::Selection(_) => None,
            Self::Acquisition(error) => Some(error),
            Self::Input(error) => Some(error),
            Self::Resolution(error) => Some(error),
            Self::Candidate(error) => Some(error),
        }
    }
}
