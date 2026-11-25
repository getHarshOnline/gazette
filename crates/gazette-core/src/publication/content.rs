//! The structure of a publication's content: pages, the collections that develop one
//! question, the journeys that set the order an argument needs, and how they relate.
//!
//! This module builds on the shared keys and lifecycle values beside it. The compiler,
//! application and renderers build on it, as typed relationships before policy
//! admission, rendering or persistence. This internal model does not replace authored
//! frontmatter or freeze a wire ABI. One resolved structure supplies queries; SQL and
//! renderers must not rebuild it.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use serde::{Deserialize, Serialize};

use super::{ContentLifecycle, Key};

mod attribution;
mod browsing;
mod citation;
pub use citation::{Citation, CitationFormat};
mod date;
mod license;
mod navigation;
mod order;
mod projects;
mod purpose;
mod resolve;
mod selection;
mod subject;
pub use attribution::{Attribution, AttributionRole};
pub use browsing::BrowsingRelation;
pub use date::{EditorialDate, EditorialDateError};
pub use license::{ContentLicense, ContentLicenseError};
pub use navigation::NavigationItem;
pub use projects::{
    ProjectDeclaration, ProjectDeclarationKind, ProjectEvidence, ProjectLink, ProjectRelation,
};
pub use purpose::CollectionPurpose;
pub use selection::Disposition;
pub use subject::{ServiceSubject, Subject};

/// Generic page purpose; an extension names a separately admitted domain schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum PageFamily {
    /// A bounded authored contribution.
    Article,
    /// Authored project documentation, not generated API-reference artifacts.
    Documentation,
    /// A page about an independently identified project.
    Project,
    /// An index or landing page.
    Index,
    /// Structural, legal, about or contact content with separately selected presentation.
    Utility,
    /// Data constrained by an explicitly selected schema, not automatic extension approval.
    Extension {
        /// Exact workspace schema reference; no title, path or Theme inference.
        schema: Key,
        /// Original domain values, validated once by the workspace compiler.
        data: serde_json::Value,
        /// Optional authored presentation intent for this schema-bound page.
        ///
        /// The Engine retains these generic coordinates without assigning a
        /// `DesignSystem` world, Theme package, template or visual treatment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presentation: Option<ExtensionPresentation>,
    },
}

/// Generic authored presentation intent for one extension page.
///
/// Namespaced mode and variant keys preserve owner vocabulary without adding
/// a site-name or schema-name branch to the reusable Engine. Amplitude is a
/// bounded permission ceiling, not a command to produce a specific effect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtensionPresentation {
    /// Owner-namespaced semantic page mode.
    pub mode: Key,
    /// Bounded expressive permission ceiling, from zero through four.
    #[schemars(range(min = 0, max = 4))]
    pub amplitude: u8,
    /// Optional owner-namespaced stable treatment variant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<Key>,
}

/// The three forms content takes: a page of writing, a collection that develops one
/// question, and a journey that sets the order an argument needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ContentKind {
    /// A page is one piece of writing, the unit that collections gather and journeys
    /// order. A leaf page has at most one immediate editorial container.
    Page {
        /// Explicit purpose, not inferred from a template filename.
        family: PageFamily,
    },
    /// A collection develops one question, and a subcollection takes up a branch of it.
    /// A collection's optional structural parent is another collection.
    Collection {
        /// No parent means a root collection, not an inferred folder ancestor.
        parent: Option<Key>,
        /// Omission inherits the containing collection; an omitted root is editorial.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        purpose: Option<CollectionPurpose>,
        /// Optional complete documentation outline, independent of containment.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        navigation: Option<Vec<NavigationItem>>,
        /// Direct children shown first in this order; unlisted children follow by identity.
        /// This changes presentation order without declaring membership or a journey.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        member_order: Vec<Key>,
    },
    /// A journey sets the order an argument needs. It is an ordered traversal inside a
    /// declared collection or subcollection.
    Journey {
        /// Containing collection. Absence is rejected during whole-graph resolution.
        collection: Option<Key>,
        /// Order is meaningful and never replaced with sorting by title or key.
        members: Vec<JourneyMember>,
    },
}

/// Explicit structured-data type, independent of editorial residence or lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum SchemaType {
    /// An essay or developed inquiry.
    Article,
    /// A personal post or journal contribution.
    BlogPosting,
    /// A technical explanation, procedure or specification.
    TechArticle,
    /// A utility page whose primary subject is an explicitly named principal.
    AboutPage,
    /// The identifying profile of one person on their declared website.
    ProfilePage,
}

/// Compatibility name for callers that select article contribution types.
pub type ArticleType = SchemaType;

/// One native content identity; its state and site are never inferred from membership.
/// Authored parsing may leave the site unresolved until the source is bound;
/// the graph uses the default, complete `Key` form.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContentNode<S = Key> {
    /// Stable object identity, distinct from path, slug and display label.
    pub id: Key,
    /// Resolved home-site reference; cross-site edges do not move this home.
    pub site: S,
    /// Explicit title, used by projections instead of reverse-engineering a slug.
    pub title: String,
    /// Authored plain text, not a Markdown summary or inherited site description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Distinct authored article phrases in declaration order, not taxonomy memberships.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    /// Explicit structured-data type, checked against the page family and subject.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_type: Option<SchemaType>,
    /// Explicit primary subject, separate from authorship and project relationships.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<Subject>,
    /// Exact declared content-licence URI; not a software licence or permission proof.
    /// Absence never inherits from a repository, site, collection or rights holder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<ContentLicense>,
    /// Optional authored citation; never generated or inherited.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<Citation>,
    /// Authored publication chronology, independent of lifecycle and release authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<EditorialDate>,
    /// Authored modification chronology; absence never falls back to publication time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<EditorialDate>,
    /// Independent authored content state.
    pub lifecycle: ContentLifecycle,
    /// Prose language; the workspace resolver fills an omission from its publication.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<super::LanguageTag>,
    /// Exact classification references, distinct from project or collection relations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taxonomies: Vec<TaxonomyMembership>,
    /// Role declarations in authored order. The workspace may fill an empty list
    /// from the published content's configured default author.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributions: Vec<Attribution>,
    /// Authored or derived routing; absence has no public address until completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<super::routing::Route>,
    /// Readable address component, independent from identity, title and source filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<super::routing::RouteSlug>,
    /// Page or container semantics.
    pub structure: ContentKind,
}

impl<S> ContentNode<S> {
    /// Bind a source-selected home while carrying every content fact unchanged.
    pub fn with_site<T>(self, site: T) -> ContentNode<T> {
        ContentNode {
            id: self.id,
            site,
            title: self.title,
            description: self.description,
            keywords: self.keywords,
            schema_type: self.schema_type,
            subject: self.subject,
            license: self.license,
            citation: self.citation,
            published: self.published,
            modified: self.modified,
            lifecycle: self.lifecycle,
            language: self.language,
            taxonomies: self.taxonomies,
            attributions: self.attributions,
            route: self.route,
            slug: self.slug,
            structure: self.structure,
        }
    }
}

/// One position in a journey, without a redundant numeric ordering field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JourneyMember {
    /// Exact leaf-content reference, not a local-name lookup.
    pub content: Key,
    /// Marks the sole containing journey; false is not an alternative membership.
    pub primary: bool,
}

/// Classification uses a composite taxonomy/term identity, never a display slug.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TaxonomyMembership {
    /// Exact configured taxonomy key.
    pub taxonomy: Key,
    /// Exact term key within that taxonomy; other taxonomies are separate namespaces.
    pub term: Key,
}

/// Direct collection residence, mutually exclusive with journey containment.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CollectionMembership {
    /// Leaf-content reference; container parents have their own typed fields.
    pub content: Key,
    /// Exact collection reference.
    pub collection: Key,
}

/// Editorial association carries no structural containment or inheritance.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct EditorialRelation {
    /// Explicit source content object.
    pub source: Key,
    /// Explicit related content object, including another site's object.
    pub target: Key,
}

/// Complete structural declarations; this is not a publication authorization packet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    /// Full selected content set, including draft and archived objects.
    pub nodes: Vec<ContentNode>,
    /// All explicit leaf-to-collection memberships.
    pub memberships: Vec<CollectionMembership>,
    /// All explicit editorial associations, separate from membership.
    pub related: Vec<EditorialRelation>,
    /// Explicit page-owned directory paths, without editorial containment.
    #[serde(default)]
    pub browsing: Vec<BrowsingRelation>,
    /// Explicit page subjects/mentions and container context, never inferred from names.
    #[serde(default)]
    pub projects: Vec<ProjectDeclaration>,
}

/// Bounds apply to the complete graph before indexing or traversal.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum content nodes, including containers.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Combined input edges; independently also caps context traversal steps and
    /// unique derived project evidence. No partial propagation is returned.
    #[schemars(range(max = usize::MAX))]
    pub edges: usize,
    /// Aggregate declaration text and serialized extension data; independently
    /// caps derived project evidence key bytes.
    #[schemars(range(max = usize::MAX))]
    pub text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            nodes: 16_384,
            edges: 131_072,
            text_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Deterministic structural failure, not a policy-admission decision.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Diagnostic {
    /// Stable error class.
    pub code: &'static str,
    /// Exact declaring content identity.
    pub source: Key,
    /// Implicated endpoint when applicable.
    pub target: Option<Key>,
}

/// No partial graph is returned on a resource or structural error.
#[derive(Debug)]
pub enum Error {
    /// A complete-set resource ceiling was exceeded.
    Limit(&'static str),
    /// Independent failures in stable order.
    Invalid(Vec<Diagnostic>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => write!(f, "content graph {resource} ceiling exceeded"),
            Self::Invalid(errors) => {
                write!(f, "{} content graph constraints failed", errors.len())?;
                for error in errors {
                    write!(f, "\n  {} at {}", error.code, error.source)?;
                    if let Some(target) = &error.target {
                        write!(f, " -> {target}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

/// Immutable structural graph; private construction follows whole-set validation.
///
/// This resolves content endpoints, collection cycles and project-relation context.
/// Site/project catalogs are joined by the compiler. Rights, source provenance,
/// routes and policy admission are not implied by structural validity.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Graph {
    nodes: BTreeMap<Key, ContentNode>,
    memberships: BTreeSet<CollectionMembership>,
    related: BTreeSet<EditorialRelation>,
    browsing: BTreeSet<BrowsingRelation>,
    #[serde(skip)]
    browsing_by_source: BTreeMap<Key, BTreeSet<Key>>,
    projects: BTreeSet<ProjectDeclaration>,
    project_links: BTreeMap<Key, BTreeMap<Key, ProjectLink>>,
    #[serde(skip)]
    residences: BTreeMap<Key, Key>,
    #[serde(skip)]
    children: BTreeMap<Key, Vec<Key>>,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    documentation: BTreeSet<Key>,
}

/// Read-only content and validated containment before route completion.
/// Canonical references are checked after the caller supplies missing paths.
#[derive(Clone, Copy)]
pub struct RoutingContext<'a> {
    graph: &'a Graph,
}

impl RoutingContext<'_> {
    /// Every identity, including unrouted, draft and archived content.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &ContentNode> {
        self.graph.nodes()
    }

    /// Exact known content identity.
    pub fn node(&self, id: &Key) -> Option<&ContentNode> {
        self.graph.node(id)
    }

    /// Validated immediate collection or journey parent.
    pub fn editorial_parent(&self, id: &Key) -> Option<&ContentNode> {
        self.graph.editorial_parent(id)
    }

    /// Documentation classification from the validated editorial ancestry.
    pub fn is_documentation(&self, id: &Key) -> bool {
        self.graph.is_documentation(id)
    }
}

/// Borrowed ordered view for human and machine navigation projections.
///
/// This is structural evidence, not an admission-filtered public navigation list.
/// Source titles and states are borrowed from the same nodes, never reconstructed.
#[derive(Debug, Serialize)]
pub struct JourneyView<'a> {
    /// The exact journey record, including its declared sequence.
    pub journey: &'a ContentNode,
    /// Every member in that sequence, including draft/archived content.
    pub steps: Vec<&'a ContentNode>,
}

impl Graph {
    /// Resolve every declaration without I/O, filtering lifecycles or inferring memberships.
    ///
    /// # Errors
    /// Rejects limits, invalid authored text, duplicate identities/edges, missing or wrong-kind endpoints,
    /// repeated journey members, multiple residences, uncontained journeys,
    /// cross-site containment and collection cycles.
    pub fn resolve(declaration: Declaration, limits: Limits) -> Result<Self, Error> {
        resolve::graph(declaration, limits, None)
    }

    /// Resolve structure, derive missing paths, then validate canonical references.
    /// The callback receives validated containment, never a completed routed graph.
    /// It may supply paths only for known nodes without an explicit route.
    ///
    /// # Errors
    /// Propagates structural or derivation failures and refuses route replacements,
    /// unknown identities, canonical failures and combined declaration/path limits.
    pub fn resolve_with_routes(
        declaration: Declaration,
        limits: Limits,
        derive: impl Fn(RoutingContext<'_>) -> Result<BTreeMap<Key, super::routing::RoutePath>, Error>,
    ) -> Result<Self, Error> {
        resolve::graph(declaration, limits, Some(&derive))
    }

    /// All content objects in stable identity order, including non-published objects.
    pub fn nodes(&self) -> impl ExactSizeIterator<Item = &ContentNode> {
        self.nodes.values()
    }

    /// Resolve an exact content identity; no slug, title or site-local fallback.
    pub fn node(&self, id: &Key) -> Option<&ContentNode> {
        self.nodes.get(id)
    }

    /// Documentation category from authoritative containment or an explicit leaf family.
    /// Links, project assertions, source paths and presentation never contribute.
    pub fn is_documentation(&self, id: &Key) -> bool {
        self.documentation.contains(id)
    }

    /// Complete collection membership, distinct from ordered journey members.
    pub fn memberships(&self) -> &BTreeSet<CollectionMembership> {
        &self.memberships
    }

    /// Editorial edges do not affect ancestry or primary traversal.
    pub fn related(&self) -> &BTreeSet<EditorialRelation> {
        &self.related
    }

    /// Page-owned directory paths, independent of residence and project subjects.
    pub fn browsing(&self) -> &BTreeSet<BrowsingRelation> {
        &self.browsing
    }

    /// Direct directory identities only; this never walks editorial or project ancestry.
    pub fn browsing_indexes<'a>(&'a self, source: &Key) -> impl Iterator<Item = &'a Key> {
        self.browsing_by_source
            .get(source)
            .into_iter()
            .flat_map(BTreeSet::iter)
    }

    /// Original declarations, including unused container context; catalogs must
    /// validate every project reference, not only references that reach a leaf.
    pub fn declared_projects(&self) -> &BTreeSet<ProjectDeclaration> {
        &self.projects
    }

    /// Final page relationships and every contributing declaration origin.
    ///
    /// Returns an empty map for a known page without relationships, and none for
    /// containers or unknown keys. This internal view is not disclosure admission.
    pub fn project_relations(&self, content: &Key) -> Option<&BTreeMap<Key, ProjectLink>> {
        self.project_links.get(content)
    }

    /// The sole containing journey, or none for standalone/direct-collection pages.
    pub fn primary_journey(&self, content: &Key) -> Option<&ContentNode> {
        self.editorial_parent(content)
            .filter(|node| matches!(node.structure, ContentKind::Journey { .. }))
    }

    /// Immediate declared editorial parent, never a source folder or related link.
    /// A known standalone page or root collection has no parent.
    pub fn editorial_parent(&self, content: &Key) -> Option<&ContentNode> {
        self.residences
            .get(content)
            .and_then(|id| self.nodes.get(id))
    }

    /// Direct children in journey order or the collection's selected member order.
    /// Unlisted collection children follow in identity order.
    pub fn editorial_children<'a>(&'a self, content: &Key) -> Vec<&'a ContentNode> {
        if let Some(ContentNode {
            structure: ContentKind::Journey { members, .. },
            ..
        }) = self.node(content)
        {
            return members
                .iter()
                .filter_map(|member| self.node(&member.content))
                .collect();
        }
        self.children
            .get(content)
            .into_iter()
            .flatten()
            .filter_map(|id| self.node(id))
            .collect()
    }

    /// Nearest declared documentation outline, or the outermost documentation collection.
    pub fn documentation_navigation_owner(&self, content: &Key) -> Option<&ContentNode> {
        if !self.is_documentation(content) {
            return None;
        }
        let mut fallback = None;
        for node in self.editorial_ancestry(content)? {
            if let ContentKind::Collection { navigation, .. } = &node.structure {
                if !self.is_documentation(&node.id) {
                    continue;
                }
                if navigation.is_some() {
                    return Some(node);
                }
                fallback = Some(node);
            }
        }
        fallback
    }

    /// The object followed by its complete editorial ancestry, nearest first.
    /// Whole-graph validation proves single residence and acyclic containment.
    pub fn editorial_ancestry(&self, content: &Key) -> Option<impl Iterator<Item = &ContentNode>> {
        let node = self.node(content)?;
        Some(std::iter::successors(Some(node), |node| {
            self.editorial_parent(&node.id)
        }))
    }

    /// Borrow a container followed by its explicit ancestors, nearest first.
    ///
    /// The resolved graph already proves parent kinds and acyclicity. Unknown
    /// keys and leaf pages have no container ancestry; memberships are separate.
    pub fn container_ancestry(&self, id: &Key) -> Option<impl Iterator<Item = &ContentNode>> {
        let node = self.node(id)?;
        if matches!(node.structure, ContentKind::Page { .. }) {
            return None;
        }
        self.editorial_ancestry(id)
    }

    /// Borrow the journey's exact ordered member sequence. Draft members are not removed.
    pub fn journey_members(&self, journey: &Key) -> Option<&[JourneyMember]> {
        match &self.nodes.get(journey)?.structure {
            ContentKind::Journey { members, .. } => Some(members),
            _ => None,
        }
    }

    /// Resolve a complete ordered view, preserving titles, identities and states.
    ///
    /// Returns none for an unknown or non-journey key. This performs no policy
    /// filtering and grants no publication permission; later admission must not
    /// silently shrink the selected journey into a successful partial view.
    pub fn journey_view(&self, journey: &Key) -> Option<JourneyView<'_>> {
        let steps = self
            .journey_members(journey)?
            .iter()
            .map(|member| self.nodes.get(&member.content))
            .collect::<Option<Vec<_>>>()?;
        Some(JourneyView {
            journey: self.nodes.get(journey)?,
            steps,
        })
    }
}
