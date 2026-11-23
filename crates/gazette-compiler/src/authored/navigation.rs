//! Bounded structural paths over resolved nodes and routes, not site-menu selection.

use std::io;

use gazette_core::{
    publication::{
        Key,
        content::{ContentKind, ContentNode, JourneyMember, Limits, NavigationItem},
    },
    render::LimitedWriter,
};
use serde::Serialize;

use super::{ContentView, DiscoveryNode, ProjectionError, SitePresentation};
use crate::workspace::{ResolvedContent, ResolvedRoute};

/// One workspace-selected destination resolved through Gazette's graph and route table.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ResolvedDestination<'a> {
    /// Existing semantic object with its home-site presentation identity.
    pub content: ContentView<'a>,
    /// Exact configured canonical route; no slug or HTML is invented.
    pub route: &'a ResolvedRoute,
}

/// An exact structural endpoint; absent routing never removes the relationship.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct NavigationLink<'a> {
    /// Whether the destination is an explicitly configured native listing.
    #[schemars(default, skip_serializing_if = "std::ops::Not::not")]
    pub listing: bool,
    /// Borrowed node retains identity, title, home and lifecycle without inference.
    #[schemars(schema_with = "navigation_node_schema")]
    pub node: &'a ContentNode,
    /// Only a configured canonical route can make this endpoint clickable.
    pub route: Option<&'a ResolvedRoute>,
    /// Destination Theme/palette identity, never inferred from the route or domain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
}

/// One root-first path through an explicit container, ending at the selected node.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct BreadcrumbTrail<'a> {
    /// Direct collection or journey that distinguishes this path; self when uncontained.
    pub via: &'a Key,
    /// Complete ordered endpoints, including unrouted and non-published nodes.
    pub items: Vec<NavigationLink<'a>>,
}

/// Direct contents of the selected page's actual editorial collection.
/// Resolved collection order presents reading choices without sequence or progress.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CollectionNavigation<'a> {
    /// Exact containing collection and its optional configured destination.
    pub collection: NavigationLink<'a>,
    /// Direct children only, including the selected page; no recursive flattening.
    pub members: Vec<NavigationLink<'a>>,
}

/// One complete product-documentation outline over existing content destinations.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationNavigation<'a> {
    /// Exact collection governing this outline and its configured destination.
    pub root: NavigationLink<'a>,
    /// Whether the governing collection is the page currently being projected.
    pub current: bool,
    /// Balanced traversal in authored order, consumed without template recursion.
    pub events: Vec<DocumentationNavigationEvent<'a>>,
}

/// One semantic opening or closing in the complete documentation traversal.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DocumentationNavigationEvent<'a> {
    /// Begin one existing page or container destination.
    PageOpen {
        /// Existing content title, identity and resolved address.
        destination: NavigationLink<'a>,
        /// Whether this is the page currently being projected.
        current: bool,
        /// Whether this container belongs to the selected page's real ancestry.
        ancestor: bool,
        /// Whether admitted child events occur before the matching closing.
        has_children: bool,
    },
    /// End the matching destination, without template-side stack reconstruction.
    PageClose {
        /// Same resolved child-list state as the matching opening.
        has_children: bool,
    },
    /// Begin a named grouping that creates no content node or parent.
    GroupOpen {
        /// Original plain-text navigation label.
        title: &'a str,
    },
    /// End the matching nonempty group.
    GroupClose,
}

enum DocumentationStep<'a> {
    Item(&'a NavigationItem),
    Page(&'a ContentNode),
    PageClose(usize),
    GroupClose(usize),
}

/// Explicit participation, ordered members and neighbors in one complete journey sequence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct JourneyMembership<'a> {
    /// Existing graph node, not a separately reconstructed journey.
    #[schemars(schema_with = "navigation_node_schema")]
    pub journey: &'a ContentNode,
    /// Declared primary status for the selected content only.
    pub primary: bool,
    /// Journey's exact optional route, without a slug-derived fallback.
    pub route: Option<&'a ResolvedRoute>,
    /// Zero-based index in the declared sequence, before any public admission.
    pub position: usize,
    /// Full declared sequence length, including draft and archived members.
    pub total: usize,
    /// Complete declared order, retaining the selected, unrouted and non-published nodes.
    pub members: Vec<NavigationLink<'a>>,
    /// Immediate predecessor; absence means first, not an omitted unrouted member.
    pub previous: Option<NavigationLink<'a>>,
    /// Immediate successor; absence means last, not an omitted unrouted member.
    pub next: Option<NavigationLink<'a>>,
}

fn navigation_node_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    // Private inspection keeps native edges; discovery-safe widgets emit the
    // same identity with shallow structure and destination presentation instead.
    schemars::json_schema!({
        "anyOf": [
            generator.subschema_for::<ContentNode>(),
            generator.subschema_for::<DiscoveryNode<'_>>()
        ]
    })
}

pub(super) fn discovery_nodes_schema(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    schemars::json_schema!({
        "type": "array",
        "items": {
            "anyOf": [
                generator.subschema_for::<ContentView<'_>>(),
                generator.subschema_for::<DiscoveryNode<'_>>()
            ]
        }
    })
}

pub(super) struct NavigationBuilder<'a> {
    content: &'a ResolvedContent<'a>,
    remaining: usize,
    bytes: LimitedWriter<io::Sink>,
}

impl<'a> NavigationBuilder<'a> {
    pub(super) fn documentation(
        &mut self,
        selected: &'a ContentNode,
        discovery: Option<&super::Discovery<'_>>,
    ) -> Result<Option<DocumentationNavigation<'a>>, ProjectionError> {
        let Some(root) = self
            .content
            .graph()
            .documentation_navigation_owner(&selected.id)
        else {
            return Ok(None);
        };
        if discovery.is_some_and(|discovery| !discovery.includes_navigation_node(root)) {
            return Ok(None);
        }
        let mut stack = Vec::new();
        self.documentation_children(root, &mut stack);
        let mut events = Vec::new();
        let mut work = self.remaining;
        while let Some(step) = stack.pop() {
            work = work
                .checked_sub(1)
                .ok_or(ProjectionError::Limit("documentation navigation traversal"))?;
            match step {
                DocumentationStep::Item(item) => match item {
                    NavigationItem::Content { content } => {
                        let node = self.content.graph().node(content).ok_or_else(|| {
                            ProjectionError::Binding {
                                id: content.clone(),
                                reason: "resolved documentation endpoint missing",
                            }
                        })?;
                        stack.push(DocumentationStep::Page(node));
                    }
                    NavigationItem::Group { group, pages } => {
                        let opening = events.len();
                        self.documentation_event(
                            &mut events,
                            DocumentationNavigationEvent::GroupOpen { title: group },
                        )?;
                        stack.push(DocumentationStep::GroupClose(opening));
                        stack.extend(pages.iter().rev().map(DocumentationStep::Item));
                    }
                },
                DocumentationStep::Page(node) => {
                    if discovery.is_some_and(|discovery| !discovery.includes_navigation_node(node))
                    {
                        continue;
                    }
                    let ancestor = self
                        .content
                        .graph()
                        .editorial_ancestry(&selected.id)
                        .is_some_and(|mut path| path.any(|parent| parent.id == node.id))
                        && node.id != selected.id;
                    let opening = events.len();
                    self.documentation_event(
                        &mut events,
                        DocumentationNavigationEvent::PageOpen {
                            destination: self.destination(node),
                            current: node.id == selected.id,
                            ancestor,
                            has_children: false,
                        },
                    )?;
                    stack.push(DocumentationStep::PageClose(opening));
                    if !matches!(node.structure, ContentKind::Page { .. }) {
                        self.documentation_children(node, &mut stack);
                    }
                }
                DocumentationStep::PageClose(opening) => {
                    let has_children = events.len() > opening + 1;
                    let Some(DocumentationNavigationEvent::PageOpen {
                        has_children: value,
                        ..
                    }) = events.get_mut(opening)
                    else {
                        return Err(ProjectionError::Binding {
                            id: root.id.clone(),
                            reason: "documentation closing has no matching opening",
                        });
                    };
                    *value = has_children;
                    self.documentation_event(
                        &mut events,
                        DocumentationNavigationEvent::PageClose { has_children },
                    )?;
                }
                DocumentationStep::GroupClose(opening) => {
                    if events.len() == opening + 1 {
                        events.truncate(opening);
                    } else {
                        self.documentation_event(
                            &mut events,
                            DocumentationNavigationEvent::GroupClose,
                        )?;
                    }
                }
            }
        }
        Ok(Some(DocumentationNavigation {
            root: self.link(root)?,
            current: root.id == selected.id,
            events,
        }))
    }

    fn documentation_children(
        &self,
        node: &'a ContentNode,
        stack: &mut Vec<DocumentationStep<'a>>,
    ) {
        if let ContentKind::Collection {
            navigation: Some(items),
            ..
        } = &node.structure
        {
            stack.extend(items.iter().rev().map(DocumentationStep::Item));
            return;
        }
        stack.extend(
            self.content
                .graph()
                .editorial_children(&node.id)
                .into_iter()
                .rev()
                .map(DocumentationStep::Page),
        );
    }

    fn documentation_event(
        &mut self,
        events: &mut Vec<DocumentationNavigationEvent<'a>>,
        event: DocumentationNavigationEvent<'a>,
    ) -> Result<(), ProjectionError> {
        self.charge(&event)?;
        events.push(event);
        Ok(())
    }

    pub(super) fn new(content: &'a ResolvedContent<'a>, limits: Limits) -> Self {
        Self {
            content,
            remaining: limits.edges,
            bytes: LimitedWriter::new(io::sink(), limits.text_bytes),
        }
    }

    pub(super) fn link(
        &mut self,
        node: &'a ContentNode,
    ) -> Result<NavigationLink<'a>, ProjectionError> {
        let link = self.destination(node);
        self.charge(&link)?;
        Ok(link)
    }

    fn destination(&self, node: &'a ContentNode) -> NavigationLink<'a> {
        NavigationLink {
            listing: self.content.is_listing(node),
            node,
            route: self.content.route(&node.id),
            presentation: self
                .content
                .presentation_for(&node.site)
                .map(SitePresentation::from),
        }
    }

    pub(super) fn view(
        &mut self,
        node: &'a ContentNode,
    ) -> Result<ContentView<'a>, ProjectionError> {
        self.view_with_artwork(node, None)
    }

    pub(super) fn view_with_artwork(
        &mut self,
        node: &'a ContentNode,
        artwork: Option<super::ListingArtwork>,
    ) -> Result<ContentView<'a>, ProjectionError> {
        let mut view = ContentView::new(
            node,
            self.content.presentation_for(&node.site),
            self.content.route(&node.id),
            self.content.graph().is_documentation(&node.id),
            self.content.is_listing(node),
        );
        view.artwork = artwork;
        self.charge(&view)?;
        Ok(view)
    }

    pub(super) fn charge(&mut self, endpoint: &impl Serialize) -> Result<(), ProjectionError> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(ProjectionError::Limit("navigation endpoint occurrences"))?;
        // Bound repeated endpoint/route expansion before retaining a page view.
        let encoded = serde_json::to_writer(&mut self.bytes, endpoint);
        if self.bytes.exceeded() {
            return Err(ProjectionError::Limit("navigation endpoint JSON bytes"));
        }
        encoded.map_err(ProjectionError::Encoding)?;
        Ok(())
    }

    fn member(&mut self, member: &'a JourneyMember) -> Result<NavigationLink<'a>, ProjectionError> {
        let node =
            self.content
                .graph()
                .node(&member.content)
                .ok_or_else(|| ProjectionError::Binding {
                    id: member.content.clone(),
                    reason: "resolved journey endpoint missing",
                })?;
        self.link(node)
    }

    pub(super) fn journey(
        &mut self,
        journey: &'a ContentNode,
        members: &'a [JourneyMember],
        position: usize,
    ) -> Result<JourneyMembership<'a>, ProjectionError> {
        let endpoint = self.link(journey)?;
        Ok(JourneyMembership {
            journey,
            primary: members[position].primary,
            route: endpoint.route,
            position,
            total: members.len(),
            members: members
                .iter()
                .map(|member| self.member(member))
                .collect::<Result<_, _>>()?,
            previous: position
                .checked_sub(1)
                .map(|index| self.member(&members[index]))
                .transpose()?,
            next: members
                .get(position + 1)
                .map(|member| self.member(member))
                .transpose()?,
        })
    }

    fn trail(
        &mut self,
        via: &'a ContentNode,
        leaf: &'a ContentNode,
        discovery: Option<&super::Discovery<'_>>,
    ) -> Result<Option<BreadcrumbTrail<'a>>, ProjectionError> {
        if let Some(discovery) = discovery
            && (!discovery.includes_navigation_node(leaf)
                || !discovery.includes_navigation_node(via)
                || self
                    .content
                    .graph()
                    .container_ancestry(&via.id)
                    .is_some_and(|mut nodes| {
                        nodes.any(|node| !discovery.includes_navigation_node(node))
                    }))
        {
            return Ok(None);
        }
        let mut items = Vec::new();
        if let Some(ancestors) = self.content.graph().container_ancestry(&via.id) {
            for ancestor in ancestors {
                items.push(self.link(ancestor)?);
            }
            items.reverse();
        }
        if via.id != leaf.id || items.is_empty() {
            items.push(self.link(leaf)?);
        }
        Ok(Some(BreadcrumbTrail {
            via: &via.id,
            items,
        }))
    }

    pub(super) fn breadcrumbs(
        &mut self,
        node: &'a ContentNode,
        collections: &[&'a ContentNode],
        journeys: &[JourneyMembership<'a>],
        discovery: Option<&super::Discovery<'_>>,
    ) -> Result<Vec<BreadcrumbTrail<'a>>, ProjectionError> {
        if !matches!(node.structure, ContentKind::Page { .. })
            || (collections.is_empty() && journeys.is_empty())
        {
            // Filtering all declared paths must not invent a new standalone trail.
            // A genuinely uncontained page still has its ordinary self endpoint.
            if discovery.is_some()
                && matches!(node.structure, ContentKind::Page { .. })
                && self.content.graph().editorial_parent(&node.id).is_some()
            {
                return Ok(Vec::new());
            }
            return Ok(self.trail(node, node, discovery)?.into_iter().collect());
        }
        collections
            .iter()
            .copied()
            .chain(journeys.iter().map(|membership| membership.journey))
            .map(|via| self.trail(via, node, discovery))
            .filter_map(Result::transpose)
            .collect()
    }
}
