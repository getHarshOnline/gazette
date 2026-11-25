//! Local, unadmitted presentation views over the existing resolved corpus.

use std::{collections::BTreeMap, fmt};

use gazette_core::publication::{
    Key, Principal, Project, PublicationRole,
    content::{AttributionRole, ContentKind, ContentNode, ProjectDeclaration, ProjectLink},
};
use gazette_core::source::EntryPath;
use serde::Serialize;

use super::{
    AssetError, BreadcrumbTrail, CollectionNavigation, ContentView, JourneyMembership, Origin,
    ResolvedAuthored, SitePresentation, StaticAsset, navigation::NavigationBuilder,
};
use crate::{
    document,
    markdown::{self, Summary},
    workspace::ContentSource,
};

/// Minimal home-site coordinates, excluding private composition configuration.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct SiteView<'a> {
    /// Shared public brand, home, language and contact/profile contract, without private machinery.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<super::PublicationMetadata<'a>>,
    /// Exact home identity; cross-site relationships never relocate content.
    pub id: &'a Key,
    /// Configured publisher role, not authorship or release authorization.
    pub publisher: &'a Key,
    /// Exact declared publisher facts, independent of authorship and site branding.
    pub publisher_principal: &'a Principal,
    /// Publisher's explicitly configured website through the shared address model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher_website: Option<super::PrincipalWebsiteView>,
    /// Site-scoped rights selection, independent from this page's attribution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<&'a Key>,
    /// Publication's author default, applied only to unattributed published articles.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_author: Option<&'a Key>,
    /// Exact site-scoped responsibilities, independent of page attribution.
    pub roles: Vec<PublicationRoleView<'a>>,
    /// Effective Theme/palette identity only; provider and package readiness stay separate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
    /// Absolute terms-of-use address for linked data; Themes read it through discovery.
    #[serde(skip)]
    pub terms_url: Option<String>,
}

/// One site responsibility joined to its declared principal, without inferred authority.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PublicationRoleView<'a> {
    /// Borrowed source principal; never a copied or synthesized identity.
    pub principal: &'a Principal,
    /// Explicit principal website through the shared address selection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<super::PrincipalWebsiteView>,
    /// Exact responsibility declared for this site only.
    pub role: PublicationRole,
}

/// A direct content role bound to its exact declared principal, not admitted identity data.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct AttributionView<'a> {
    /// Borrowed principal reference; no global author inventory or guessed profile.
    pub principal: &'a Principal,
    /// Explicit website, with separate serving and public identity URLs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<super::PrincipalWebsiteView>,
    /// Page role, including the publication's author default where applicable.
    pub role: AttributionRole,
}

/// Catalog identity joined to the graph's already resolved relationship evidence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectView<'a> {
    /// Resolved public entity IRI, or document-local identity without a public anchor.
    pub identity: String,
    /// Shallow configured reading destinations whose graph relation is About this subject.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[schemars(default)]
    pub works: Vec<ContentView<'a>>,
    /// Declared code-documentation destination joined to its validated provider page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_documentation: Option<super::ProjectDocumentation>,
    /// Exact configured project, retaining independent state axes.
    pub project: &'a Project,
    /// Description from this subject's owning landing.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) description: Option<&'a str>,
    /// Public entity identities of the parent programme and its declared members.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) programme: Option<String>,
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) programme_members: Vec<String>,
    /// Public brand identities joined from the workspace portfolio.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) portfolio_brands: Vec<String>,
    /// The brand represented by this research programme.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) research_brand: Option<String>,
    /// Credit from the project's source citation, with its resolved public profile.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) source_author: Option<(&'a Principal, Option<super::PrincipalWebsiteView>)>,
    /// Independently declared source rights holder and its public website.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) source_rights_holder: Option<(&'a Principal, Option<super::PrincipalWebsiteView>)>,
    /// Existing final relationship and declaration origins; no second inheritance pass.
    pub relationship: &'a ProjectLink,
    /// Explicit project landing, not a fallback to its documentation or mentions index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing: Option<super::ProjectLanding<'a>>,
    /// Same shallow native resource projection used by hosted components.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<gazette_core::markdown::ProjectResourcesView>,
}

/// Original project declarations remain distinct from inherited final page links.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ProjectDeclarationView<'a> {
    /// Exact catalog project without independent state inference.
    pub project: &'a Project,
    /// Selected node's declaration, preserving About, Mentions or container Context.
    pub declaration: &'a ProjectDeclaration,
}

/// Local unadmitted presentation data, never a public-ready page or release receipt.
///
/// All lifecycles and project states remain visible for inspection. No authors,
/// rights, canonical URLs or permission are inferred. Related records borrow the
/// same bounded graph; native paths and workspace configuration are not copied.
#[derive(Debug, schemars::JsonSchema)]
pub struct PageProjection<'a> {
    /// Selected alternate bytes from the same admitted plan; not duplicated into machine metadata.
    #[schemars(skip)]
    pub body_markdown: Option<String>,
    /// Private retained notebook/render identities; never forwarded as public metadata.
    #[schemars(skip)]
    pub notebooks: Vec<super::NotebookEvidence>,
    /// Shared typed metadata; presentation and search do not reconstruct it.
    #[schemars(flatten)]
    pub data: PageData<'a>,
    /// Private formatter boundary; serialization exposes only the borrowed HTML fragments.
    #[schemars(with = "Option<crate::markdown::Opening<'static>>")]
    #[schemars(skip_serializing_if = "Option::is_none")]
    opening: Option<gazette_core::markdown::OpeningBoundary>,
    /// Body fragment from the retained Markdown AST and its original render policy.
    pub body_html: String,
    /// Text from the same retained AST, including code examples and image alt text.
    pub body_text: String,
    /// Explicit retained-body excerpt, not an authored description or inferred abstract.
    #[schemars(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Summary>,
}

impl PageProjection<'_> {
    /// Borrow the exact leading H1 and remainder without parsing or copying the emitted HTML.
    pub fn opening(&self) -> Option<crate::markdown::Opening<'_>> {
        let end = self.opening?.heading_end;
        Some(crate::markdown::Opening {
            heading_html: self.body_html.get(..end)?,
            remainder_html: self.body_html.get(end..)?,
        })
    }
}

impl Serialize for PageProjection<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Wire<'a, 'data> {
            #[serde(flatten)]
            data: &'a PageData<'data>,
            body_html: &'a str,
            body_text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            summary: Option<&'a Summary>,
            #[serde(skip_serializing_if = "Option::is_none")]
            opening: Option<crate::markdown::Opening<'a>>,
        }
        Wire {
            data: &self.data,
            body_html: &self.body_html,
            body_text: &self.body_text,
            summary: self.summary.as_ref(),
            opening: self.opening(),
        }
        .serialize(serializer)
    }
}

/// Common resolved metadata for human and machine projections of one page.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PageData<'a> {
    /// A validated body slot owns the container output; false preserves the default append.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[schemars(default)]
    pub container_contents_placed: bool,
    /// Workspace-selected page invitations, independent from footer destinations.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    #[schemars(default)]
    pub invitations: std::collections::BTreeMap<Key, super::InvitationView<'a>>,
    /// Publication-selected source invitation resolved against the canonical public host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_source: Option<crate::workspace::PreferredSourceView>,
    /// Resources from one direct About project on its page or configured root collection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_resources: Option<gazette_core::markdown::ProjectResourcesView>,
    /// Optional authored citation with resolved serving destinations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation: Option<super::CitationView<'a>>,
    /// Configured processing context only for explicitly bound owner-held legal outputs.
    /// It supplies neither observed live providers nor publication approval.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legal_processing: Option<super::LegalProcessingContext<'a>>,
    /// Explicit native documentation mount and its shared routed landing, if selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_mount: Option<super::DocumentationMountView<'a>>,
    /// Compiler-selected reading references and pagination for an existing index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listing: Option<super::ListingPage>,
    /// Optional article-owned commentary, not an independent page or appended body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<super::ArticleNotes>,
    /// Validated authored component data from the same body projection, without native spans.
    /// Shared by review, templates and machine metadata; no template-derived meaning.
    pub components: Vec<markdown::ComponentUse>,
    /// Compiler-proved annotations for anchors emitted inside `body_html`.
    pub links: Vec<markdown::LinkUse>,
    /// Exact selected content, including its original structure and lifecycle.
    pub content: ContentView<'a>,
    /// Exact declared taxonomy memberships joined once to scoped display metadata.
    pub classifications: Vec<super::ClassificationView<'a>>,
    /// Identity, publisher and explicit responsibilities from the bound home site.
    pub site: SiteView<'a>,
    /// The public brand graph for a bound homepage or About page.
    #[serde(skip)]
    #[schemars(skip)]
    pub(super) brand_graph: Option<super::brand_graph::BrandGraph<'a>>,
    /// Source-owned selected discussion binding; provider readiness is separate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discussion: Option<super::DiscussionView<'a>>,
    /// Resolved page attributions, including an eligible publication default.
    /// Empty remains empty; the site's publisher never becomes a fallback author.
    pub attributions: Vec<AttributionView<'a>>,
    /// Explicit primary subject, resolved independently from attribution and publisher.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<super::SubjectView<'a>>,
    /// Compiler-owned canonical address, output and redirects, when explicitly bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<&'a crate::workspace::ResolvedRoute>,
    /// Private complete-document identity, excluded from template/model serialization.
    /// No field span is invented; an operator evidence envelope may carry this separately.
    #[serde(skip)]
    #[schemars(skip)]
    pub source: Origin,
    /// Configured site/project source context, not a subject, author or rights claim.
    /// Only identities serialize; native coordinates remain private evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ContentSource<'a>>,
    /// Native-profile derivation evidence, absent for explicitly framed metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub derivation: Option<&'a super::NativeDerivation>,
    /// Primary Markdown heading IDs, levels and parent indexes used by the renderer.
    /// Notes and their notebooks keep their own namespaced anchors. This is local
    /// navigation evidence, not a second slug or hierarchy derivation.
    pub headings: &'a [crate::markdown::Heading],
    /// Package-owned and referenced static transports from the same site plan,
    /// deduplicated in URL order.
    /// Opaque contents are not validated MIME, transformed images or admitted disclosure.
    pub assets: Vec<&'a StaticAsset<'a>>,
    /// Emitted body Markdown images; Notes, including notebooks, keep their own list.
    pub images: Vec<markdown::ImageUse>,
    /// Explicit leaf collections, or the container's direct containing collection.
    #[schemars(schema_with = "super::navigation::discovery_nodes_schema")]
    pub collections: Vec<ContentView<'a>>,
    /// Direct sibling navigation in the collection's resolved member order.
    /// Journey residents retain their own sequence and do not acquire a second residence.
    pub collection_navigation: Vec<CollectionNavigation<'a>>,
    /// Complete graph-owned documentation outline, with authored grouping and order.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation_navigation: Option<super::DocumentationNavigation<'a>>,
    /// Explicit containing journeys in identity order, never inferred from collections.
    pub journeys: Vec<JourneyMembership<'a>>,
    /// Every explicit structural path, not an inferred primary collection or site menu.
    pub breadcrumbs: Vec<BreadcrumbTrail<'a>>,
    /// Selected journey's members in declared order; empty for other kinds.
    pub steps: Vec<ContentView<'a>>,
    /// Selected collection's direct children in its resolved member order.
    #[schemars(schema_with = "super::navigation::discovery_nodes_schema")]
    pub children: Vec<ContentView<'a>>,
    /// Outgoing editorial targets only, without structural inheritance.
    #[schemars(schema_with = "super::navigation::discovery_nodes_schema")]
    pub related: Vec<ContentView<'a>>,
    /// Derived source-eligible suggestions, separate from authored editorial links.
    pub recommendations: super::Recommendations<'a>,
    /// Final direct/contextual project relationships in project identity order.
    pub projects: Vec<ProjectView<'a>>,
    /// Original selected-node project declarations, including container context.
    pub project_declarations: Vec<ProjectDeclarationView<'a>>,
}

/// Searchable text plus the same resolved metadata supplied to presentation.
/// This is a document corpus record, not a ranking index or admission decision.
#[derive(Debug, Serialize)]
pub struct SearchDocument<'a, 'data> {
    #[serde(flatten)]
    data: &'a PageData<'data>,
    body: &'a str,
    /// Plain excerpt text only; search never embeds the rendered excerpt HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<&'a str>,
}

impl PageProjection<'_> {
    /// Borrow all rendered component uses, including article-owned commentary.
    ///
    /// Enhancement consumers share this closure without merging notes into the
    /// main body or giving commentary an independent publication identity.
    pub fn component_uses(&self) -> impl Iterator<Item = &markdown::ComponentUse> {
        self.data.components.iter().chain(
            self.data
                .notes
                .iter()
                .flat_map(|notes| notes.components.iter()),
        )
    }

    /// Borrow the complete search document without parsing or resolving again.
    pub fn search(&self) -> SearchDocument<'_, '_> {
        SearchDocument {
            data: &self.data,
            body: &self.body_text,
            summary: self.summary.as_ref().map(|summary| summary.text.as_str()),
        }
    }
}

/// A local view cannot be produced completely from its retained owners.
#[derive(Debug)]
pub enum ProjectionError {
    /// Optional authored citation could not be resolved.
    Citation(super::CitationError),
    /// A configured generated index could not be resolved completely.
    Listing(super::ListingError),
    /// Complete emitted lineage would expose a nonpublic catalog endpoint.
    ProjectOrigin(super::ProjectOriginError),
    /// An ordered notebook attachment cannot bind to this page's source.
    NotebookBinding(super::NotebookError),
    /// A retained notebook cannot be rendered by the selected static profile.
    Notebook {
        /// Exact logical retained input; never an ambient path.
        path: EntryPath,
        /// Original renderer failure; no placeholder or raw-source fallback.
        error: crate::notebook::RenderError,
    },
    /// Derived navigation exceeded its independent complete-view allowance.
    Limit(&'static str),
    /// Borrowed projection serialization failed without a partial success.
    Encoding(serde_json::Error),
    /// The exact requested identity does not exist; titles and paths are not fallbacks.
    UnknownContent(Key),
    /// A resolved binding invariant is missing, retaining its exact failing identity.
    Binding {
        /// Missing content, site, project or document identity.
        id: Key,
        /// The violated binding contract.
        reason: &'static str,
    },
    /// A configured source and its document disagree about the publication home.
    Source(super::SourceBindingError),
    /// The selected site's single static-transport plan could not be resolved.
    Asset(AssetError),
    /// Structured-source failure, located in original YAML when possible.
    Landing {
        /// Original source identity and containing construct.
        origin: Origin,
        /// Stable underlying diagnostic class.
        code: &'static str,
        /// Bounded diagnostic with any normalized coordinates explicitly named.
        message: String,
    },
    /// Existing document rendering policy or limits rejected the body.
    Document {
        /// Exact complete source identity; original error retains local coordinates.
        origin: Origin,
        /// Original parser/render failure without classification loss.
        error: Box<document::Error>,
    },
}

impl ProjectionError {
    pub(super) fn body(
        path: &EntryPath,
        file: &crate::corpus::File<'_>,
        error: markdown::Error,
    ) -> Self {
        let mut origin = Origin {
            path: path.clone(),
            source_sha256: file.source_sha256(),
            span: None,
        };
        if matches!(file, crate::corpus::File::Landing(_)) {
            origin.span = error
                .location()
                .and_then(|location| file.body_location(location))
                .map(|location| location.span);
            return Self::Landing {
                origin,
                code: error.code(),
                message: format!("normalized body: {error}"),
            };
        }
        Self::Document {
            origin,
            error: Box::new(document::Error::Markdown {
                error,
                offset: file.body_offset().unwrap_or(0),
            }),
        }
    }
    /// Stable diagnostic class; document failures retain their native code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Citation(error) => error.code(),
            Self::Listing(error) => error.code(),
            Self::ProjectOrigin(error) => error.code(),
            Self::NotebookBinding(error) => error.code(),
            Self::Notebook { error, .. } => error.code(),
            Self::Limit(_) => "authored_projection_limit",
            Self::Encoding(_) => "authored_projection_encoding",
            Self::UnknownContent(_) => "authored_content_reference",
            Self::Binding { .. } => "authored_projection_binding",
            Self::Source(error) => error.code(),
            Self::Asset(error) => error.code(),
            Self::Document { error, .. } => error.code(),
            Self::Landing { code, .. } => code,
        }
    }
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Citation(error) => error.fmt(f),
            Self::Listing(error) => error.fmt(f),
            Self::ProjectOrigin(error) => error.fmt(f),
            Self::NotebookBinding(error) => error.fmt(f),
            Self::Notebook { path, error } => write!(f, "notebook {path}: {error}"),
            Self::Limit(unit) => write!(
                f,
                "authored projection exceeds {unit} allowance; narrow the selected structure or explicitly raise its limit"
            ),
            Self::Encoding(error) => write!(f, "authored projection encoding failed: {error}"),
            Self::UnknownContent(id) => write!(f, "unknown authored content {id}"),
            Self::Binding { id, reason } => write!(f, "authored projection {id}: {reason}"),
            Self::Source(error) => error.fmt(f),
            Self::Asset(error) => error.fmt(f),
            Self::Landing {
                origin, message, ..
            } => write!(
                f,
                "landing {:?} at {:?}: {message}",
                origin.path, origin.span
            ),
            Self::Document { origin, error } => {
                write!(f, "authored projection {:?}: {error}", origin.path)
            }
        }
    }
}

impl std::error::Error for ProjectionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Citation(error) => Some(error),
            Self::Listing(error) => Some(error),
            Self::ProjectOrigin(error) => Some(error),
            Self::NotebookBinding(error) => Some(error),
            Self::Notebook { error, .. } => Some(error),
            Self::Encoding(error) => Some(error),
            Self::Document { error, .. } => Some(error.as_ref()),
            Self::Source(error) => Some(error),
            Self::Asset(error) => Some(error),
            _ => None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Project one exact content object from the shared graph and retained document.
    ///
    /// This is local inspection/presentation, not policy admission. Structural
    /// reverse lookups stay within this resolved graph; project inheritance and
    /// Markdown parsing are never repeated. No native source is opened.
    /// Stored raster outputs use the supplied pure decoder; callers selecting
    /// `raster::unavailable` receive an explicit failure if a page needs it.
    ///
    /// # Errors
    /// Rejects unknown identities, missing resolved bindings and existing body-render
    /// failures without a successful partial view or alternate identity lookup.
    pub fn project_page(
        &self,
        id: &Key,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PageProjection<'_>, ProjectionError> {
        self.project_page_with(id, raster, markdown, None, None)
    }

    /// Render the same exact page while excluding ineligible generated recommendations.
    /// Authored body links, the selected node and its explicit journey steps remain intact.
    /// Use the returned page's `discovery_view` for emitted serialization and `listing`
    /// for aggregate records; private `project_page` remains complete inspection.
    ///
    /// # Errors
    /// Rejects a discovery view from another resolved source before rendering, and
    /// preserves the ordinary page projection's complete failure behavior.
    pub fn project_discovery_page(
        &self,
        id: &Key,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        discovery: &super::Discovery<'_>,
    ) -> Result<PageProjection<'_>, ProjectionError> {
        self.project_page_with(id, raster, markdown, Some(discovery), None)
    }

    /// Project the same resolved page with selected discovery and static component presentation.
    /// The optional consumer changes formatting only; source and relationship resolution stay here.
    ///
    /// # Errors
    /// Rejects a foreign discovery owner and preserves ordinary source/render failures.
    pub fn project_page_with(
        &self,
        id: &Key,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        discovery: Option<&super::Discovery<'_>>,
        components: Option<markdown::ComponentPresentation<'_>>,
    ) -> Result<PageProjection<'_>, ProjectionError> {
        self.project_page_with_renditions(
            id,
            raster,
            markdown,
            discovery,
            components,
            &BTreeMap::new(),
        )
    }

    /// Project a page with the independently compiled image extents for its exact
    /// resource URLs. Unknown extents remain absent, never guessed from recipes.
    ///
    /// # Errors
    /// Preserves projection failures and rejects invalid supplied dimensions.
    pub fn project_page_with_renditions(
        &self,
        id: &Key,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
        discovery: Option<&super::Discovery<'_>>,
        components: Option<markdown::ComponentPresentation<'_>>,
        dimensions: &BTreeMap<String, gazette_core::resource::ImageLayout>,
    ) -> Result<PageProjection<'_>, ProjectionError> {
        if dimensions
            .values()
            .any(|value| matches!(value, gazette_core::resource::ImageLayout::Raster(size) if size.width == 0 || size.height == 0))
        {
            return Err(ProjectionError::Binding {
                id: id.clone(),
                reason: "compiled image extent must be positive",
            });
        }
        if discovery.is_some_and(|view| !view.matches(self)) {
            return Err(ProjectionError::Binding {
                id: id.clone(),
                reason: "discovery belongs to another resolved source",
            });
        }
        let eligible = |id: &Key| discovery.is_none_or(|view| view.contains(id));
        let graph = self.content.graph();
        let content = graph
            .node(id)
            .ok_or_else(|| ProjectionError::UnknownContent(id.clone()))?;
        let binding = |id: &Key, reason| ProjectionError::Binding {
            id: id.clone(),
            reason,
        };
        let node = |id: &Key| {
            graph
                .node(id)
                .ok_or_else(|| binding(id, "resolved content endpoint missing"))
        };
        let site = self
            .content
            .site_for(id)
            .ok_or_else(|| binding(&content.site, "bound home site missing"))?;
        let subject = super::subjects::resolve(&self.content, content)?;
        let publisher_principal = self
            .content
            .compilation()
            .workspace
            .principal(&site.publisher)
            .ok_or_else(|| binding(&site.publisher, "bound publisher principal missing"))?;
        let attributions = content
            .attributions
            .iter()
            .map(|attribution| {
                let principal = self
                    .content
                    .compilation()
                    .workspace
                    .principal(&attribution.principal)
                    .ok_or_else(|| binding(&attribution.principal, "bound principal missing"))?;
                Ok(AttributionView {
                    principal,
                    website: self.content.principal_website(principal, &content.site),
                    role: attribution.role,
                })
            })
            .collect::<Result<_, ProjectionError>>()?;
        let roles = site
            .roles
            .iter()
            .map(|assignment| {
                let principal = self
                    .content
                    .compilation()
                    .workspace
                    .principal(&assignment.principal)
                    .ok_or_else(|| {
                        binding(&assignment.principal, "bound site-role principal missing")
                    })?;
                Ok(PublicationRoleView {
                    principal,
                    website: self.content.principal_website(principal, &content.site),
                    role: assignment.role,
                })
            })
            .collect::<Result<_, ProjectionError>>()?;
        let file = self
            .file(id)
            .ok_or_else(|| binding(id, "bound document missing"))?;
        let body = file
            .body()
            .ok_or_else(|| binding(id, "bound Markdown body missing"))?;
        let path = self
            .paths
            .get(id)
            .ok_or_else(|| binding(id, "bound document path missing"))?;
        let source = Origin {
            path: path.clone(),
            source_sha256: file.source_sha256(),
            span: None,
        };
        let provenance = match self.source_binding(id) {
            Ok(source) => Some(source),
            Err(super::SourceBindingError::Missing(_)) => None,
            Err(error) => return Err(ProjectionError::Source(error)),
        };
        let asset_plan = self.assets_for(&site.id).map_err(ProjectionError::Asset)?;
        let route = self.content.route(id);
        let base = route
            .map(|route| url::Url::parse(&route.url))
            .transpose()
            .map_err(|_| binding(id, "resolved serving URL is invalid"))?;
        let qualify = |destination: &str, span| {
            base.as_ref()
                .map(|base| {
                    base.join(destination).map(String::from).map_err(|_| {
                        markdown::Error::LinkResolution {
                            span,
                            reason: "reference cannot be joined to the configured serving URL",
                        }
                    })
                })
                .transpose()
        };
        let document_error = |error| ProjectionError::body(path, file, error);
        let mut assets = BTreeMap::new();
        if let Some(package) = self.page_package(id) {
            for companion in package.companions() {
                if let Some(asset) = asset_plan.get(companion.path()) {
                    assets.insert(asset.url.as_str(), asset);
                }
            }
        }
        let citation = content
            .citation
            .as_ref()
            .map(|citation| {
                let mut download = |field,
                                    value: &Option<String>|
                 -> Result<
                    Option<(String, markdown::LinkDestination)>,
                    ProjectionError,
                > {
                    value
                        .as_ref()
                        .map(|destination| {
                            let resolved = self
                                .citation_reference(id, asset_plan, field, destination)
                                .map_err(ProjectionError::Citation)?;
                            if let Some(asset) = resolved.asset {
                                assets.insert(asset.url.as_str(), asset);
                            }
                            let annotation = self.link_destination(
                                content,
                                destination,
                                &resolved.destination,
                                Some(&resolved),
                            );
                            Ok((resolved.destination, annotation))
                        })
                        .transpose()
                };
                let (bibtex, bibtex_destination) = download("bibtex", &citation.bibtex)?
                    .map_or((None, None), |(url, destination)| {
                        (Some(url), Some(destination))
                    });
                let (pdf, pdf_destination) = download("pdf", &citation.pdf)?
                    .map_or((None, None), |(url, destination)| {
                        (Some(url), Some(destination))
                    });
                Ok::<_, ProjectionError>(super::CitationView {
                    text: citation.text.as_deref(),
                    formats: &citation.formats,
                    bibtex,
                    bibtex_destination,
                    pdf,
                    pdf_destination,
                })
            })
            .transpose()?;
        let mut collections = BTreeMap::new();
        let mut journeys = Vec::new();
        let mut steps = Vec::new();
        let mut children = Vec::new();
        let mut navigation = NavigationBuilder::new(&self.content, self.projection_limits);
        match &content.structure {
            ContentKind::Page { .. } => {
                if let Some(parent) = graph.editorial_parent(id)
                    && eligible(&parent.id)
                {
                    match &parent.structure {
                        ContentKind::Collection { .. } => {
                            collections.insert(&parent.id, parent);
                        }
                        ContentKind::Journey { members, .. }
                            if members.iter().all(|member| eligible(&member.content)) =>
                        {
                            let position = members
                                .iter()
                                .position(|member| &member.content == id)
                                .ok_or_else(|| {
                                    binding(id, "resolved journey residence missing its member")
                                })?;
                            journeys.push(navigation.journey(parent, members, position)?);
                        }
                        _ => (),
                    }
                }
            }
            ContentKind::Collection { parent, .. } => {
                if let Some(parent) = parent
                    && eligible(parent)
                {
                    collections.insert(parent, node(parent)?);
                }
                children = graph
                    .editorial_children(id)
                    .into_iter()
                    .filter(|child| eligible(&child.id))
                    .collect();
            }
            ContentKind::Journey { collection, .. } => {
                if let Some(collection) = collection
                    && eligible(collection)
                {
                    collections.insert(collection, node(collection)?);
                }
                steps = graph
                    .journey_view(id)
                    .ok_or_else(|| binding(id, "resolved journey members missing"))?
                    .steps;
            }
        }
        let collections: Vec<_> = collections.into_values().collect();
        let mut collection_navigation = Vec::new();
        if matches!(content.structure, ContentKind::Page { .. }) {
            for collection in &collections {
                let members = graph
                    .editorial_children(&collection.id)
                    .into_iter()
                    .filter(|child| eligible(&child.id))
                    .map(|child| navigation.link(child))
                    .collect::<Result<_, _>>()?;
                collection_navigation.push(CollectionNavigation {
                    collection: navigation.link(collection)?,
                    members,
                });
            }
        }
        let breadcrumbs = navigation.breadcrumbs(content, &collections, &journeys, discovery)?;
        let documentation_navigation = navigation.documentation(content, discovery)?;
        // Structural card presentation follows only the validated residence chain.
        // It neither changes the child identity nor follows editorial related links.
        let mut artwork_visits = self.projection_limits.edges;
        let mut artwork_records = self.projection_limits.edges;
        let mut artwork_bytes = self.projection_limits.text_bytes;
        let mut artwork_for = |node: &gazette_core::publication::content::ContentNode| {
            let mut artwork = None;
            for ancestor in graph.editorial_ancestry(&node.id).into_iter().flatten() {
                if !eligible(&ancestor.id) {
                    break;
                }
                artwork_visits = artwork_visits
                    .checked_sub(1)
                    .ok_or(ProjectionError::Limit("structural artwork ancestry visits"))?;
                artwork = self
                    .project_listing_artwork(&ancestor.id, &mut artwork_records, &mut artwork_bytes)
                    .map_err(ProjectionError::Listing)?;
                if artwork.is_some() {
                    break;
                }
            }
            Ok::<_, ProjectionError>(artwork)
        };
        let mut step_cards = Vec::with_capacity(steps.len());
        for node in steps {
            step_cards.push(navigation.view_with_artwork(node, artwork_for(node)?)?);
        }
        let mut child_cards = Vec::with_capacity(children.len());
        for node in children {
            child_cards.push(navigation.view_with_artwork(node, artwork_for(node)?)?);
        }
        let host = gazette_core::markdown::ComponentHost {
            content: content.id.clone(),
            site: content.site.clone(),
            surface: (&content.structure).into(),
            project_resources: self.project_resources_for(id)?,
        };
        let markdown_alternate = self
            .content
            .compilation()
            .workspace
            .publication_for(&content.site)
            .and_then(|publication| publication.machine.markdown_alternates.as_ref())
            .is_some_and(|alternate| alternate.enabled)
            && content
                .route
                .as_ref()
                .is_some_and(|route| route.canonical.is_none());
        let container_contents = if components
            .is_some_and(|selected| body.requests_container_contents(selected.definitions))
            && matches!(
                content.structure,
                ContentKind::Collection { .. } | ContentKind::Journey { .. }
            ) {
            let reading_discovery = markdown_alternate
                .then(|| self.discovery())
                .transpose()
                .map_err(ProjectionError::Source)?;
            Some(super::container_contents::prepare(
                super::container_contents::Container {
                    content,
                    entries: if matches!(content.structure, ContentKind::Journey { .. }) {
                        &step_cards
                    } else {
                        &child_cards
                    },
                    documentation: graph.is_documentation(id),
                    navigation: documentation_navigation.as_ref(),
                    discovery,
                },
                reading_discovery.as_ref(),
                self.projection_limits.text_bytes,
            )?)
        } else {
            None
        };
        let image_presentations = self
            .project_body_images(id, asset_plan, &mut assets, dimensions)
            .map_err(document_error)?;
        let mut rendered = body
            .render_projection(
                markdown::Projection {
                    container_contents: container_contents.as_ref(),
                    namespace: "",
                    heading_offset: 0,
                    images: &BTreeMap::new(),
                    components,
                    host: Some(&host),
                    markdown_alternate,
                },
                |form, kind, destination, span| {
                    // Validate the original source coordinate before URL normalization
                    // can erase an encoded traversal or separator. Both body and excerpt
                    // reuse this one mapped destination and exact asset obligation.
                    let resolved =
                        self.resolve_reference(id, asset_plan, form, kind, destination, span)?;
                    if let Some(asset) = resolved.as_ref().and_then(|reference| reference.asset) {
                        assets.insert(asset.url.as_str(), asset);
                    }
                    let mapped = resolved
                        .as_ref()
                        .map_or(destination, |reference| reference.destination.as_str());
                    let mapped = qualify(mapped, span)?.or_else(|| {
                        resolved
                            .as_ref()
                            .map(|reference| reference.destination.clone())
                    });
                    let effective = mapped.as_deref().unwrap_or(destination);
                    let annotation = (kind == markdown::LinkKind::Link).then(|| {
                        self.link_destination(content, destination, effective, resolved.as_ref())
                    });
                    let image = (kind == markdown::LinkKind::Image
                        && resolved
                            .as_ref()
                            .is_some_and(|reference| reference.resource))
                    .then(|| {
                        super::body_images::presentation_for(
                            &image_presentations,
                            resolved.as_ref(),
                            dimensions,
                            raster,
                        )
                    })
                    .flatten();
                    Ok(markdown::ResolvedLink {
                        image,
                        destination: mapped,
                        annotation,
                    })
                },
            )
            .map_err(document_error)?;
        if rendered.opening.is_some_and(|opening| {
            opening.heading_end == 0 || !rendered.body_html.is_char_boundary(opening.heading_end)
        }) {
            return Err(document_error(markdown::Error::InvalidOutput));
        }
        if let Some(summary) = &mut rendered.summary
            && let Some(continuation) =
                qualify(&summary.continuation, summary.marker).map_err(document_error)?
        {
            // The canonical route comes from bounded configuration; the complete
            // projected value remains subject to the consumer's context/output cap.
            summary.continuation = continuation;
        }
        let headings = body.headings().map_err(document_error)?;
        let related: Vec<&ContentNode> = graph
            .related()
            .iter()
            .filter(|edge| &edge.source == id)
            .filter(|edge| eligible(&edge.target))
            .map(|edge| node(&edge.target))
            .collect::<Result<_, _>>()?;
        let public_project_identity = |project: &Project| {
            let landing = project.resources.as_ref()?.landing.as_ref()?;
            if project.visibility != gazette_core::publication::Visibility::Public
                || discovery.is_some_and(|selected| !selected.contains(landing))
            {
                return None;
            }
            Some(super::public_identity::project(
                project,
                Some(self.content.route(landing)?),
            ))
        };
        let source_credit = |id: &Key| {
            let principal = self.content.compilation().workspace.principal(id)?;
            Some((
                principal,
                self.content.principal_website(principal, &content.site),
            ))
        };
        for item in rendered
            .components
            .iter_mut()
            .filter_map(|component| component.catalogue.as_mut())
            .flatten()
        {
            let Some(destination) = item.destination.as_ref() else {
                continue;
            };
            if discovery.is_some_and(|selected| !selected.contains(destination)) {
                continue;
            }
            let (Some(node), Some(route)) =
                (graph.node(destination), self.content.route(destination))
            else {
                continue;
            };
            if node.lifecycle != gazette_core::publication::ContentLifecycle::Published {
                continue;
            }
            let project = self.content.projects().find(|project| {
                project.visibility == gazette_core::publication::Visibility::Public
                    && project
                        .resources
                        .as_ref()
                        .and_then(|resources| resources.landing.as_ref())
                        == Some(destination)
            });
            let (identity, kind) = match project {
                Some(project) => (
                    super::public_identity::project(project, Some(route)),
                    super::public_identity::project_type(project.kind),
                ),
                None => (
                    super::public_identity::work(node, Some(route), false),
                    super::public_identity::work_type(node, false, self.content.is_listing(node)),
                ),
            };
            item.identity = Some(identity);
            item.kind = Some(kind);
            item.software = project.and_then(|project| project.software.clone());
        }
        let projects = graph
            .project_relations(id)
            .into_iter()
            .flat_map(|links| links.iter())
            .map(|(id, relationship)| {
                let project = self
                    .content
                    .project(id)
                    .ok_or_else(|| binding(id, "bound project missing"))?;
                Ok(ProjectView {
                    identity: super::public_identity::project(
                        project,
                        project
                            .resources
                            .as_ref()
                            .and_then(|resources| resources.landing.as_ref())
                            .and_then(|id| self.content.route(id)),
                    ),
                    works: self.project_works(project, discovery)?,
                    code_documentation: None,
                    project,
                    description: project
                        .resources
                        .as_ref()
                        .and_then(|resources| resources.landing.as_ref())
                        .and_then(|id| graph.node(id))
                        .and_then(|node| node.description.as_deref()),
                    programme: project
                        .programme
                        .as_ref()
                        .and_then(|id| self.content.project(id))
                        .and_then(public_project_identity),
                    programme_members: if project.kind
                        == Some(gazette_core::publication::ProjectKind::ResearchProject)
                    {
                        self.content
                            .projects()
                            .filter(|member| member.programme.as_ref() == Some(&project.id))
                            .filter_map(public_project_identity)
                            .collect()
                    } else {
                        Vec::new()
                    },
                    portfolio_brands: super::brand_graph::portfolio_brands(self.content(), project),
                    research_brand: super::brand_graph::research_brand(self.content(), project),
                    source_author: project
                        .source
                        .as_ref()
                        .and_then(|source| source_credit(&source.author)),
                    source_rights_holder: project
                        .source
                        .as_ref()
                        .and_then(|source| source_credit(&source.rights_holder)),
                    relationship,
                    landing: self.project_landing_for(project, content)?,
                    resources: self.resources_for_project(project, content)?,
                })
            })
            .collect::<Result<_, ProjectionError>>()?;
        let project_declarations = graph
            .declared_projects()
            .iter()
            .filter(|declaration| &declaration.content == id)
            .map(|declaration| {
                let project = self.content.project(&declaration.project).ok_or_else(|| {
                    binding(&declaration.project, "declared project binding missing")
                })?;
                Ok(ProjectDeclarationView {
                    project,
                    declaration,
                })
            })
            .collect::<Result<_, ProjectionError>>()?;
        let body_text = std::mem::take(&mut rendered.body_text);
        if rendered
            .body_markdown
            .as_ref()
            .is_some_and(|body| body.len() > self.projection_limits.text_bytes)
        {
            return Err(ProjectionError::Limit("page Markdown bytes"));
        }
        let mut notes = None;
        if let Some((projection, dependencies)) = self.project_notes(
            content,
            asset_plan,
            components,
            markdown_alternate,
            dimensions,
            raster,
        )? {
            for asset in dependencies {
                assets.insert(asset.url.as_str(), asset);
            }
            notes = Some(projection);
        }
        if notes.is_some() {
            for prefix in [
                super::notes::NAMESPACE.to_owned(),
                format!("fn-{}", super::notes::NAMESPACE),
                format!("fnref-{}", super::notes::NAMESPACE),
            ] {
                if body.has_anchor_prefix(&prefix).map_err(document_error)? {
                    return Err(binding(
                        id,
                        "primary body anchor conflicts with its notes namespace",
                    ));
                }
            }
        }
        let note_bytes = notes
            .as_ref()
            .map_or(0, |notes| notes.body_html.len().max(notes.body_text.len()));
        let maximum = self
            .projection_limits
            .text_bytes
            .checked_sub(note_bytes)
            .ok_or(ProjectionError::Limit("page and notes output bytes"))?;
        if rendered.body_html.len().max(body_text.len()) > maximum {
            return Err(ProjectionError::Limit("page and notes output bytes"));
        }
        // Declared notebooks join the Notes after any `notes.md`, so the essay's
        // body, outline and primary text hold only the essay.
        let notebooks = self.project_notebooks(
            &super::notebooks::NotebookContext {
                content,
                asset_plan,
                image_presentations: &image_presentations,
                dimensions,
                raster,
                markdown,
                components,
                host: &host,
            },
            |namespace| body.has_anchor_prefix(namespace).map_err(document_error),
            &mut notes,
            &mut assets,
            maximum.saturating_sub(rendered.body_html.len().max(body_text.len())),
            rendered.body_markdown.as_ref().map(String::len),
        )?;
        if let Some(alternate) = &mut rendered.body_markdown
            && let Some(notes) = &notes
        {
            let commentary = notes
                .body_markdown
                .as_deref()
                .ok_or_else(|| binding(id, "selected notes Markdown representation is missing"))?;
            let opening = "\n\n## Article notes\n\n";
            let closing = "\n";
            if alternate
                .len()
                .saturating_add(opening.len())
                .saturating_add(commentary.len())
                .saturating_add(closing.len())
                > self.projection_limits.text_bytes
            {
                return Err(ProjectionError::Limit("page and notes Markdown bytes"));
            }
            alternate.push_str(opening);
            alternate.push_str(commentary);
            alternate.push_str(closing);
        }
        let page = PageProjection {
            opening: rendered.opening,
            body_markdown: rendered.body_markdown,
            notebooks,
            body_html: rendered.body_html,
            body_text,
            summary: rendered.summary,
            data: PageData {
                brand_graph: super::brand_graph::graph(self.content(), &site.id, id),
                container_contents_placed: rendered.container_contents_placed,
                invitations: self.page_invitations(content)?,
                preferred_source: self
                    .content
                    .compilation()
                    .workspace
                    .publication_for(&content.site)
                    .and_then(|publication| publication.processing.as_ref())
                    .and_then(|processing| processing.preferred_sources.as_ref())
                    .zip(
                        self.content
                            .compilation()
                            .workspace
                            .public_base_url(&content.site),
                    )
                    .and_then(|(selection, base)| selection.project(content, base)),
                project_resources: self.project_resources_for(id)?,
                citation,
                legal_processing: provenance
                    .as_ref()
                    .filter(|source| source.legal_binding().is_some())
                    .and_then(|source| {
                        self.content
                            .compilation()
                            .workspace
                            .publication_for(source.site)
                    })
                    .and_then(|publication| publication.processing.as_ref())
                    .map(super::LegalProcessingContext::from),
                documentation_mount: self.documentation_mount_view(id)?,
                listing: if self
                    .content
                    .compilation()
                    .workspace
                    .publication_for(&content.site)
                    .is_some_and(|publication| {
                        publication
                            .listings
                            .iter()
                            .any(|listing| &listing.content == id)
                    }) {
                    let native_discovery = self.discovery().map_err(ProjectionError::Source)?;
                    self.project_listing_pages(
                        &content.site,
                        discovery.unwrap_or(&native_discovery),
                    )
                    .map_err(ProjectionError::Listing)?
                    .into_iter()
                    .find(|page| &page.content == id && page.is_primary())
                } else {
                    None
                },
                notes,
                components: rendered.components,
                links: rendered.links,
                classifications: self
                    .classification_views(content)
                    .map_err(super::classifications::ClassificationError::projection)?,
                content: ContentView::new(
                    content,
                    self.content.presentation_for(&content.site),
                    route,
                    graph.is_documentation(&content.id),
                    self.content.is_listing(content),
                ),
                route,
                site: SiteView {
                    metadata: self.publication_metadata(&site.id).map_err(|error| {
                        ProjectionError::Binding {
                            id: error.id,
                            reason: error.reason,
                        }
                    })?,
                    id: &site.id,
                    publisher: &site.publisher,
                    publisher_principal,
                    publisher_website: self
                        .content
                        .principal_website(publisher_principal, &site.id),
                    rights_holder: site.rights_holder.as_ref(),
                    default_author: site.default_author.as_ref(),
                    roles,
                    presentation: self
                        .content
                        .presentation_for(&site.id)
                        .map(SitePresentation::from),
                    terms_url: self
                        .content
                        .compilation()
                        .workspace
                        .publication_for(&site.id)
                        .and_then(|publication| publication.terms_url()),
                },
                attributions,
                subject,
                source,
                discussion: super::discussion::resolve(&self.content, content, provenance.as_ref()),
                provenance,
                derivation: self.derivation(id),
                headings,
                assets: assets.into_values().collect(),
                images: rendered.images,
                collections: collections
                    .into_iter()
                    .map(|node| navigation.view(node))
                    .collect::<Result<_, _>>()?,
                journeys,
                collection_navigation,
                documentation_navigation,
                breadcrumbs,
                steps: step_cards,
                children: child_cards,
                related: related
                    .into_iter()
                    .map(|node| navigation.view(node))
                    .collect::<Result<_, _>>()?,
                recommendations: self.recommendations(content, &mut navigation)?,
                projects,
                project_declarations,
            },
        };
        if let Some(discovery) = discovery {
            discovery
                .validate_page_projects(&page.data, false)
                .map_err(ProjectionError::ProjectOrigin)?;
        }
        Ok(page)
    }
}
