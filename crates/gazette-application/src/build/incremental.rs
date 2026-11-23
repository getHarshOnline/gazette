//! Whole-site persistent reuse over exact typed dependencies; clean rendering remains canonical.

use std::{collections::BTreeMap, fmt, path::Path};

use gazette_core::{
    destination,
    publication::Key,
    store::{
        self, BuildCacheDependencySet, BuildCacheInput, BuildCacheReceipt, BuildCacheSiteInput,
        BuildCacheWriteOptions, CachedBuildSite, JournalHeader, ReadOptions, WriterOptions,
    },
};
use gazette_ports::{
    destination::RootSeparation,
    markdown::Markdown,
    render::TemplateCompiler,
    resource::ResourceCompiler,
    store::{BuildCacheReader, BuildCacheWriter, JournalWriter, WriterConnector},
};

use super::{
    AcquiredInputs, AcquiredPublicationSet, PublicationRenderError, PublicationSetError,
    PublicationSetRenderLimits, PublicationSetRenderOptions, RenderedPublicationSet,
};

/// Application-derived cache lookup bound to exact retained build evidence.
///
/// Callers may carry this value but cannot construct or alter its dependency closure.
/// It remains rebuild evidence, never admission, certification or release authority.
///
/// ```compile_fail
/// use gazette_application::build::BuildCacheKey;
/// let _forged = BuildCacheKey { site: todo!(), dependencies: todo!() };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildCacheKey {
    site: Key,
    dependencies: BuildCacheDependencySet,
}

impl BuildCacheKey {
    /// Exact site whose retained evidence produced this key.
    pub fn site(&self) -> &Key {
        &self.site
    }

    fn dependencies(&self) -> &BuildCacheDependencySet {
        &self.dependencies
    }
}

/// Complete application-owned pre-render dependency plan for one exact site set.
#[derive(Debug, PartialEq, Eq)]
pub struct BuildCachePlan {
    keys: BTreeMap<Key, BuildCacheKey>,
    configuration_sha256: gazette_core::source::Sha256Digest,
    selection_sha256: gazette_core::source::Sha256Digest,
}

/// Clean render produced only after the application revalidated the complete plan.
#[derive(Debug)]
pub struct BuildCacheClean<'a> {
    plan: BuildCachePlan,
    rendered: RenderedPublicationSet<'a>,
}

impl<'a> BuildCacheClean<'a> {
    /// Exact verified pre-render plan used for this clean render.
    pub fn plan(&self) -> &BuildCachePlan {
        &self.plan
    }

    /// Complete clean output for downstream inspection and qualification.
    pub fn rendered(&self) -> &RenderedPublicationSet<'a> {
        &self.rendered
    }
}

impl BuildCachePlan {
    /// Exact derived key for a selected site.
    pub fn key(&self, site: &Key) -> Option<&BuildCacheKey> {
        self.keys.get(site)
    }

    /// Complete selected site keys in deterministic order.
    pub fn sites(&self) -> impl ExactSizeIterator<Item = &Key> {
        self.keys.keys()
    }
}

/// Pre-render dependency planning failed before any cache or output effect.
#[derive(Debug)]
pub enum BuildCachePlanError {
    /// Shared syntax, semantic selection or exact-set validation failed.
    Publication(PublicationSetError),
    /// One site's retained Theme could not be checked and compiled.
    Site {
        /// Exact failed site.
        site: Key,
        /// Existing native application failure.
        cause: PublicationRenderError,
    },
    /// Compiler/adapter profiles or retained evidence could not form a canonical identity.
    Evidence,
}

/// A planned clean build failed before it could become cache-write eligible.
#[derive(Debug)]
pub enum BuildCacheRunError {
    /// Current retained evidence could not produce a valid plan.
    Plan(BuildCachePlanError),
    /// Build adapters, options, limits or selected semantics changed after planning.
    Changed,
    /// The exact verified clean render failed.
    Render(PublicationSetError),
}

impl BuildCacheRunError {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Plan(error) => error.code(),
            Self::Changed => "build_cache_dependencies",
            Self::Render(error) => error.code(),
        }
    }
}

impl fmt::Display for BuildCacheRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => error.fmt(formatter),
            Self::Changed => formatter.write_str(
                "cache plan changed before rendering; no render or cache write attempted",
            ),
            Self::Render(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for BuildCacheRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Changed => None,
        }
    }
}

impl BuildCachePlanError {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Publication(error) => error.code(),
            Self::Site { cause, .. } => cause.code(),
            Self::Evidence => "build_cache_dependencies",
        }
    }
}

impl fmt::Display for BuildCachePlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Publication(error) => error.fmt(formatter),
            Self::Site { site, cause } => write!(formatter, "cache plan for {site}: {cause}"),
            Self::Evidence => formatter.write_str(
                "cache dependency evidence or build-mechanism profile is invalid; no cache read attempted",
            ),
        }
    }
}

impl std::error::Error for BuildCachePlanError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Publication(error) => Some(error),
            Self::Site { cause, .. } => Some(cause),
            Self::Evidence => None,
        }
    }
}

#[derive(serde::Serialize)]
struct GraphOptionsEvidence<'a> {
    output: &'a gazette_core::source::EntryPath,
    limits: gazette_core::publication::content::Limits,
}

#[derive(serde::Serialize)]
struct BuildMechanismEvidence<'a> {
    template_compiler_profile: &'static str,
    compiled_template_tool: &'static str,
    resource_compiler_profile: &'static str,
    raster_decoder_profile: &'static str,
    markdown_adapter_profile: &'static str,
    corpus_limits: gazette_compiler::corpus::Limits,
    graph_limits: gazette_core::publication::content::Limits,
    metadata_limits: gazette_compiler::toml::Limits,
    template_limits: gazette_core::render::Limits,
    resource_limits: gazette_core::resource::Limits,
    resource_metadata_bytes: usize,
    aggregate_output_bytes: u64,
    aggregate_output_entries: usize,
    render_year: Option<gazette_compiler::authored::RenderYear>,
    model_output: Option<&'a gazette_core::source::EntryPath>,
    graph: Option<GraphOptionsEvidence<'a>>,
    output_limits: gazette_core::source::Limits,
    preview_urls: Option<&'a BTreeMap<Key, gazette_core::publication::routing::PublicBaseUrl>>,
    preview_link_bases: Option<&'a gazette_core::publication::routing::PreviewLinkBases>,
    math: Option<&'a super::MathCapture>,
    diagrams: Option<&'a super::DiagramCapture>,
}

/// Exact compiler and adapter set shared by planning and the verified clean render.
pub struct BuildCacheAdapters<'a, T, R> {
    /// Captured-template compiler.
    pub templates: &'a T,
    /// Captured resource compiler.
    pub resources: &'a R,
    /// Pure retained raster inspector.
    pub raster: &'a gazette_core::raster::Inspect<'a>,
    /// Qualified retained-raster decoder profile.
    pub raster_profile: &'static str,
    /// Retained-source Markdown adapter.
    pub markdown: &'a dyn Markdown,
}

/// Application decision for one exact site dependency closure.
#[derive(Debug)]
pub enum BuildCacheSchedule {
    /// No exact artifact exists; run the normal clean renderer.
    Clean,
    /// Complete output was reconstructed and verified from the rebuildable projection.
    Warm(CachedBuildSite),
}

/// Inspect and schedule one site. A mismatch is a clean miss, never stale fallback.
///
/// # Errors
/// Refuses unsupported/corrupt state, exceeded bounds or an expected-header mismatch.
pub fn schedule_build_cache_site(
    reader: &impl BuildCacheReader,
    database: &Path,
    expected: Option<&JournalHeader>,
    key: &BuildCacheKey,
    options: ReadOptions,
) -> Result<BuildCacheSchedule, store::Error> {
    options.validate()?;
    let inspection = reader.inspect_build_cache(database, options)?;
    if expected.is_some_and(|expected| *expected != inspection.journal.header) {
        return Err(store::Error::JournalMismatch);
    }
    reader
        .load_build_cache_site(database, key.site(), key.dependencies(), options)
        .map(|site| site.map_or(BuildCacheSchedule::Clean, BuildCacheSchedule::Warm))
}

/// Independent replacement and writer policies for the rebuildable cache.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct BuildCacheReconcileOptions {
    /// Explicit complete-set replacement permission.
    pub cache: BuildCacheWriteOptions,
    /// Existing store generation/reservation policy.
    pub writer: WriterOptions,
}

/// Cache preparation or effect failure, preserving whether writer acquisition occurred.
#[derive(Debug)]
pub enum BuildCacheError {
    /// Dependency coverage does not equal the complete successful rendered set.
    Dependencies,
    /// Exact retained evidence could not be encoded into a dependency identity.
    Evidence,
    /// Permission or native-bound refusal before acquisition.
    Policy(store::Error),
    /// Source/store root separation failed.
    Separation {
        /// Native path/identity refusal.
        cause: destination::Error,
        /// Whether the local writer generation was already acquired.
        writer_acquired: bool,
    },
    /// Native writer acquisition failed with its own generation evidence.
    Acquire(store::WriterAcquireError),
    /// Complete-set cache mutation or read-back failed.
    Write(store::Error),
    /// Replacement verified but final writer close failed.
    Close {
        /// Verified cache receipt.
        receipt: Box<BuildCacheReceipt>,
        /// Native close failure.
        cause: store::Error,
    },
}

impl BuildCacheError {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Dependencies | Self::Evidence => "build_cache_dependencies",
            Self::Policy(error) | Self::Write(error) => error.code(),
            Self::Separation { .. } => "build_cache_source_separation",
            Self::Acquire(error) => error.cause.code(),
            Self::Close { cause, .. } => cause.code(),
        }
    }
}

impl fmt::Display for BuildCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dependencies => formatter.write_str("build-cache dependencies must name exactly every successful rendered site; no cache write attempted"),
            Self::Evidence => formatter.write_str("build-cache dependency evidence could not be encoded; no cache read or write attempted"),
            Self::Policy(error) => write!(formatter, "{error}; no writer acquired"),
            Self::Separation { cause, writer_acquired } => write!(formatter, "{cause}; writer acquired: {writer_acquired}; cache is rebuildable but release history must remain untouched"),
            Self::Acquire(error) => error.fmt(formatter),
            Self::Write(error) => write!(formatter, "{error}; writer generation was already advanced; inspect before another replacement"),
            Self::Close { receipt, cause } => write!(formatter, "{cause}; cache {} was verified at writer generation {}, but close failed", receipt.cache.rows_sha256, receipt.journal.writer_fence),
        }
    }
}

impl std::error::Error for BuildCacheError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Dependencies | Self::Evidence => None,
            Self::Policy(error) | Self::Write(error) => Some(error),
            Self::Separation { cause, .. } => Some(cause),
            Self::Acquire(error) => Some(error),
            Self::Close { cause, .. } => Some(cause),
        }
    }
}

impl AcquiredPublicationSet {
    /// Resolve exact semantics and compile selected Theme programs without rendering pages.
    ///
    /// The returned keys are derived only from retained acquisition, configuration,
    /// selected semantics, Theme/template identity, adapter profiles and explicit limits.
    /// Cached output bytes are deliberately excluded, allowing an exact warm hit to bypass
    /// resource and page rendering while storage still verifies cached artifact bytes.
    ///
    /// # Errors
    /// Refuses incomplete acquisition, mismatched sets, checked-policy effects, invalid
    /// semantics, Theme compilation or malformed compiler/adapter evidence.
    pub fn plan_build_cache(
        &self,
        adapters: BuildCacheAdapters<'_, impl TemplateCompiler, impl ResourceCompiler>,
        options: &PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
    ) -> Result<BuildCachePlan, BuildCachePlanError> {
        let BuildCacheAdapters {
            templates,
            resources,
            raster_profile,
            markdown,
            ..
        } = adapters;
        if options.review_responses.is_some() {
            return Err(BuildCachePlanError::Publication(
                PublicationSetError::Selection(
                    "cache planning cannot bypass configured candidate-check effects",
                ),
            ));
        }
        if !self.sites().keys().eq(options.sites.keys())
            || self.sites().values().any(Result::is_err)
            || options
                .preview_urls
                .as_ref()
                .is_some_and(|urls| !self.sites().keys().eq(urls.keys()))
        {
            return Err(BuildCachePlanError::Publication(
                PublicationSetError::Selection(
                    "cache planning requires the exact complete acquired site set",
                ),
            ));
        }
        for profile in [
            templates.profile(),
            resources.profile(),
            raster_profile,
            markdown.profile(),
        ] {
            if profile.is_empty()
                || profile.len() > 256
                || !profile.bytes().all(|byte| byte.is_ascii_graphic())
            {
                return Err(BuildCachePlanError::Evidence);
            }
        }
        let inputs = self
            .sites()
            .values()
            .find_map(|site| site.as_ref().ok())
            .ok_or(BuildCachePlanError::Evidence)?;
        let parsed = inputs
            .parse_publication(limits.publication.corpus, markdown)
            .map_err(|error| BuildCachePlanError::Publication(PublicationSetError::Input(error)))?;
        let resolved = parsed.resolve(limits.publication.graph).map_err(|error| {
            BuildCachePlanError::Publication(PublicationSetError::Resolution(error))
        })?;
        let semantic_sha256 = match &options.selection {
            Some(selection) => {
                let selected = resolved.into_selected(selection.clone()).map_err(|error| {
                    BuildCachePlanError::Publication(PublicationSetError::Candidate(error))
                })?;
                digest(&(selected.authored().content().graph(), selection))
                    .map_err(|_| BuildCachePlanError::Evidence)?
            }
            None => digest(&(resolved.authored().content().graph(), &options.selection))
                .map_err(|_| BuildCachePlanError::Evidence)?,
        };
        let selection_sha256 = digest(&(
            &options.selection,
            &options.preview_urls,
            &options.preview_link_bases,
        ))
        .map_err(|_| BuildCachePlanError::Evidence)?;
        let configuration_sha256 = self.compilation().source_sha256;
        let mut keys = BTreeMap::new();
        for (site, acquired) in self.sites() {
            let acquired = acquired
                .as_ref()
                .map_err(|_| BuildCachePlanError::Evidence)?;
            let site_options = &options.sites[site];
            let metadata = acquired
                .check_metadata(limits.publication.output.metadata)
                .map_err(|cause| BuildCachePlanError::Site {
                    site: site.clone(),
                    cause: PublicationRenderError::Metadata(cause),
                })?;
            let theme = metadata
                .compile_templates(templates, limits.publication.output.render)
                .map_err(|cause| BuildCachePlanError::Site {
                    site: site.clone(),
                    cause: PublicationRenderError::Template(cause),
                })?;
            let source =
                source_digest(site, acquired).map_err(|_| BuildCachePlanError::Evidence)?;
            let mechanisms = digest(&BuildMechanismEvidence {
                template_compiler_profile: templates.profile(),
                compiled_template_tool: theme.evidence().tool,
                resource_compiler_profile: resources.profile(),
                raster_decoder_profile: raster_profile,
                markdown_adapter_profile: markdown.profile(),
                corpus_limits: limits.publication.corpus,
                graph_limits: limits.publication.graph,
                metadata_limits: limits.publication.output.metadata,
                template_limits: limits.publication.output.render,
                resource_limits: limits.publication.output.resources,
                resource_metadata_bytes: limits.publication.output.resource_metadata_bytes,
                aggregate_output_bytes: limits.bytes,
                aggregate_output_entries: limits.entries,
                render_year: site_options.render_year,
                model_output: site_options.model_output,
                graph: site_options
                    .graph
                    .as_ref()
                    .map(|graph| GraphOptionsEvidence {
                        output: graph.output,
                        limits: graph.limits,
                    }),
                output_limits: site_options.limits,
                preview_urls: options.preview_urls.as_ref(),
                preview_link_bases: options.preview_link_bases.as_ref(),
                math: acquired.math_capture(),
                diagrams: acquired.diagram_capture(),
            })
            .map_err(|_| BuildCachePlanError::Evidence)?;
            let selected = digest(&(site, semantic_sha256, selection_sha256))
                .map_err(|_| BuildCachePlanError::Evidence)?;
            let dependencies = dependency_set(
                source,
                configuration_sha256,
                theme.evidence().template_tree.sha256(),
                mechanisms,
                selected,
            )
            .map_err(|_| BuildCachePlanError::Evidence)?;
            keys.insert(
                site.clone(),
                BuildCacheKey {
                    site: site.clone(),
                    dependencies,
                },
            );
        }
        Ok(BuildCachePlan {
            keys,
            configuration_sha256,
            selection_sha256,
        })
    }
}

impl BuildCachePlan {
    /// Revalidate the complete plan and run the exact clean renderer under the same adapters.
    ///
    /// # Errors
    /// Refuses any changed adapter, option, limit, selected semantic input or retained source
    /// before rendering; then preserves the normal complete-set render failure.
    pub fn render_clean<'a>(
        self,
        inputs: &'a AcquiredPublicationSet,
        adapters: BuildCacheAdapters<'_, impl TemplateCompiler, impl ResourceCompiler>,
        options: PublicationSetRenderOptions<'_>,
        limits: PublicationSetRenderLimits,
    ) -> Result<BuildCacheClean<'a>, BuildCacheRunError> {
        let BuildCacheAdapters {
            templates,
            resources,
            raster,
            raster_profile,
            markdown,
        } = adapters;
        let verified = inputs
            .plan_build_cache(
                BuildCacheAdapters {
                    templates,
                    resources,
                    raster,
                    raster_profile,
                    markdown,
                },
                &options,
                limits,
            )
            .map_err(BuildCacheRunError::Plan)?;
        if verified != self {
            return Err(BuildCacheRunError::Changed);
        }
        let rendered = inputs
            .render_local(templates, resources, raster, options, limits, markdown)
            .map_err(BuildCacheRunError::Render)?;
        Ok(BuildCacheClean {
            plan: self,
            rendered,
        })
    }
}

impl BuildCacheClean<'_> {
    /// Replace the complete rebuildable site cache from clean rendered output.
    ///
    /// Cached output never becomes certification, publication admission or release evidence.
    /// A later warm lookup must present the exact same typed dependency set.
    ///
    /// # Errors
    /// Refuses incomplete renders/dependencies, unsafe roots and uncertain native effects.
    pub fn reconcile_build_cache<C: WriterConnector>(
        &self,
        connector: &C,
        separation: &impl RootSeparation,
        root: &Path,
        expected: JournalHeader,
        options: BuildCacheReconcileOptions,
    ) -> Result<BuildCacheReceipt, BuildCacheError>
    where
        C::Writer: BuildCacheWriter,
    {
        options.cache.validate().map_err(BuildCacheError::Policy)?;
        options.writer.validate().map_err(BuildCacheError::Policy)?;
        let rendered = &self.rendered;
        let plan = &self.plan;
        if !rendered.is_complete()
            || rendered.compilation().source_sha256 != plan.configuration_sha256
            || !rendered.sites().keys().eq(plan.keys.keys())
            || digest(&(
                rendered.selection(),
                rendered.preview_urls(),
                rendered.preview_link_bases(),
            ))
            .map_err(|_| BuildCacheError::Evidence)?
                != plan.selection_sha256
        {
            return Err(BuildCacheError::Dependencies);
        }
        let mut roots = std::collections::BTreeSet::new();
        let mut sites = Vec::with_capacity(rendered.sites().len());
        for (key, outcome) in rendered.sites() {
            let rendered = outcome
                .as_ref()
                .map_err(|_| BuildCacheError::Dependencies)?;
            if source_digest(key, rendered.inputs())?
                != dependency_sha256(
                    plan.keys[key].dependencies(),
                    store::BuildCacheDependencyKind::Source,
                )?
                || rendered.site().evidence().theme.template_tree.sha256()
                    != dependency_sha256(
                        plan.keys[key].dependencies(),
                        store::BuildCacheDependencyKind::ThemeTemplate,
                    )?
            {
                return Err(BuildCacheError::Dependencies);
            }
            roots.extend(
                rendered
                    .inputs()
                    .source_roots()
                    .map_err(|_| BuildCacheError::Dependencies)?,
            );
            sites.push(BuildCacheSiteInput {
                site: key,
                dependencies: plan.keys[key].dependencies(),
                output: rendered.site().output(),
                media_types: rendered.site().media_types(),
            });
        }
        let roots: Vec<_> = roots.into_iter().collect();
        separation
            .check_root_separation(root, &roots)
            .map_err(|cause| BuildCacheError::Separation {
                cause,
                writer_acquired: false,
            })?;
        let mut writer = connector
            .acquire_writer(root, expected, options.writer)
            .map_err(BuildCacheError::Acquire)?;
        separation
            .check_root_separation(root, &roots)
            .map_err(|cause| BuildCacheError::Separation {
                cause,
                writer_acquired: true,
            })?;
        let receipt = writer
            .replace_build_cache(BuildCacheInput { sites }, options.cache)
            .map_err(BuildCacheError::Write)?;
        if let Err(cause) = writer.close() {
            return Err(BuildCacheError::Close {
                receipt: Box::new(receipt),
                cause,
            });
        }
        Ok(receipt)
    }
}

fn source_digest(
    site: &Key,
    inputs: &AcquiredInputs,
) -> Result<gazette_core::source::Sha256Digest, BuildCacheError> {
    let inspection = inputs.inspect();
    digest(&(
        site,
        &inspection.sources,
        inputs
            .documentation_catalog()
            .map(super::DocumentationCatalog::evidence_sha256),
    ))
}

fn dependency_set(
    source: gazette_core::source::Sha256Digest,
    configuration: gazette_core::source::Sha256Digest,
    theme: gazette_core::source::Sha256Digest,
    mechanisms: gazette_core::source::Sha256Digest,
    semantic: gazette_core::source::Sha256Digest,
) -> Result<BuildCacheDependencySet, BuildCacheError> {
    BuildCacheDependencySet::new([
        dependency(
            store::BuildCacheDependencyKind::Source,
            "retained-source-set",
            source,
        )?,
        dependency(
            store::BuildCacheDependencyKind::Configuration,
            "compiled-workspace",
            configuration,
        )?,
        dependency(
            store::BuildCacheDependencyKind::ThemeTemplate,
            "selected-theme-template-tree",
            theme,
        )?,
        dependency(
            store::BuildCacheDependencyKind::BuildMechanism,
            "render-mechanism-closure",
            mechanisms,
        )?,
        dependency(
            store::BuildCacheDependencyKind::SelectedSemanticInput,
            "selected-semantic-closure",
            semantic,
        )?,
    ])
    .map_err(BuildCacheError::Policy)
}

fn dependency_sha256(
    dependencies: &BuildCacheDependencySet,
    kind: store::BuildCacheDependencyKind,
) -> Result<gazette_core::source::Sha256Digest, BuildCacheError> {
    dependencies
        .iter()
        .find(|dependency| dependency.kind == kind)
        .map(|dependency| dependency.sha256)
        .ok_or(BuildCacheError::Dependencies)
}

fn dependency(
    kind: store::BuildCacheDependencyKind,
    identity: &'static str,
    sha256: gazette_core::source::Sha256Digest,
) -> Result<store::BuildCacheDependency, BuildCacheError> {
    store::BuildCacheDependency::new(kind, identity, sha256).map_err(BuildCacheError::Policy)
}

fn digest(
    value: &impl serde::Serialize,
) -> Result<gazette_core::source::Sha256Digest, BuildCacheError> {
    serde_json::to_vec(value)
        .map(|bytes| gazette_core::source::Sha256Digest::of(&bytes))
        .map_err(|_| BuildCacheError::Evidence)
}
