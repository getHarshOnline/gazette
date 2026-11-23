//! Site-scoped structural projection; no registry reads, ranking or public admission.

use std::{collections::BTreeMap, fmt};

use gazette_core::{
    publication::{
        Key, Principal, Project,
        content::{self, ContentKind, ContentNode, ProjectLink},
    },
    render::LimitedWriter,
};
use serde::Serialize;

use super::{Discovery, ResolvedAuthored, SitePresentation, SourceBindingError};
use crate::workspace::{ContentSource, ResolvedRoute};

mod discovery;
pub use discovery::DiscoveryGraph;

/// A foreign endpoint is a reference, not permission to export its record.
#[derive(Debug, Serialize)]
pub struct ForeignContent<'a> {
    /// Exact content identity.
    pub id: &'a Key,
    /// Exact borrowed display label; not a copied foreign page or profile.
    pub title: &'a str,
    /// Original publication home; an incident edge never relocates it.
    pub site: &'a Key,
    /// Existing compiled address, if explicitly declared; no slug fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<&'a str>,
    /// Destination Theme/palette identity from the exact configured home site.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
}

/// Explicit container parent, separate from leaf membership and editorial links.
#[derive(Debug, Serialize)]
pub struct ContainerLink<'a> {
    /// Collection or journey declaring the parent.
    pub child: &'a Key,
    /// Declared containing collection.
    pub parent: &'a Key,
}

/// A journey's original position, not the position of a filtered foreign subset.
#[derive(Debug, Serialize)]
pub struct JourneyStep<'a> {
    /// Exact traversal identity.
    pub journey: &'a Key,
    /// Exact member identity.
    pub content: &'a Key,
    /// Zero-based position in the complete declared journey.
    pub position: usize,
    /// Explicit primary traversal selection for this member.
    pub primary: bool,
}

/// One bounded, local, draft-inclusive graph shared by Theme and JSON consumers.
///
/// Local objects are complete. Foreign endpoints contain only references, and
/// their unrelated edges are not recursively exported. This is not discovery
/// eligibility, admitted public knowledge, or an authorization packet.
#[derive(Debug, Serialize)]
pub struct SiteGraph<'a> {
    /// Generated reading views over existing index identities, not additional content nodes.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub listings: Vec<super::ListingPage>,
    /// Local projection version, not a frozen external release ABI.
    pub format: &'static str,
    /// Exact selected site.
    pub site: &'a Key,
    /// Independently configured role; never a fallback author.
    pub publisher: &'a Key,
    /// Explicit site rights holder; content rights remain independently declared.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rights_holder: Option<&'a Key>,
    /// Configured default-person selection, not a derived authorship relationship.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_author: Option<&'a Key>,
    /// Exact normalized site responsibilities, without publisher or sibling inference.
    pub roles: &'a [gazette_core::publication::PublicationRoleBinding],
    /// All content owned by this site, preserving every lifecycle and structure.
    pub nodes: BTreeMap<&'a Key, &'a ContentNode>,
    /// Foreign endpoints needed by the retained relationships only.
    pub references: BTreeMap<&'a Key, ForeignContent<'a>>,
    /// The same compiled routes used by page rendering, for local nodes only.
    pub routes: BTreeMap<&'a Key, &'a ResolvedRoute>,
    /// Leaf memberships incident to this site; not ordered journey membership.
    pub memberships: Vec<&'a content::CollectionMembership>,
    /// Explicit outgoing editorial associations of local objects only.
    pub related: Vec<&'a content::EditorialRelation>,
    /// Explicit page-owned directory paths, never additional editorial parents.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub browsing: Vec<&'a content::BrowsingRelation>,
    /// Parent relationships incident to this site's containers.
    pub containers: Vec<ContainerLink<'a>>,
    /// Incident journey membership, retaining original positions and primary flags.
    pub journeys: Vec<JourneyStep<'a>>,
    /// Already resolved local page/project assertions and complete declaration evidence.
    pub project_links: BTreeMap<&'a Key, &'a BTreeMap<Key, ProjectLink>>,
    /// Original local declarations, distinct from inherited final assertions.
    pub declarations: Vec<&'a content::ProjectDeclaration>,
    /// Only projects referenced by local declarations, assertions or native provenance.
    pub projects: BTreeMap<&'a Key, &'a Project>,
    /// This site's explicit principal selections, role holders and direct content attributions.
    pub principals: BTreeMap<&'a Key, &'a Principal>,
    /// Native source context, never promoted into about, membership or authorship.
    pub provenance: BTreeMap<&'a Key, ContentSource<'a>>,
    /// The same scoped classification views used by local page consumers.
    pub classifications: BTreeMap<&'a Key, Vec<super::ClassificationView<'a>>>,
}

/// No partial site graph escapes a missing binding or exhausted output budget.
#[derive(Debug)]
pub enum SiteGraphError {
    /// A configured index cannot provide its complete ordered reading views.
    Listing(super::ListingError),
    /// Complete emitted lineage would expose a nonpublic catalog endpoint.
    ProjectOrigin(super::ProjectOriginError),
    /// Discovery eligibility belongs to a different resolved content owner.
    Discovery,
    /// Exact requested site does not exist.
    Site(Key),
    /// A previously resolved reference is missing its native owner.
    Binding(Key),
    /// Native content source/home validation failed.
    Source(SourceBindingError),
    /// Complete projection exceeds a caller-selected ceiling.
    Limit(&'static str),
    /// Bounded serialization rejected the projection.
    Encoding(serde_json::Error),
}

impl SiteGraphError {
    /// Stable class for SDK and executable diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Listing(error) => error.code(),
            Self::ProjectOrigin(error) => error.code(),
            Self::Discovery => "graph_discovery",
            Self::Site(_) => "graph_site",
            Self::Binding(_) => "graph_binding",
            Self::Source(error) => error.code(),
            Self::Limit(_) => "graph_limit",
            Self::Encoding(_) => "graph_encoding",
        }
    }
}

impl fmt::Display for SiteGraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Listing(error) => error.fmt(f),
            Self::ProjectOrigin(error) => error.fmt(f),
            Self::Discovery => f.write_str("discovery selection belongs to another resolved owner"),
            Self::Site(id) => write!(f, "unknown graph site {id}; select a configured site"),
            Self::Binding(id) => write!(f, "graph reference {id} is unresolved; no graph produced"),
            Self::Source(error) => error.fmt(f),
            Self::Limit(unit) => write!(
                f,
                "complete site graph exceeds {unit}; no relationships truncated"
            ),
            Self::Encoding(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SiteGraphError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Listing(error) => Some(error),
            Self::ProjectOrigin(error) => Some(error),
            Self::Source(error) => Some(error),
            Self::Encoding(error) => Some(error),
            _ => None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Project the selected site's complete structure from the retained owners.
    ///
    /// Draft and archived objects remain present. Incident cross-site structural
    /// edges retain foreign identities and borrowed title labels without copying pages or profiles.
    /// No native I/O, policy inference, graph ranking or lifecycle filtering occurs.
    ///
    /// # Errors
    /// Rejects unknown sites, invalid source bindings and complete node/edge/encoded
    /// byte excess. Limits never select a smaller successful graph.
    pub fn project_site_graph(
        &self,
        site: &Key,
        limits: content::Limits,
    ) -> Result<SiteGraph<'_>, SiteGraphError> {
        let result = self.project_graph(site, limits, None)?;
        check_encoding(&result, limits)?;
        Ok(result)
    }

    /// Project emitted discovery from the same graph traversal, preserving canonical meaning.
    /// Hidden endpoints and whole assertions with hidden evidence are not listed. Journey
    /// positions remain the original positions; this projection is not a shortened journey.
    ///
    /// # Errors
    /// Rejects a discovery view from another owner and all existing graph binding/budget failures.
    pub fn project_discovery_graph<'a>(
        &'a self,
        site: &Key,
        limits: content::Limits,
        discovery: &'a Discovery<'a>,
    ) -> Result<DiscoveryGraph<'a>, SiteGraphError> {
        if !discovery.matches(self) {
            return Err(SiteGraphError::Discovery);
        }
        let result = DiscoveryGraph {
            graph: self.project_graph(site, limits, Some(discovery))?,
            discovery,
        };
        check_encoding(&result, limits)?;
        Ok(result)
    }

    fn project_graph(
        &self,
        site: &Key,
        limits: content::Limits,
        discovery: Option<&Discovery<'_>>,
    ) -> Result<SiteGraph<'_>, SiteGraphError> {
        let eligible = |id: &Key| discovery.is_none_or(|view| view.contains(id));
        let content = self.content();
        let graph = content.graph();
        let workspace = &content.compilation().workspace;
        let site = workspace
            .site(site)
            .ok_or_else(|| SiteGraphError::Site(site.clone()))?;
        let mut result = SiteGraph {
            listings: if workspace
                .publication_for(&site.id)
                .is_some_and(|publication| !publication.listings.is_empty())
            {
                let native_discovery = self.discovery().map_err(SiteGraphError::Source)?;
                let pages = self
                    .project_listing_pages(&site.id, discovery.unwrap_or(&native_discovery))
                    .map_err(SiteGraphError::Listing)?;
                if pages.len() > limits.nodes
                    || pages
                        .iter()
                        .try_fold(0usize, |total, page| total.checked_add(page.items.len()))
                        .is_none_or(|total| total > limits.edges)
                {
                    return Err(SiteGraphError::Limit("listing views or references"));
                }
                pages
                    .into_iter()
                    .filter(|page| eligible(&page.content))
                    .collect()
            } else {
                Vec::new()
            },
            format: "gazette-local-site-graph/1",
            site: &site.id,
            publisher: &site.publisher,
            rights_holder: site.rights_holder.as_ref(),
            default_author: site.default_author.as_ref(),
            roles: &site.roles,
            nodes: BTreeMap::new(),
            references: BTreeMap::new(),
            routes: BTreeMap::new(),
            memberships: Vec::new(),
            related: Vec::new(),
            browsing: Vec::new(),
            containers: Vec::new(),
            journeys: Vec::new(),
            project_links: BTreeMap::new(),
            declarations: Vec::new(),
            projects: BTreeMap::new(),
            principals: BTreeMap::new(),
            provenance: BTreeMap::new(),
            classifications: BTreeMap::new(),
        };
        let mut classification_bytes = limits.text_bytes;
        let mut budget = Budget {
            limits,
            nodes: 0,
            edges: 0,
        };
        for node in graph
            .nodes()
            .filter(|node| node.site == site.id && eligible(&node.id))
        {
            budget.node()?;
            result.nodes.insert(&node.id, node);
            if let Some(route) = content.route(&node.id) {
                result.routes.insert(&node.id, route);
            }
        }
        let mut endpoints = Vec::new();
        let mut project_ids = Vec::new();
        let mut principal_ids = vec![&site.publisher];
        for selected in [site.rights_holder.as_ref(), site.default_author.as_ref()]
            .into_iter()
            .flatten()
        {
            budget.edge()?;
            principal_ids.push(selected);
        }
        for binding in &site.roles {
            budget.edge()?;
            principal_ids.push(&binding.principal);
        }
        for node in graph.nodes() {
            if !eligible(&node.id) {
                continue;
            }
            let local = result.nodes.contains_key(&node.id);
            let parent = match &node.structure {
                ContentKind::Collection { parent, .. } => parent.as_ref(),
                ContentKind::Journey {
                    collection,
                    members,
                } => {
                    for (position, member) in members.iter().enumerate() {
                        if eligible(&member.content)
                            && (local || result.nodes.contains_key(&member.content))
                        {
                            budget.edge()?;
                            result.journeys.push(JourneyStep {
                                journey: &node.id,
                                content: &member.content,
                                position,
                                primary: member.primary,
                            });
                            endpoints.extend([&node.id, &member.content]);
                        }
                    }
                    collection.as_ref()
                }
                ContentKind::Page { .. } => None,
            };
            if let Some(parent) = parent
                && eligible(parent)
                && (local || result.nodes.contains_key(parent))
            {
                budget.edge()?;
                result.containers.push(ContainerLink {
                    child: &node.id,
                    parent,
                });
                endpoints.extend([&node.id, parent]);
            }
            if !local {
                continue;
            }
            if let Some(canonical) = node
                .route
                .as_ref()
                .and_then(|route| route.canonical.as_ref())
                .filter(|canonical| eligible(canonical))
            {
                budget.edge()?;
                endpoints.push(canonical);
            }
            for _ in &node.taxonomies {
                budget.edge()?;
                // Each view emits one taxonomy and term record; count repeated
                // occurrences too, before expanding the borrowed output vector.
                budget.node()?;
                budget.node()?;
            }
            result.classifications.insert(
                &node.id,
                self.classification_views_from(node, &node.site, &mut classification_bytes)
                    .map_err(super::classifications::ClassificationError::graph)?,
            );
            for attribution in &node.attributions {
                budget.edge()?;
                principal_ids.push(&attribution.principal);
            }
            if let Some(links) = graph.project_relations(&node.id) {
                for (project, link) in links
                    .iter()
                    .filter(|(_, link)| link_visible(link, discovery))
                {
                    budget.edge()?;
                    project_ids.push(project);
                    for evidence in link.evidence() {
                        budget.edge()?;
                        endpoints.push(&evidence.source);
                    }
                }
                // The discovery serializer borrows the original complete assertions, omitting
                // any assertion whose full evidence cannot be listed. It never trims evidence.
                result.project_links.insert(&node.id, links);
            }
            match self.source_binding(&node.id) {
                Ok(source) => {
                    budget.edge()?;
                    if let Some(project) = source.project {
                        project_ids.push(project);
                    }
                    result.provenance.insert(&node.id, source);
                }
                Err(SourceBindingError::Missing(_)) if discovery.is_none() => {}
                Err(error) => return Err(SiteGraphError::Source(error)),
            }
        }
        for edge in graph.memberships() {
            if eligible(&edge.content)
                && eligible(&edge.collection)
                && (result.nodes.contains_key(&edge.content)
                    || result.nodes.contains_key(&edge.collection))
            {
                budget.edge()?;
                endpoints.extend([&edge.content, &edge.collection]);
                result.memberships.push(edge);
            }
        }
        for edge in graph
            .related()
            .iter()
            .filter(|edge| result.nodes.contains_key(&edge.source) && eligible(&edge.target))
        {
            budget.edge()?;
            endpoints.push(&edge.target);
            result.related.push(edge);
        }
        for edge in graph
            .browsing()
            .iter()
            .filter(|edge| result.nodes.contains_key(&edge.source) && eligible(&edge.index))
        {
            budget.edge()?;
            endpoints.push(&edge.index);
            result.browsing.push(edge);
        }
        for declaration in graph
            .declared_projects()
            .iter()
            .filter(|declaration| result.nodes.contains_key(&declaration.content))
        {
            budget.edge()?;
            project_ids.push(&declaration.project);
            result.declarations.push(declaration);
        }
        for id in endpoints {
            if result.nodes.contains_key(id) || result.references.contains_key(id) {
                continue;
            }
            let node = graph
                .node(id)
                .ok_or_else(|| SiteGraphError::Binding(id.clone()))?;
            budget.node()?;
            result.references.insert(
                &node.id,
                ForeignContent {
                    id: &node.id,
                    title: &node.title,
                    site: &node.site,
                    canonical_url: content
                        .route(&node.id)
                        .map(|route| route.canonical_url.as_str()),
                    presentation: content
                        .presentation_for(&node.site)
                        .map(SitePresentation::from),
                },
            );
        }
        for id in project_ids {
            if result.projects.contains_key(id) {
                continue;
            }
            let project = content
                .project(id)
                .ok_or_else(|| SiteGraphError::Binding(id.clone()))?;
            if let Some(discovery) = discovery {
                discovery
                    .validate_project_origin(project)
                    .map_err(SiteGraphError::ProjectOrigin)?;
            }
            if project
                .origin
                .as_ref()
                .and_then(|origin| origin.upstream())
                .is_some()
            {
                budget.edge()?;
            }
            budget.node()?;
            result.projects.insert(&project.id, project);
        }
        for id in principal_ids {
            if result.principals.contains_key(id) {
                continue;
            }
            let principal = workspace
                .principal(id)
                .ok_or_else(|| SiteGraphError::Binding(id.clone()))?;
            budget.node()?;
            result.principals.insert(&principal.id, principal);
        }
        Ok(result)
    }
}

fn link_visible(link: &ProjectLink, discovery: Option<&Discovery<'_>>) -> bool {
    discovery.is_none_or(|view| {
        link.evidence()
            .iter()
            .all(|evidence| view.contains(&evidence.source))
    })
}

fn check_encoding(value: &impl Serialize, limits: content::Limits) -> Result<(), SiteGraphError> {
    let mut sink = LimitedWriter::new(std::io::sink(), limits.text_bytes);
    let encoded = serde_json::to_writer(&mut sink, value);
    if sink.exceeded() {
        return Err(SiteGraphError::Limit("encoded bytes"));
    }
    encoded.map_err(SiteGraphError::Encoding)
}

struct Budget {
    limits: content::Limits,
    nodes: usize,
    edges: usize,
}
impl Budget {
    fn node(&mut self) -> Result<(), SiteGraphError> {
        charge(&mut self.nodes, self.limits.nodes, "nodes")
    }
    fn edge(&mut self) -> Result<(), SiteGraphError> {
        charge(&mut self.edges, self.limits.edges, "edges")
    }
}
fn charge(value: &mut usize, limit: usize, unit: &'static str) -> Result<(), SiteGraphError> {
    *value = value
        .checked_add(1)
        .filter(|value| *value <= limit)
        .ok_or(SiteGraphError::Limit(unit))?;
    Ok(())
}
