//! Complete local site assembly; no partial tree or public-admission claim escapes.

use std::collections::{BTreeMap, BTreeSet};

use gazette_compiler::authored::{
    AssetError, Origin, ResolvedAuthored, SiteGraphError, SitePagesError, StaticAsset,
};
use gazette_compiler::workspace::FeedFormat;
use gazette_core::{
    publication::{CapabilityId, Key, content, routing::RoutePath},
    render::{self, MediaType},
    resource::{self, ResourceId},
    source::{self, CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    CapabilityEvidence, CapabilityProjection, CompiledTheme, MirrorRenderEvidence, PageRenderError,
    PageResourceEvidence, TemplateCompilation,
    capability_status::ResolvedCapabilities,
    page_resources::CompiledPageResources,
    resources::CompiledResources,
    serialization::{context, encode},
};

mod json_ld;
mod page_cache;
use json_ld::SiteJsonLd;

pub(super) struct SiteRenderContext<'services, 'page> {
    pub capabilities: &'services ResolvedCapabilities,
    pub resources: &'services CompiledResources,
    pub page_resources: &'services CompiledPageResources,
    pub raster: &'services gazette_core::raster::Inspect<'services>,
    pub markdown: &'services dyn gazette_ports::markdown::Markdown,
    pub host_files: Option<&'services dyn gazette_ports::hosting::HostFiles>,
    pub prepared: Option<super::islands::PreparedPages<'page>>,
    pub social_card: Option<&'services super::social_cards::Capture>,
    pub social_card_limits: super::social_cards::RequestLimits,
    pub documentation_catalog: Option<&'services super::DocumentationCatalog>,
}

/// Explicit output choices; the compiled Theme owns ordinary render entries.
/// There is no kind/filename fallback. Same-site aliases receive the page model;
/// cross-site mirrors receive only `MirrorModel`. Both use `route.canonical_url`
/// as their target; actual redirect behavior is Theme-owned.
#[derive(Clone)]
pub struct SiteRenderOptions<'a> {
    /// Explicit presentation year; omission never reads a clock in pure rendering.
    pub render_year: Option<gazette_compiler::authored::RenderYear>,
    /// Optional site-relative JSON model artifact; selected collisions are fatal.
    /// Omission does not remove the shared model or any configured projection.
    pub model_output: Option<&'a EntryPath>,
    /// Optional explicit local graph view; the identical model also enters site JSON.
    pub graph: Option<GraphRenderOptions<'a>>,
    /// Bound the complete output inventory and every retained file.
    pub limits: source::Limits,
}

/// Theme consumption of the shared local structural graph, not public admission.
#[derive(Clone)]
pub struct GraphRenderOptions<'a> {
    /// Exact site-relative output, checked with all other output paths.
    pub output: &'a EntryPath,
    /// Complete graph ceilings; exceeding them never truncates relationships.
    pub limits: content::Limits,
}

/// Private graph-view evidence binds the exact shared model and rendered bytes.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct GraphRenderEvidence {
    /// Explicit Theme entry.
    pub template: EntryPath,
    /// Exact generated output.
    pub output: EntryPath,
    /// Compact encoding of the same value supplied to Theme and site JSON.
    pub model_sha256: Sha256Digest,
    /// Exact emitted UTF-8 view bytes, not proof of browser behavior.
    pub output_sha256: Sha256Digest,
}

/// One complete unadmitted local tree over the supplied model. Unfiltered models
/// retain drafts; selected candidates do not. Native staging, public disclosure,
/// certified artifacts and release remain distinct operations.
#[derive(Debug)]
pub struct RenderedSite {
    output: CapturedSource,
    search_page: Option<RoutePath>,
    media_types: BTreeMap<EntryPath, MediaType>,
    evidence: SiteRenderEvidence,
}

/// Private read-back evidence kept outside the render tree.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SiteRenderEvidence {
    /// Exact configured/pinned presentation input, including private source coordinates.
    pub theme: TemplateCompilation,
    /// Exact validated documentation discovery evidence, when installed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_catalog: Option<Sha256Digest>,
    /// All selected content identities, with their source and output digests.
    pub pages: BTreeMap<Key, SitePageEvidence>,
    /// Cross-site redirect outputs, distinct from this site's primary content.
    pub mirrors: BTreeMap<EntryPath, MirrorRenderEvidence>,
    /// Exact retained asset origins keyed by output path, outside the public model.
    pub assets: BTreeMap<EntryPath, Origin>,
    /// Selected provider input/tool/dependency evidence, outside the public model.
    pub capabilities: BTreeMap<CapabilityId, CapabilityEvidence>,
    /// Independently selected site resources and reference-reachable dependencies.
    pub resources: BTreeMap<ResourceId, super::resources::ResourceEvidence>,
    /// Ordered page-local compilation evidence, separate from the public resource facts.
    pub page_resources: BTreeMap<Key, Vec<PageResourceEvidence>>,
    /// Generated page-local interaction programs; distinct from captured module sources.
    pub islands: BTreeMap<Key, super::IslandEvidence>,
    /// Selected debug model artifact, not permission to publish its contents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_output: Option<EntryPath>,
    /// Explicit selected redirect entry; no inferred template or hidden fallback.
    pub alias_template: Option<EntryPath>,
    /// Absent when no graph view was requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphRenderEvidence>,
    /// Theme-owned missing-page output, separate from authored content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_found: Option<GraphRenderEvidence>,
    /// Exact generated social-card input and native capture evidence, absent for manual assets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub social_card: Option<super::SocialCardCaptureEvidence>,
    /// Derived navigation pages, separate from authored content and its provenance.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub taxonomy_pages: BTreeMap<EntryPath, super::TaxonomyRenderEvidence>,
    /// Continuations share their authored index identity; output evidence is path-keyed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub listing_pages: BTreeMap<EntryPath, super::ListingRenderEvidence>,
}

/// Exact page input and shared-model rendering evidence, without repeated Theme records.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SitePageEvidence {
    /// Actually rendered notebook attachments; not raw-source publication permission.
    pub notebooks: Vec<gazette_compiler::authored::NotebookEvidence>,
    /// Retained authored source origin, separate from canonical routes.
    pub source: gazette_compiler::authored::Origin,
    /// Selected template entry.
    pub template: EntryPath,
    /// Hash of the same model supplied to the renderer.
    pub model_sha256: Sha256Digest,
    /// Exact primary HTML bytes.
    pub html_sha256: Sha256Digest,
}

impl RenderedSite {
    /// Exact complete local output; no mutable access or successful partial tree.
    pub fn output(&self) -> &CapturedSource {
        &self.output
    }
    /// Selected Search destination that was emitted in this site's output.
    pub fn search_page(&self) -> Option<&RoutePath> {
        self.search_page.as_ref()
    }
    /// Producer-declared formats, including extensionless generated routes.
    /// Static assets without a format declaration are intentionally absent.
    pub fn media_types(&self) -> &BTreeMap<EntryPath, MediaType> {
        &self.media_types
    }
    /// Private input/output evidence, never included as a public page field.
    pub fn evidence(&self) -> &SiteRenderEvidence {
        &self.evidence
    }
}

mod error;
pub use error::SiteRenderError;

#[derive(Serialize)]
struct SiteModel<'a> {
    format: &'static str,
    site: &'a Key,
    pages: BTreeMap<Key, Value>,
    mirrors: BTreeMap<EntryPath, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    taxonomy_pages: BTreeMap<EntryPath, Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    listing_pages: BTreeMap<EntryPath, Value>,
    assets: BTreeMap<&'a str, &'a StaticAsset<'a>>,
    capabilities: &'a BTreeMap<CapabilityId, CapabilityProjection>,
    resources: &'a BTreeMap<ResourceId, super::resources::ResourceProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    browser_modules: Option<&'a super::resources::BrowserModules>,
    #[serde(skip_serializing_if = "Option::is_none")]
    graph: Option<Value>,
}

impl CompiledTheme {
    /// Assemble every object in the selected site from one fully resolved corpus.
    ///
    /// This is explicitly local and draft-inclusive, not a policy bypass or certified
    /// build. No native writes occur. Cross-site records remain exact graph references;
    /// other sites' primary pages are not rendered into this site's output.
    ///
    /// # Errors
    /// Rejects mismatched inputs, empty sites, missing routes/templates, unsupported
    /// opaque assets, any render failure, collisions and all configured output bounds.
    pub fn render_site(
        &self,
        authored: &ResolvedAuthored<'_, '_>,
        options: SiteRenderOptions<'_>,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<RenderedSite, SiteRenderError> {
        self.render_site_with_host_files(authored, options, raster, markdown, None)
    }

    /// Assemble a complete site with an explicitly supplied pure hosting provider.
    ///
    /// # Errors
    /// Preserves complete-site failures and rejects configured hosting without a provider.
    pub fn render_site_with_host_files(
        &self,
        authored: &ResolvedAuthored<'_, '_>,
        options: SiteRenderOptions<'_>,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        host_files: Option<&dyn gazette_ports::hosting::HostFiles>,
    ) -> Result<RenderedSite, SiteRenderError> {
        let capabilities = ResolvedCapabilities::default();
        let resources = CompiledResources::default();
        let page_resources = CompiledPageResources::default();
        self.render_site_with_resources(
            authored,
            options,
            SiteRenderContext {
                capabilities: &capabilities,
                resources: &resources,
                page_resources: &page_resources,
                raster,
                markdown,
                prepared: None,
                host_files,
                social_card: None,
                social_card_limits: Default::default(),
                documentation_catalog: None,
            },
        )
    }

    pub(super) fn render_site_with_resources(
        &self,
        authored: &ResolvedAuthored<'_, '_>,
        options: SiteRenderOptions<'_>,
        input: SiteRenderContext<'_, '_>,
    ) -> Result<RenderedSite, SiteRenderError> {
        let SiteRenderContext {
            capabilities,
            resources,
            page_resources,
            raster,
            markdown,
            mut prepared,
            host_files,
            social_card,
            social_card_limits,
            documentation_catalog,
        } = input;
        if self.evidence().configuration_sha256 != authored.content().compilation().source_sha256 {
            return Err(SiteRenderError::Configuration);
        }
        let discovery = authored.discovery().map_err(SiteRenderError::Discovery)?;
        let islands = prepared
            .as_mut()
            .map(|prepared| std::mem::take(&mut prepared.islands))
            .unwrap_or_default();
        // The compiler owns source scope, public placement and supported transport.
        // Unsupported companions remain explicit failures, never dropped or copied.
        let assets = authored
            .assets_for(&self.evidence().site)
            .map_err(SiteRenderError::Asset)?;
        let mut notebooks = BTreeSet::new();
        for node in authored
            .content()
            .graph()
            .nodes()
            .filter(|node| node.site == self.evidence().site)
        {
            for notebook in authored.notebooks(&node.id).map_err(|error| {
                SiteRenderError::Page(PageRenderError::Projection(
                    gazette_compiler::authored::ProjectionError::NotebookBinding(error),
                ))
            })? {
                notebooks.insert(notebook.path);
            }
        }
        if let Some(path) = assets.uncompiled().iter().find(|path| {
            !page_resources.consumed.contains(*path)
                && !resources.consumed.contains(*path)
                && !notebooks.contains(*path)
        }) {
            return Err(SiteRenderError::UncompiledAsset(path.clone()));
        }
        let selection = authored
            .project_site_pages(&self.evidence().site)
            .map_err(SiteRenderError::Pages)?;
        // Resolve the one publication-owned destination at the shared renderer boundary,
        // so SDK, commands, protocols, preview and preparation cannot disagree.
        let configured_graph = selection.publication().machine.graph.as_ref();
        let graph = match configured_graph {
            Some(configured) => {
                if !configured.enabled {
                    if options.graph.is_some() {
                        return Err(SiteRenderError::GraphSelection(
                            "graph output was explicitly disabled by publication configuration",
                        ));
                    }
                    None
                } else {
                    if options
                        .graph
                        .as_ref()
                        .is_some_and(|requested| configured.path.output() != requested.output)
                    {
                        return Err(SiteRenderError::GraphSelection(
                            "invocation graph output differs from its configured publication destination",
                        ));
                    }
                    Some(GraphRenderOptions {
                        output: configured.path.output(),
                        limits: options
                            .graph
                            .as_ref()
                            .map_or_else(Default::default, |requested| requested.limits),
                    })
                }
            }
            None => options.graph.clone(),
        };
        let taxonomy_pages = authored
            .project_taxonomy_pages(&self.evidence().site, &discovery)
            .map_err(SiteRenderError::Taxonomy)?;
        let listing_pages = authored
            .project_listing_pages(&self.evidence().site, &discovery)
            .map_err(SiteRenderError::Listing)?;
        let maximum_alternates =
            usize::try_from(options.limits.file_bytes.min(options.limits.total_bytes))
                .unwrap_or(usize::MAX);
        let mut alternates: BTreeMap<_, _> = selection
            .markdown_alternates(&taxonomy_pages, &listing_pages, maximum_alternates)
            .map_err(SiteRenderError::Pages)?
            .into_iter()
            .map(|alternate| (alternate.html_url.clone(), alternate))
            .collect();
        let discovery_pages = selection
            .discoverable(&discovery)
            .map_err(SiteRenderError::Pages)?
            .with_taxonomy_pages(&taxonomy_pages)
            .map_err(SiteRenderError::Pages)?
            .with_listing_pages(&listing_pages)
            .map_err(SiteRenderError::Pages)?;
        let recipes = self.render_recipes();
        for page in selection.pages() {
            if !page.route.aliases.is_empty() && recipes.alias.is_none() {
                return Err(SiteRenderError::AliasTemplateRequired(page.node.id.clone()));
            }
        }
        if let Some(mirror) = selection.mirrors().first()
            && recipes.alias.is_none()
        {
            return Err(SiteRenderError::AliasTemplateRequired(
                mirror.node.id.clone(),
            ));
        }
        let external_redirects = &selection.publication().external_redirects;
        if !external_redirects.is_empty() && recipes.alias.is_none() {
            return Err(SiteRenderError::AliasTemplateRequired(
                self.evidence().site.clone(),
            ));
        }
        let mut files = BTreeMap::new();
        let mut media_types = BTreeMap::new();
        let mut builder = InventoryBuilder::new(options.limits);
        let mut directories = BTreeSet::new();
        let mut evidence = BTreeMap::new();
        let mut mirror_evidence = BTreeMap::new();
        let mut asset_evidence = BTreeMap::new();
        let mut taxonomy_evidence = BTreeMap::new();
        let mut listing_evidence = BTreeMap::new();
        let mut listing_contexts = BTreeMap::new();
        let mut model = SiteModel {
            format: "gazette-local-site-model/1",
            site: &self.evidence().site,
            pages: BTreeMap::new(),
            mirrors: BTreeMap::new(),
            taxonomy_pages: BTreeMap::new(),
            listing_pages: BTreeMap::new(),
            capabilities: &capabilities.model,
            resources: &resources.model,
            browser_modules: (!resources.browser.is_empty()).then_some(&resources.browser),
            graph: None,
            assets: assets
                .entries()
                .filter(|(_, asset)| asset.provenance.discoverable)
                .map(|(_, asset)| (asset.url.as_str(), asset))
                .collect(),
        };
        for (path, bytes) in resources
            .outputs
            .iter()
            .chain(&page_resources.outputs)
            .chain(&islands.outputs)
        {
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                path.clone(),
                bytes.to_vec(),
            )?;
        }
        for resource in resources
            .model
            .values()
            .chain(page_resources.model.values().flatten())
            .chain(islands.model.values())
        {
            media_types.insert(
                resource.output.clone(),
                match resource.kind {
                    resource::Kind::BrowserModule => MediaType::JavaScript,
                    resource::Kind::Stylesheet => MediaType::Css,
                    resource::Kind::PngImage => MediaType::Png,
                    resource::Kind::JpegImage => MediaType::Jpeg,
                    resource::Kind::WebpImage => MediaType::Webp,
                    resource::Kind::SvgImage => MediaType::Svg,
                    resource::Kind::Woff2Font => MediaType::Woff2,
                },
            );
        }
        // Emit each explicitly scoped static asset once, including unreferenced
        // entries. Reuse the same output inventory as pages, aliases and JSON.
        for (_, asset) in assets.entries() {
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                asset.output.clone(),
                asset.content().to_vec(),
            )?;
            asset_evidence.insert(asset.output.clone(), asset.source().clone());
        }
        let maximum_model =
            usize::try_from(options.limits.file_bytes.min(options.limits.total_bytes))
                .unwrap_or(usize::MAX);
        let mut model_bytes = 0usize;
        let mut search_bytes = 0usize;
        let mut search_documents = Vec::new();
        // Feed discovery must describe completed wire outputs before any Theme renders.
        // Retain each projection once, then consume the same body for its HTML surface.
        let mut projections = BTreeMap::new();
        let mut feed_discovery = Vec::new();
        if selection.publication().machine.feeds.is_some() {
            for selected in selection.pages() {
                let projection = match prepared.as_mut() {
                    Some(prepared) => prepared
                        .pages
                        .remove(&selected.node.id)
                        .ok_or(SiteRenderError::Configuration)?,
                    None => authored
                        .project_page_with_renditions(
                            &selected.node.id,
                            raster,
                            markdown,
                            Some(&discovery),
                            Some(gazette_compiler::markdown::ComponentPresentation {
                                definitions: &self.components,
                                renderer: self,
                            }),
                            &page_resources.dimensions(&selected.node.id),
                        )
                        .map_err(|error| {
                            SiteRenderError::Page(PageRenderError::Projection(error))
                        })?,
                };
                projections.insert(selected.node.id.clone(), projection);
            }
        }
        let mut emitted_feeds = BTreeSet::new();
        if let Some(feeds) = &selection.publication().machine.feeds {
            for configuration in &feeds.channels {
                let feed = authored
                    .project_discovery_feed(
                        &self.evidence().site,
                        configuration,
                        &projections,
                        &discovery,
                    )
                    .map_err(SiteRenderError::Feed)?;
                let maximum = usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX);
                let Some(bytes) = feed.render(maximum).map_err(SiteRenderError::Feed)? else {
                    continue;
                };
                feed_discovery.push(super::presentation::FeedDiscovery {
                    id: configuration.id.clone(),
                    channel: configuration.channel.clone(),
                    title: feed.title().to_owned(),
                    url: feed.url().to_owned(),
                    format: configuration.format,
                    media_type: match configuration.format {
                        FeedFormat::Atom => "application/atom+xml",
                        FeedFormat::Rss => "application/rss+xml",
                        FeedFormat::Json => "application/feed+json",
                    },
                });
                emitted_feeds.insert(configuration.path.output());
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    configuration.path.output().clone(),
                    bytes,
                )?;
                media_types.insert(
                    configuration.path.output().clone(),
                    match configuration.format {
                        FeedFormat::Atom => MediaType::Atom,
                        FeedFormat::Rss => MediaType::Rss,
                        FeedFormat::Json => MediaType::Json,
                    },
                );
            }
        }

        if let Some(hosting) = selection
            .publication()
            .processing
            .as_ref()
            .and_then(|processing| processing.hosting.as_ref())
        {
            let provider = host_files.ok_or_else(|| {
                super::deployment::invalid("configured hosting requires a HostFiles provider")
            })?;
            let production = selection
                .publication()
                .public_base_url
                .as_ref()
                .ok_or_else(|| {
                    super::deployment::invalid(
                        "configured hosting requires its production public base URL",
                    )
                })?;
            let tree = provider
                .generate(hosting.provider, production, builder.remaining_limits())
                .map_err(SiteRenderError::Output)?;
            for (path, bytes) in tree.into_parts().1.into_files() {
                media_types.insert(path.clone(), MediaType::Text);
                insert(&mut builder, &mut directories, &mut files, path, bytes)?;
            }
        }

        // Emit machine entrypoints before rendering so Themes consume actual outputs.
        // The shared inventory still enforces collisions and whole-assembly limits.
        let mut output_discovery = Vec::new();
        let discovery_record = |kind, path: &EntryPath, media_type| {
            let invalid = |error| SiteRenderError::Render(render::Error::Context(Box::new(error)));
            let route = "/"
                .parse::<gazette_core::publication::routing::RoutePath>()
                .map_err(invalid)?
                .join_file(path)
                .map_err(invalid)?;
            let base = authored
                .content()
                .base_url(&self.evidence().site)
                .ok_or(SiteRenderError::Configuration)?;
            Ok::<_, SiteRenderError>(super::presentation::OutputDiscovery {
                kind,
                url: base.resolve(&route),
                media_type,
            })
        };
        if selection.publication().machine.sitemap {
            let tree = discovery_pages
                .sitemaps(builder.remaining_limits())
                .map_err(SiteRenderError::Pages)?;
            for (path, bytes) in tree.into_parts().1.into_files() {
                media_types.insert(path.clone(), MediaType::Xml);
                insert(&mut builder, &mut directories, &mut files, path, bytes)?;
            }
            // The compiler uses this entrypoint for both a single map and an index.
            let root: EntryPath = "sitemap.xml".parse().map_err(SiteRenderError::Output)?;
            if !files.contains_key(&root) {
                return Err(SiteRenderError::Configuration);
            }
            output_discovery.push(discovery_record(
                super::presentation::DiscoveryKind::Sitemap,
                &root,
                "application/xml",
            )?);
        }
        if selection
            .publication()
            .machine
            .llms
            .as_ref()
            .is_some_and(|configuration| configuration.enabled)
        {
            let maximum = usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX);
            let llms_configuration = selection
                .publication()
                .machine
                .llms
                .as_ref()
                .ok_or(SiteRenderError::Configuration)?;
            let bytes = if let Some(template) = recipes.llms.as_ref() {
                let mut llms = selection
                    .llms_model(
                        &discovery,
                        &taxonomy_pages,
                        &listing_pages,
                        self.evidence().limits.context_bytes,
                    )
                    .map_err(SiteRenderError::Pages)?;
                super::documentation_reading::extend_llms(
                    &mut llms,
                    documentation_catalog,
                    authored,
                    self.evidence().limits.context_bytes,
                )
                .map_err(SiteRenderError::Render)?;
                let (value, _) = context(
                    serde_json::json!({"llms": llms}),
                    self.evidence().limits.context_bytes,
                    "llms model bytes",
                )
                .map_err(SiteRenderError::Render)?;
                self.render(template, &value)
                    .map_err(SiteRenderError::Render)?
            } else if llms_configuration.coverage
                != gazette_compiler::workspace::LlmsCoverage::Curated
            {
                let mut llms = selection
                    .llms_model(
                        &discovery,
                        &taxonomy_pages,
                        &listing_pages,
                        self.evidence().limits.context_bytes,
                    )
                    .map_err(SiteRenderError::Pages)?;
                super::documentation_reading::extend_llms(
                    &mut llms,
                    documentation_catalog,
                    authored,
                    self.evidence().limits.context_bytes,
                )
                .map_err(SiteRenderError::Render)?;
                llms.markdown(maximum).map_err(SiteRenderError::Pages)?
            } else {
                discovery_pages
                    .llms(&discovery, maximum)
                    .map_err(SiteRenderError::Pages)?
            };
            std::str::from_utf8(&bytes).map_err(|error| {
                SiteRenderError::Render(render::Error::Context(Box::new(error)))
            })?;
            let path: EntryPath = "llms.txt".parse().map_err(SiteRenderError::Output)?;
            media_types.insert(path.clone(), MediaType::Text);
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                path.clone(),
                bytes,
            )?;
            output_discovery.push(discovery_record(
                super::presentation::DiscoveryKind::Llms,
                &path,
                "text/plain",
            )?);
        }

        let cache = selection.publication().machine.page_cache;
        if cache.is_some_and(|policy| !policy.retire) {
            let path = page_cache::path()?;
            output_discovery.push(discovery_record(
                super::presentation::DiscoveryKind::PageCache,
                &path,
                "application/json",
            )?);
        }

        let chrome = authored
            .site_chrome_with_composition(&self.evidence().site, &self.composition)
            .map_err(|error| SiteRenderError::Page(PageRenderError::Projection(error)))?;
        let search_control = chrome
            .as_ref()
            .and_then(|chrome| chrome.shell.search.as_ref());
        let search_binding =
            super::search::binding(authored.content(), &self.evidence().site, search_control)
                .map_err(SiteRenderError::Render)?;
        let identity_assets = super::presentation::IdentityAssets::resolve(
            &selection.publication().identity_assets,
            &resources.model,
        )
        .map_err(|(role, resource)| SiteRenderError::IdentityAsset { role, resource })?;
        let base = authored
            .content()
            .base_url(&self.evidence().site)
            .ok_or(SiteRenderError::Configuration)?;
        let mut publication = authored
            .publication_metadata(&self.evidence().site)
            .map_err(|_| {
                super::deployment::invalid("deployment outputs require valid publication metadata")
            })?;
        if let Some(metadata) = publication.as_mut() {
            metadata.render_year = options.render_year;
        }
        super::deployment::machine_outputs(
            selection.publication(),
            publication.as_ref(),
            base,
            identity_assets,
            &mut output_discovery,
            &mut super::deployment::Output {
                builder: &mut builder,
                directories: &mut directories,
                files: &mut files,
                media_types: &mut media_types,
            },
        )?;
        let graph_discovery = graph
            .as_ref()
            .map(|graph| {
                let invalid =
                    |error| SiteRenderError::Render(render::Error::Context(Box::new(error)));
                let route = match configured_graph {
                    Some(configuration) => configuration.path.clone(),
                    None => "/"
                        .parse::<gazette_core::publication::routing::RoutePath>()
                        .map_err(invalid)?
                        .join_file(graph.output)
                        .map_err(invalid)?,
                };
                let base = authored
                    .content()
                    .base_url(&self.evidence().site)
                    .ok_or(SiteRenderError::Configuration)?;
                Ok::<_, SiteRenderError>(super::presentation::GraphDiscovery {
                    url: base.resolve(&route),
                })
            })
            .transpose()?;
        let not_found_evidence = if let Some(template) = recipes.not_found.as_ref() {
            let mut presentation = super::presentation::Presentation::site(
                self,
                &capabilities.model,
                resources,
                super::presentation::SurfaceKind::NotFound,
                chrome.as_ref(),
                search_binding.as_ref(),
                identity_assets,
            );
            presentation.feeds = &feed_discovery;
            presentation.discovery = &output_discovery;
            presentation.publication = publication;
            presentation.graph = graph_discovery.as_ref();
            let search_url = selection
                .publication()
                .search_page
                .as_ref()
                .filter(|id| selection.pages().any(|page| &page.node.id == *id))
                .and_then(|id| authored.content().route(id))
                .map(|route| route.url.as_str());
            let (bytes, evidence) = super::deployment::not_found(
                self,
                template,
                &presentation,
                base,
                search_url,
                maximum_model,
            )?;
            media_types.insert(evidence.output.clone(), MediaType::Html);
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                evidence.output.clone(),
                bytes,
            )?;
            Some(evidence)
        } else {
            None
        };
        let graph_evidence = if let Some(graph) = &graph {
            let template = recipes
                .graph
                .as_ref()
                .ok_or(SiteRenderError::GraphTemplateRequired)?;
            let projection = authored
                .project_discovery_graph(&self.evidence().site, graph.limits, &discovery)
                .map_err(SiteRenderError::Graph)?;
            let mut presentation = super::presentation::Presentation::site(
                self,
                &capabilities.model,
                resources,
                super::presentation::SurfaceKind::Graph,
                chrome.as_ref(),
                search_binding.as_ref(),
                identity_assets,
            );
            presentation.feeds = &feed_discovery;
            presentation.discovery = &output_discovery;
            presentation.graph = graph_discovery.as_ref();
            presentation.publication = authored
                .publication_metadata(&self.evidence().site)
                .map_err(|error| {
                    SiteRenderError::Page(PageRenderError::Projection(
                        gazette_compiler::authored::ProjectionError::Binding {
                            id: error.id,
                            reason: error.reason,
                        },
                    ))
                })?;
            if let Some(metadata) = presentation.publication.as_mut() {
                metadata.render_year = options.render_year;
            }
            // One typed producer, one bounded Value: neither Theme nor JSON
            // reconstructs relationships or queries mutable source state.
            let value: Value = serde_json::from_slice(
                &encode(
                    &super::presentation::Presented {
                        value: &projection,
                        presentation: &presentation,
                    },
                    maximum_model.min(self.evidence().limits.context_bytes),
                    "graph model bytes",
                )
                .map_err(SiteRenderError::Render)?,
            )
            .map_err(|error| SiteRenderError::Render(render::Error::Context(Box::new(error))))?;
            let (value, encoded) = context(
                value,
                maximum_model.min(self.evidence().limits.context_bytes),
                "graph model bytes",
            )
            .map_err(SiteRenderError::Render)?;
            model_bytes += encoded.len();
            let bytes = self
                .render(template, &value)
                .map_err(SiteRenderError::Render)?;
            std::str::from_utf8(&bytes).map_err(SiteRenderError::GraphUtf8)?;
            let evidence = GraphRenderEvidence {
                template: template.clone(),
                output: graph.output.clone(),
                model_sha256: Sha256Digest::of(&encoded),
                output_sha256: Sha256Digest::of(&bytes),
            };
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                graph.output.clone(),
                bytes,
            )?;
            model.graph = Some(value);
            media_types.insert(graph.output.clone(), MediaType::Html);
            Some(evidence)
        } else {
            None
        };
        let (card_path, card_mime, card_media_type) =
            match social_card.map(|capture| capture.evidence.image.format) {
                Some(gazette_core::raster::Format::Jpeg) => {
                    ("assets/social-card.jpg", "image/jpeg", MediaType::Jpeg)
                }
                _ => ("assets/social-card.png", "image/png", MediaType::Png),
            };
        let generated_card = social_card.map(|capture| super::SocialCardProjection {
            url: base.resolve_output(
                &format!("/{card_path}")
                    .parse::<RoutePath>()
                    .expect("fixed card route"),
            ),
            width: capture.evidence.image.width,
            height: capture.evidence.image.height,
            media_type: card_mime,
            title: capture.evidence.title.clone(),
            sha256: Sha256Digest::of(&capture.bytes),
        });
        let mut used_card = false;
        for selected in selection.pages() {
            let node = selected.node;
            let is_card_home = selection.publication().home.as_ref() == Some(&node.id)
                && identity_assets.is_none_or(|assets| assets.link_preview.is_none())
                && recipes.social_card.is_some();
            let route = selected.route;
            let page = self
                .render_page_with_resources(
                    authored,
                    &node.id,
                    authored.selected_template(&node.id).unwrap_or_else(|| {
                        recipes.page_in_publication(node, selection.publication())
                    }),
                    super::page::PageRenderContext {
                        documentation_catalog,
                        render_year: options.render_year,
                        graph: graph_discovery.as_ref(),
                        feeds: &feed_discovery,
                        discovery: &output_discovery,
                        alternate: alternates.get(&route.url),
                        listing_pages: &listing_pages,
                        capabilities: &capabilities.model,
                        theme_resources: &resources.theme,
                        resources: page_resources
                            .model
                            .get(&node.id)
                            .map_or(&[], Vec::as_slice),
                        browser_modules: Some(&resources.browser),
                        projection: super::page::ProjectionInputs {
                            social_card: None,
                            social_card_limits: Default::default(),
                            raster,
                            markdown,
                            discovery: Some(&discovery),
                        },
                        prepared: if selection.publication().machine.feeds.is_some() {
                            Some(
                                projections
                                    .remove(&node.id)
                                    .ok_or(SiteRenderError::Configuration)?,
                            )
                        } else {
                            prepared
                                .as_mut()
                                .map(|prepared| {
                                    prepared
                                        .pages
                                        .remove(&node.id)
                                        .ok_or(SiteRenderError::Configuration)
                                })
                                .transpose()?
                        },
                        interaction: islands.model.get(&node.id),
                        chrome: chrome.as_ref(),
                        identity_assets,
                        social_card: generated_card.as_ref(),
                    },
                )
                .map_err(SiteRenderError::Page)?;
            let super::page::RenderedPage {
                evidence: page_evidence,
                model: mut page_model,
                html,
                search,
                projection,
                listing,
            } = page;
            if is_card_home {
                let recipe = recipes
                    .social_card
                    .as_ref()
                    .ok_or(SiteRenderError::Configuration)?;
                let renderer = authored
                    .content()
                    .compilation()
                    .workspace
                    .social_card_renderer()
                    .ok_or_else(|| {
                        SiteRenderError::SocialCard(Box::new(
                            super::SocialCardGenerationError::Unconfigured,
                        ))
                    })?;
                let mut capture_assets = BTreeMap::new();
                for resource in resources
                    .model
                    .values()
                    .chain(page_resources.model.values().flatten())
                    .chain(islands.model.values())
                {
                    let (media_type, role) = match resource.kind {
                        resource::Kind::BrowserModule => ("text/javascript", "module"),
                        resource::Kind::Stylesheet => ("text/css", "stylesheet"),
                        resource::Kind::PngImage => ("image/png", "media"),
                        resource::Kind::JpegImage => ("image/jpeg", "media"),
                        resource::Kind::WebpImage => ("image/webp", "media"),
                        resource::Kind::SvgImage => ("image/svg+xml", "media"),
                        resource::Kind::Woff2Font => ("font/woff2", "font"),
                    };
                    let bytes = files
                        .get(&resource.output)
                        .ok_or(SiteRenderError::Configuration)?;
                    capture_assets.insert(
                        resource.url.as_str(),
                        super::social_cards::AssetInput {
                            url: &resource.url,
                            bytes,
                            media_type,
                            role,
                        },
                    );
                }
                for (_, asset) in assets.entries() {
                    if let Some((media_type, role)) =
                        super::social_cards::static_media(&asset.output)
                    {
                        capture_assets.insert(
                            asset.url.as_str(),
                            super::social_cards::AssetInput {
                                url: &asset.url,
                                bytes: asset.content(),
                                media_type,
                                role,
                            },
                        );
                    }
                }
                let request = super::social_cards::prepare(
                    self,
                    &self.evidence().site,
                    &mut page_model,
                    recipe,
                    renderer,
                    &route.url,
                    &node.title,
                    &capture_assets.into_values().collect::<Vec<_>>(),
                    social_card_limits,
                    social_card,
                    resources,
                )
                .map_err(|error| SiteRenderError::SocialCard(Box::new(error)))?;
                if let Some(request) = request {
                    return Err(SiteRenderError::SocialCardCaptureRequired(Box::new(
                        request,
                    )));
                }
                let capture = social_card.ok_or(SiteRenderError::Configuration)?;
                let output: EntryPath = card_path.parse().map_err(SiteRenderError::Output)?;
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    output.clone(),
                    capture.bytes.clone(),
                )?;
                media_types.insert(output, card_media_type);
                used_card = true;
            }
            if listing_pages
                .iter()
                .any(|listing| listing.content == node.id)
            {
                listing_contexts.insert(node.id.clone(), page_model.clone());
            }
            if let Some(alternate) = alternates.remove(&route.url) {
                let bytes = projection
                    .markdown_alternate(&alternate, &discovery, maximum_alternates)
                    .map_err(SiteRenderError::Pages)?;
                media_types.insert(alternate.output.clone(), MediaType::Markdown);
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    alternate.output,
                    bytes,
                )?;
            }
            if let Some(document) = search {
                search_bytes += encode(
                    &document,
                    maximum_model.saturating_sub(search_bytes),
                    "search documents bytes",
                )
                .map_err(SiteRenderError::Render)?
                .len();
                search_documents.push(document);
            }
            if let Some(listing) = listing {
                model_bytes += encode(
                    &listing,
                    maximum_model.saturating_sub(model_bytes),
                    "site model bytes",
                )
                .map_err(SiteRenderError::Render)?
                .len();
                model.pages.insert(node.id.clone(), listing);
            }
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                route.output.clone(),
                html.into_bytes(),
            )?;
            media_types.insert(route.output.clone(), MediaType::Html);
            if let Some(template) = recipes.alias.as_ref() {
                for alias in &route.aliases {
                    let mut alias_model = page_model.clone();
                    if let Some(fields) = alias_model.as_object_mut() {
                        fields.remove("invitations");
                    }
                    let Some(kind) = alias_model
                        .get_mut("presentation")
                        .and_then(|presentation| presentation.get_mut("surface"))
                        .and_then(|surface| surface.get_mut("kind"))
                    else {
                        return Err(SiteRenderError::Configuration);
                    };
                    *kind = serde_json::to_value(super::presentation::SurfaceKind::Alias).map_err(
                        |error| SiteRenderError::Render(render::Error::Context(Box::new(error))),
                    )?;
                    if let Some(presentation) = alias_model
                        .get_mut("presentation")
                        .and_then(Value::as_object_mut)
                    {
                        presentation.remove("markdown_alternate");
                        presentation.remove("webmcp");
                        let destination = authored
                            .redirect_destination(&node.id, &self.evidence().site)
                            .ok_or(SiteRenderError::Configuration)?;
                        presentation.insert(
                            "redirect_destination".into(),
                            serde_json::to_value(destination).map_err(|error| {
                                SiteRenderError::Render(render::Error::Context(Box::new(error)))
                            })?,
                        );
                    }
                    let html = self
                        .render(template, &alias_model)
                        .map_err(SiteRenderError::Render)?;
                    let html = String::from_utf8(html)
                        .map_err(|error| SiteRenderError::Page(PageRenderError::Utf8(error)))?;
                    insert(
                        &mut builder,
                        &mut directories,
                        &mut files,
                        alias.output.clone(),
                        html.into_bytes(),
                    )?;
                    media_types.insert(alias.output.clone(), MediaType::Html);
                }
            }
            evidence.insert(
                node.id.clone(),
                SitePageEvidence {
                    notebooks: page_evidence.notebooks,
                    source: page_evidence.source,
                    template: page_evidence.template,
                    model_sha256: page_evidence.model_sha256,
                    html_sha256: page_evidence.html_sha256,
                },
            );
        }
        for page in listing_pages.iter().filter(|page| !page.is_primary()) {
            let owner = listing_contexts
                .get(&page.content)
                .ok_or(SiteRenderError::Configuration)?;
            let template = authored
                .selected_template(&page.content)
                .unwrap_or_else(|| {
                    authored
                        .content()
                        .graph()
                        .node(&page.content)
                        .map_or(&recipes.page, |node| {
                            recipes.page_in_publication(node, selection.publication())
                        })
                });
            let rendered = super::listings::render(
                self,
                page,
                owner,
                template,
                maximum_model.saturating_sub(model_bytes),
                alternates.get(&page.url),
            )?;
            if let Some(alternate) = alternates.remove(&page.url) {
                let bytes = page
                    .markdown_alternate(&alternate, maximum_alternates)
                    .map_err(SiteRenderError::Pages)?;
                media_types.insert(alternate.output.clone(), MediaType::Markdown);
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    alternate.output,
                    bytes,
                )?;
            }
            let output = rendered.evidence.output.clone();
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                output.clone(),
                rendered.html,
            )?;
            media_types.insert(output.clone(), MediaType::Html);
            if discovery.contains(&page.content) {
                model_bytes += encode(
                    &rendered.model,
                    maximum_model.saturating_sub(model_bytes),
                    "site model bytes",
                )
                .map_err(SiteRenderError::Render)?
                .len();
                model.listing_pages.insert(output.clone(), rendered.model);
            }
            listing_evidence.insert(output, rendered.evidence);
        }
        for page in &taxonomy_pages {
            let mut taxonomy_presentation = super::presentation::Presentation::site(
                self,
                &capabilities.model,
                resources,
                super::presentation::SurfaceKind::Taxonomy,
                chrome.as_ref(),
                search_binding.as_ref(),
                identity_assets,
            );
            taxonomy_presentation.feeds = &feed_discovery;
            taxonomy_presentation.discovery = &output_discovery;
            taxonomy_presentation.graph = graph_discovery.as_ref();
            taxonomy_presentation.markdown_alternate = alternates.get(&page.model.canonical_url);
            let rendered = super::taxonomies::render(
                self,
                page,
                &taxonomy_presentation,
                selection.publication(),
                maximum_model.saturating_sub(model_bytes),
            )?;
            if let Some(alternate) = alternates.remove(&page.model.canonical_url) {
                let bytes = page
                    .model
                    .markdown_alternate(&alternate, maximum_alternates)
                    .map_err(SiteRenderError::Pages)?;
                media_types.insert(alternate.output.clone(), MediaType::Markdown);
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    alternate.output,
                    bytes,
                )?;
            }
            model_bytes += encode(
                &rendered.model,
                maximum_model.saturating_sub(model_bytes),
                "site model bytes",
            )
            .map_err(SiteRenderError::Render)?
            .len();
            let output = rendered.evidence.output.clone();
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                output.clone(),
                rendered.html,
            )?;
            media_types.insert(output.clone(), MediaType::Html);
            model.taxonomy_pages.insert(output.clone(), rendered.model);
            taxonomy_evidence.insert(output, rendered.evidence);
        }
        if let Some(template) = recipes.alias.as_ref() {
            let mut mirror_presentation = super::presentation::Presentation::site(
                self,
                &capabilities.model,
                resources,
                super::presentation::SurfaceKind::Mirror,
                chrome.as_ref(),
                search_binding.as_ref(),
                identity_assets,
            );
            mirror_presentation.feeds = &feed_discovery;
            mirror_presentation.discovery = &output_discovery;
            mirror_presentation.graph = graph_discovery.as_ref();
            for (route, target) in external_redirects {
                mirror_presentation.redirect_destination =
                    Some(gazette_compiler::markdown::LinkDestination {
                        kind: gazette_core::markdown::LinkDestinationKind::External,
                        content: None,
                        site: None,
                        site_name: None,
                        relationships: Vec::new(),
                        presentation: None,
                    });
                let value = serde_json::json!({
                    "route": { "canonical_url": target.as_str() }
                });
                let context = super::presentation::Presented {
                    value: &value,
                    presentation: &mirror_presentation,
                };
                let maximum = maximum_model.min(self.evidence().limits.context_bytes);
                let (context, _) = super::serialization::context(
                    serde_json::to_value(context).map_err(|error| {
                        SiteRenderError::Render(render::Error::Context(Box::new(error)))
                    })?,
                    maximum,
                    "external redirect model bytes",
                )
                .map_err(SiteRenderError::Render)?;
                let html = self
                    .render(template, &context)
                    .map_err(SiteRenderError::Render)?;
                let output = route.output().clone();
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    output.clone(),
                    html,
                )?;
                media_types.insert(output, MediaType::Html);
            }
            for selected in selection.mirrors() {
                mirror_presentation.redirect_destination = Some(
                    authored
                        .redirect_destination(&selected.node.id, &selected.mirror.site)
                        .ok_or(SiteRenderError::Configuration)?,
                );
                let (html, mirror_model, mirror) = super::mirrors::render(
                    self,
                    selected,
                    template,
                    maximum_model
                        .saturating_sub(model_bytes)
                        .min(self.evidence().limits.context_bytes),
                    &mirror_presentation,
                )?;
                model_bytes += encode(
                    &mirror.model,
                    maximum_model.saturating_sub(model_bytes),
                    "site model bytes",
                )
                .map_err(SiteRenderError::Render)?
                .len();
                let output = selected.mirror.output.clone();
                insert(
                    &mut builder,
                    &mut directories,
                    &mut files,
                    output.clone(),
                    html,
                )?;
                media_types.insert(output.clone(), MediaType::Html);
                model.mirrors.insert(output.clone(), mirror_model);
                mirror_evidence.insert(output, mirror);
            }
        }
        if !alternates.is_empty() {
            return Err(SiteRenderError::Configuration);
        }
        if let Some(search) = search_binding {
            let search_configuration = selection
                .publication()
                .machine
                .search
                .as_ref()
                .ok_or(SiteRenderError::Configuration)?;
            search_documents.extend(
                super::documentation_reading::search(
                    documentation_catalog,
                    authored,
                    search_configuration,
                    maximum_model.saturating_sub(search_bytes),
                )
                .map_err(SiteRenderError::Render)?,
            );
            let mut reading_urls = BTreeSet::new();
            for record in &search_documents {
                let url = record
                    .get("route")
                    .and_then(|route| route.get("url"))
                    .and_then(serde_json::Value::as_str)
                    .ok_or(SiteRenderError::Configuration)?;
                if !reading_urls.insert(url) {
                    return Err(SiteRenderError::Configuration);
                }
            }
            let bytes = encode(
                &search_documents,
                usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX),
                "search documents bytes",
            )
            .map_err(SiteRenderError::Render)?;
            let path = search.output;
            media_types.insert(path.clone(), MediaType::Json);
            insert(&mut builder, &mut directories, &mut files, path, bytes)?;
        }
        if selection.publication().machine.json_ld {
            let mut graph = discovery_pages
                .pages()
                .map(|page| {
                    model
                        .pages
                        .get(&page.node.id)
                        .and_then(|value| value.get("json_ld"))
                        .ok_or_else(|| SiteRenderError::JsonLdRequired(page.node.id.clone()))
                })
                .collect::<Result<Vec<_>, _>>()?;
            for page in model.taxonomy_pages.values() {
                graph.push(page.get("json_ld").ok_or(SiteRenderError::Configuration)?);
            }
            for page in model.listing_pages.values() {
                graph.push(page.get("json_ld").ok_or(SiteRenderError::Configuration)?);
            }
            let bytes = encode(
                &SiteJsonLd::from_documents(graph),
                usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX),
                "site JSON-LD bytes",
            )
            .map_err(SiteRenderError::Render)?;
            let path: EntryPath = "site.jsonld".parse().map_err(SiteRenderError::Output)?;
            media_types.insert(path.clone(), MediaType::JsonLd);
            insert(&mut builder, &mut directories, &mut files, path, bytes)?;
        }
        if let Some(path) = options.model_output {
            let json = encode(
                &model,
                maximum_model.min(usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX)),
                "site model bytes",
            )
            .map_err(SiteRenderError::Render)?;
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                path.clone(),
                json,
            )?;
            media_types.insert(path.clone(), MediaType::Json);
        }
        for resource in resources
            .model
            .values()
            .chain(page_resources.model.values().flatten())
            .chain(islands.model.values())
        {
            for target in resource.references.values() {
                if !files.contains_key(target) {
                    return Err(SiteRenderError::Output(source::Error::InvalidInventory {
                        path: target.as_str().into(),
                        reason: "compiled resource references a missing output file",
                    }));
                }
            }
        }
        if let Some(feeds) = &selection.publication().machine.feeds {
            // Declared feed paths remain reserved even when filtering omits bytes.
            // Reuse native inventory/path bounds before yielding any materialized tree;
            // these zero-byte reservation markers are private validation, never output.
            let mut reservations = InventoryBuilder::new(options.limits);
            let mut parents = BTreeSet::new();
            let marker = Content::File {
                bytes: 0,
                sha256: Sha256Digest::of(&[]),
            };
            for path in feeds
                .channels
                .iter()
                .map(|feed| feed.path.output())
                .chain(files.keys().filter(|path| !emitted_feeds.contains(*path)))
            {
                inventory_file(&mut reservations, &mut parents, path.clone(), marker)?;
            }
        }
        if let Some(policy) = cache {
            let path = page_cache::path()?;
            let bytes = page_cache::descriptor(
                &selection,
                &discovery,
                &files,
                policy,
                usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX),
            )?;
            insert(
                &mut builder,
                &mut directories,
                &mut files,
                path.clone(),
                bytes,
            )?;
            media_types.insert(path, MediaType::Json);
        }
        let output =
            CapturedSource::from_parts(builder.finish(), files).map_err(SiteRenderError::Output)?;
        let search_page = selection
            .publication()
            .search_page
            .as_ref()
            .and_then(|search| {
                let page = selection.pages().find(|page| &page.node.id == search)?;
                let path = &page.node.route.as_ref()?.path;
                (media_types.get(path.output()) == Some(&MediaType::Html)).then(|| path.clone())
            });
        Ok(RenderedSite {
            output,
            search_page,
            media_types,
            evidence: SiteRenderEvidence {
                theme: self.evidence().clone(),
                documentation_catalog: documentation_catalog
                    .map(super::DocumentationCatalog::evidence_sha256),
                pages: evidence,
                mirrors: mirror_evidence,
                assets: asset_evidence,
                capabilities: capabilities.evidence.clone(),
                resources: resources.evidence.clone(),
                page_resources: page_resources.evidence.clone(),
                islands: islands.evidence,
                model_output: options.model_output.cloned(),
                alias_template: recipes.alias.clone(),
                graph: graph_evidence,
                not_found: not_found_evidence,
                social_card: social_card
                    .filter(|_| used_card)
                    .map(|capture| capture.evidence.clone()),
                taxonomy_pages: taxonomy_evidence,
                listing_pages: listing_evidence,
            },
        })
    }
}

pub(super) fn insert(
    builder: &mut InventoryBuilder,
    directories: &mut BTreeSet<EntryPath>,
    files: &mut BTreeMap<EntryPath, Vec<u8>>,
    path: EntryPath,
    bytes: Vec<u8>,
) -> Result<(), SiteRenderError> {
    inventory_file(
        builder,
        directories,
        path.clone(),
        Content::File {
            bytes: bytes.len() as u64,
            sha256: Sha256Digest::of(&bytes),
        },
    )?;
    files.insert(path, bytes);
    Ok(())
}

// One native path/file insertion rule for actual output and prospective reservations.
fn inventory_file(
    builder: &mut InventoryBuilder,
    directories: &mut BTreeSet<EntryPath>,
    path: EntryPath,
    content: Content,
) -> Result<(), SiteRenderError> {
    for (offset, _) in path.as_str().match_indices('/') {
        let parent: EntryPath = path.as_str()[..offset]
            .parse()
            .map_err(SiteRenderError::Output)?;
        if directories.insert(parent.clone()) {
            builder
                .insert(parent, Content::Directory)
                .map_err(SiteRenderError::Output)?;
        }
    }
    builder
        .insert(path, content)
        .map_err(SiteRenderError::Output)?;
    Ok(())
}
