//! One local authored projection feeds both presentation and machine inspection.

use std::{collections::BTreeMap, fmt};

use gazette_compiler::authored::{self, ResolvedAuthored};
use gazette_core::{
    publication::{CapabilityId, Key, content::ContentNode},
    render,
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;
use serde_json::Value;

use super::{
    CapabilityProjection, CompiledTheme, ResourceProjection, TemplateCompilation,
    ThemeResourceProjection,
    serialization::{context, encode},
};

/// Exact adapters and retained views used to project authored content.
#[derive(Clone, Copy)]
pub(super) struct ProjectionInputs<'a> {
    pub social_card: Option<&'a super::social_cards::Capture>,
    pub social_card_limits: super::social_cards::RequestLimits,
    pub raster: &'a gazette_core::raster::Inspect<'a>,
    pub markdown: &'a dyn gazette_ports::markdown::Markdown,
    pub discovery: Option<&'a authored::Discovery<'a>>,
}

pub(super) struct PageRenderContext<'projection, 'page, 'resources> {
    pub render_year: Option<authored::RenderYear>,
    pub graph: Option<&'resources super::presentation::GraphDiscovery>,
    pub feeds: &'resources [super::presentation::FeedDiscovery],
    pub discovery: &'resources [super::presentation::OutputDiscovery],
    pub alternate: Option<&'resources authored::MarkdownAlternate>,
    pub listing_pages: &'resources [authored::ListingPage],
    pub capabilities: &'resources BTreeMap<CapabilityId, CapabilityProjection>,
    pub theme_resources: &'resources [ThemeResourceProjection],
    pub resources: &'resources [ResourceProjection],
    pub browser_modules: Option<&'resources super::BrowserModules>,
    pub projection: ProjectionInputs<'projection>,
    pub prepared: Option<authored::PageProjection<'page>>,
    pub documentation_catalog: Option<&'resources super::DocumentationCatalog>,
    pub interaction: Option<&'resources ResourceProjection>,
    pub chrome: Option<&'resources authored::SiteChromeView<'page>>,
    pub identity_assets: Option<super::presentation::IdentityAssets<'resources>>,
    pub social_card: Option<&'resources super::SocialCardProjection>,
}

#[derive(Serialize, schemars::JsonSchema)]
struct Model<'a, 'page, P> {
    #[serde(flatten)]
    page: P,
    presentation: super::presentation::Presentation<'a>,
    /// Compiler-selected indexes containing this page or a declared editorial ancestor.
    /// Exact browsing destinations, not extra parents or a record of arrival.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    browsing_locations: &'a Vec<authored::BrowsingLocation<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<serde_json::Value>")]
    json_ld: Option<authored::PublicJsonLd<'a, 'page>>,
}

pub(super) fn template_context_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Model<'static, 'static, authored::PageProjection<'static>>>()
}

/// Local, unadmitted render evidence; native source coordinates make this private.
#[derive(Debug, Serialize)]
pub struct PageRenderEvidence<'a> {
    /// Exact rendered notebook identities in authored declaration order; private evidence.
    pub notebooks: Vec<authored::NotebookEvidence>,
    /// Exact compiled Theme selection and retained input identities.
    pub theme: &'a TemplateCompilation,
    /// Explicit logical template entry, never inferred from the content key or kind.
    pub template: EntryPath,
    /// Full authored-document identity; never passed to the template as metadata.
    pub source: authored::Origin,
    /// SHA-256 of compact JSON with recursively sorted object keys, supplied to Theme.
    /// This local encoding is not a release-signature canonicalization contract.
    pub model_sha256: Sha256Digest,
    /// SHA-256 of the exact complete emitted UTF-8 bytes.
    pub html_sha256: Sha256Digest,
}

/// Complete local rendering and its shared model, not a certified public page.
///
/// Draft/private structural values intentionally remain visible for local inspection.
/// No rights, disclosure, route, capability, release or publication gate is implied.
/// Construction stays private so failures cannot return a successful partial result.
#[derive(Debug, Serialize)]
pub struct RenderedPage<'a> {
    pub(super) evidence: PageRenderEvidence<'a>,
    pub(super) model: Value,
    pub(super) html: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) search: Option<Value>,
    #[serde(skip)]
    pub(super) projection: authored::PageProjection<'a>,
    #[serde(skip)]
    pub(super) listing: Option<Value>,
}

impl RenderedPage<'_> {
    /// Exact input/output bindings, separately from the template's model.
    pub fn evidence(&self) -> &PageRenderEvidence<'_> {
        &self.evidence
    }
    /// The same model value consumed by the renderer, not independently reconstructed JSON.
    pub fn model(&self) -> &Value {
        &self.model
    }
    /// Complete UTF-8 output. It has not passed public admission or certification.
    pub fn html(&self) -> &str {
        &self.html
    }
    /// Selected complete search record from the same typed page projection.
    /// Absence means this site's brand configuration did not select search.
    pub fn search(&self) -> Option<&Value> {
        self.search.as_ref()
    }
}

/// A connected local render failed without a successful partial page or model.
#[derive(Debug)]
pub enum PageRenderError {
    /// This page declares resources but the caller supplied no complete compilation.
    UncompiledResources(Key),
    /// Interactive content requires its exact generated bootstrap and selected modules.
    UncompiledInteraction(Key),
    /// Content and Theme were resolved from different workspace bytes.
    Configuration {
        /// Compiled Theme's exact configuration.
        expected: Sha256Digest,
        /// Authored model's exact configuration.
        observed: Sha256Digest,
    },
    /// Selected content belongs to another site; cross-site links do not relocate it.
    Site {
        /// Selected Theme's consuming site.
        expected: Key,
        /// Authored object's declared home site.
        observed: Key,
    },
    /// Exact object selection or retained document projection failed.
    Projection(authored::ProjectionError),
    /// Shared serialization bounds or the rendering adapter failed.
    Render(render::Error),
    /// The rendering adapter returned bytes that cannot be a UTF-8 page.
    Utf8(std::string::FromUtf8Error),
}

impl PageRenderError {
    /// Stable diagnostic class; subordinate errors preserve their native class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::UncompiledResources(_) => "page_uncompiled_resources",
            Self::UncompiledInteraction(_) => "page_uncompiled_interaction",
            Self::Configuration { .. } => "page_configuration",
            Self::Site { .. } => "page_site",
            Self::Projection(error) => error.code(),
            Self::Render(error) => error.code(),
            Self::Utf8(_) => "page_output_utf8",
        }
    }
}
impl fmt::Display for PageRenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UncompiledInteraction(id) => write!(
                f,
                "page {id} needs its selected interaction compiled; use the retained publication build path"
            ),
            Self::UncompiledResources(id) => write!(
                f,
                "page {id} needs its declared resources compiled; use the retained publication build path"
            ),
            Self::Configuration { expected, observed } => write!(
                f,
                "content/Theme configuration mismatch: expected {expected:?}, observed {observed:?}; resolve both from the same workspace bytes"
            ),
            Self::Site { expected, observed } => write!(
                f,
                "content home {observed} differs from Theme site {expected}; select that home's composition"
            ),
            Self::Projection(error) => error.fmt(f),
            Self::Render(error) => error.fmt(f),
            Self::Utf8(error) => write!(
                f,
                "renderer returned invalid UTF-8: {error}; no page produced"
            ),
        }
    }
}
impl std::error::Error for PageRenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Projection(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Utf8(error) => Some(error),
            _ => None,
        }
    }
}

impl CompiledTheme {
    /// Render one exact authored object from the same workspace and home-site binding.
    ///
    /// The compiler derives one typed page projection from retained syntax and its
    /// shared graph. Only its serialized model reaches the renderer; private source
    /// evidence stays separate. Articles, documentation and authored container
    /// landing pages all use this path. Lifecycle is preserved, not public admission.
    /// No native reads, content rewriting, inferred template selection or writes occur.
    /// Standalone syntax needs source-bound discovery only for configured listings.
    ///
    /// # Errors
    /// Refuses configuration/site mismatch before body rendering; then propagates
    /// projection, context-size, template/output and UTF-8 failures without partial success.
    pub fn render_page<'a>(
        &'a self,
        authored: &'a ResolvedAuthored<'_, '_>,
        content: &Key,
        template: &EntryPath,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<RenderedPage<'a>, PageRenderError> {
        self.checked_page(authored, content)?;
        let chrome = authored
            .site_chrome_with_composition(&self.evidence().site, &self.composition)
            .map_err(PageRenderError::Projection)?;
        let listing_pages = if authored
            .content()
            .compilation()
            .workspace
            .publication_for(&self.evidence().site)
            .is_some_and(|publication| !publication.listings.is_empty())
        {
            let discovery = authored.discovery().map_err(|error| {
                PageRenderError::Projection(authored::ProjectionError::Source(error))
            })?;
            authored
                .project_listing_pages(&self.evidence().site, &discovery)
                .map_err(|error| {
                    PageRenderError::Projection(authored::ProjectionError::Listing(error))
                })?
        } else {
            Vec::new()
        };
        self.render_page_with_resources(
            authored,
            content,
            template,
            PageRenderContext {
                render_year: None,
                graph: None,
                feeds: &[],
                discovery: &[],
                alternate: None,
                listing_pages: &listing_pages,
                capabilities: &BTreeMap::new(),
                theme_resources: &[],
                resources: &[],
                browser_modules: None,
                projection: ProjectionInputs {
                    social_card: None,
                    social_card_limits: Default::default(),
                    raster,
                    markdown,
                    discovery: None,
                },
                prepared: None,
                documentation_catalog: None,
                interaction: None,
                chrome: chrome.as_ref(),
                identity_assets: None,
                social_card: None,
            },
        )
    }

    fn checked_page<'a>(
        &self,
        authored: &'a ResolvedAuthored<'_, '_>,
        content: &Key,
    ) -> Result<&'a ContentNode, PageRenderError> {
        let expected = self.evidence().configuration_sha256;
        let observed = authored.content().compilation().source_sha256;
        if expected != observed {
            return Err(PageRenderError::Configuration { expected, observed });
        }
        let node = authored.content().graph().node(content).ok_or_else(|| {
            PageRenderError::Projection(authored::ProjectionError::UnknownContent(content.clone()))
        })?;
        if node.site != self.evidence().site {
            return Err(PageRenderError::Site {
                expected: self.evidence().site.clone(),
                observed: node.site.clone(),
            });
        }
        Ok(node)
    }

    pub(super) fn render_page_with_resources<'a>(
        &'a self,
        authored: &'a ResolvedAuthored<'_, '_>,
        content: &Key,
        template: &EntryPath,
        input: PageRenderContext<'_, 'a, '_>,
    ) -> Result<RenderedPage<'a>, PageRenderError> {
        let PageRenderContext {
            render_year,
            graph,
            feeds,
            discovery,
            alternate,
            listing_pages,
            capabilities,
            theme_resources,
            resources,
            browser_modules,
            projection: projection_inputs,
            prepared,
            documentation_catalog,
            interaction,
            chrome,
            identity_assets,
            social_card,
        } = input;
        let node = self.checked_page(authored, content)?;
        if authored.resources(content).len() != resources.len() {
            return Err(PageRenderError::UncompiledResources(content.clone()));
        }
        let mut projection = match prepared {
            Some(projection) => projection,
            None => authored
                .project_page_with_renditions(
                    content,
                    projection_inputs.raster,
                    projection_inputs.markdown,
                    projection_inputs.discovery,
                    Some(gazette_compiler::markdown::ComponentPresentation {
                        definitions: &self.components,
                        renderer: self,
                    }),
                    &super::page_resources::dimensions(resources),
                )
                .map_err(PageRenderError::Projection)?,
        };
        if let Some(metadata) = projection.data.site.metadata.as_mut() {
            metadata.render_year = render_year;
        }
        for project in &mut projection.data.projects {
            project.code_documentation =
                super::documentation_reading::project_work(documentation_catalog, project.project)
                    .map_err(PageRenderError::Render)?;
        }
        if projection
            .component_uses()
            .any(|component| component.interaction.is_some())
            != interaction.is_some()
        {
            return Err(PageRenderError::UncompiledInteraction(content.clone()));
        }
        // Reuse the site's already bounded listing projection. Each slice contributes
        // at most one shallow location, so this never copies its item catalogue.
        let listing_locations = listing_pages
            .iter()
            .filter(|listing| {
                listing.variant == gazette_compiler::workspace::ListingVariant::Primary
            })
            .filter_map(|listing| {
                listing.location_for(content).or_else(|| {
                    projection
                        .data
                        .breadcrumbs
                        .iter()
                        .flat_map(|trail| &trail.items)
                        .find_map(|item| listing.location_for(&item.node.id))
                })
            });
        let browsing_locations = authored
            .project_browsing_locations_with(
                content,
                projection_inputs.discovery,
                listing_locations,
            )
            .map_err(PageRenderError::Projection)?;
        let publication = authored
            .content()
            .compilation()
            .workspace
            .publication_for(&node.site);
        let json_ld = publication
            .filter(|publication| publication.machine.json_ld)
            .map(|_| match projection_inputs.discovery {
                Some(discovery) => projection
                    .discovery_json_ld(discovery)
                    .map(authored::JsonLd::public)
                    .ok_or_else(|| {
                        context_error(serde::ser::Error::custom(
                            "JSON-LD discovery owner mismatch",
                        ))
                    }),
                None => Ok(projection.json_ld().public()),
            })
            .transpose()?;
        let search = publication
            .filter(|publication| {
                publication.machine.search.is_some()
                    && publication.search_page.as_ref() != Some(content)
                    && projection_inputs
                        .discovery
                        .is_none_or(|selection| selection.contains(content))
            })
            .map(|publication| {
                let selection = publication.machine.search.as_ref().ok_or_else(|| {
                    context_error(serde::ser::Error::custom(
                        "search configuration binding missing",
                    ))
                })?;
                let bytes = if let Some(discovery) = projection_inputs.discovery {
                    let listing = projection.listing(discovery).ok_or_else(|| {
                        context_error(serde::ser::Error::custom("search discovery owner mismatch"))
                    })?;
                    encode(
                        &listing.search_with(selection).ok_or_else(|| {
                            context_error(serde::ser::Error::custom(
                                "search listing eligibility mismatch",
                            ))
                        })?,
                        self.evidence().limits.context_bytes,
                        "search document bytes",
                    )
                } else {
                    encode(
                        &projection.search_with(selection),
                        self.evidence().limits.context_bytes,
                        "search document bytes",
                    )
                }
                .map_err(PageRenderError::Render)?;
                serde_json::from_slice(&bytes).map_err(context_error)
            })
            .transpose()?;
        // Bound bytes before allocating the renderer-facing Value. This is JSON
        // transport of the typed projection, not a second semantic/config parser.
        let search_control = chrome.and_then(|chrome| chrome.shell.search.as_ref());
        let search_binding = super::search::binding(authored.content(), &node.site, search_control)
            .map_err(PageRenderError::Render)?;
        let webmcp = capabilities
            .get(&gazette_core::publication::CoreCapabilityCatalog::publication_webmcp())
            .filter(|value| matches!(value, CapabilityProjection::Available { .. }))
            .and(projection_inputs.discovery)
            .and_then(|discovery| discovery.node(node))
            .zip(projection.data.route)
            .map(|(content, route)| super::presentation::BrowserPage {
                content,
                route,
                body_text: &projection.body_text,
            });
        let presentation = || {
            let mut presentation = super::presentation::Presentation::page(
                self,
                capabilities,
                theme_resources,
                resources,
                browser_modules,
                content,
                interaction,
                chrome,
                search_binding.as_ref(),
                identity_assets,
            );
            presentation.graph = graph;
            presentation.feeds = feeds;
            presentation.discovery = discovery;
            presentation.markdown_alternate = alternate;
            presentation.webmcp = webmcp.as_ref();
            presentation.social_card = social_card;
            presentation
        };
        let listing = projection_inputs
            .discovery
            .and_then(|discovery| projection.listing(discovery).map(|page| (discovery, page)))
            .map(|(discovery, page)| {
                encode(
                    &Model {
                        browsing_locations: &browsing_locations,
                        page,
                        presentation: presentation(),
                        json_ld: publication
                            .filter(|publication| publication.machine.json_ld)
                            .and_then(|_| projection.listing_json_ld(discovery))
                            .map(authored::JsonLd::public),
                    },
                    self.evidence().limits.context_bytes,
                    "listing bytes",
                )
                .map_err(PageRenderError::Render)
                .and_then(|bytes| serde_json::from_slice(&bytes).map_err(context_error))
            })
            .transpose()?;
        let bytes = if let Some(discovery) = projection_inputs.discovery {
            let page = projection.discovery_view(discovery).ok_or_else(|| {
                context_error(serde::ser::Error::custom("page discovery owner mismatch"))
            })?;
            encode(
                &Model {
                    browsing_locations: &browsing_locations,
                    page,
                    json_ld,
                    presentation: presentation(),
                },
                self.evidence().limits.context_bytes,
                "context bytes",
            )
        } else {
            encode(
                &Model {
                    browsing_locations: &browsing_locations,
                    page: &projection,
                    json_ld,
                    presentation: presentation(),
                },
                self.evidence().limits.context_bytes,
                "context bytes",
            )
        }
        .map_err(PageRenderError::Render)?;
        let model: Value = serde_json::from_slice(&bytes).map_err(context_error)?;
        let (model, encoded) =
            context(model, self.evidence().limits.context_bytes, "context bytes")
                .map_err(PageRenderError::Render)?;
        let model_sha256 = Sha256Digest::of(&encoded);
        let bytes = self
            .render(template, &model)
            .map_err(PageRenderError::Render)?;
        let html_sha256 = Sha256Digest::of(&bytes);
        let html = String::from_utf8(bytes).map_err(PageRenderError::Utf8)?;
        Ok(RenderedPage {
            evidence: PageRenderEvidence {
                notebooks: projection.notebooks.clone(),
                theme: self.evidence(),
                template: template.clone(),
                source: projection.data.source.clone(),
                model_sha256,
                html_sha256,
            },
            model,
            html,
            search,
            projection,
            listing,
        })
    }
}

fn context_error(error: serde_json::Error) -> PageRenderError {
    PageRenderError::Render(render::Error::Context(Box::new(error)))
}
