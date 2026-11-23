//! Borrowed serialization roles over a projected page, never another semantic graph.

use gazette_core::publication::content::{ContentKind, ContentNode};
use serde::{Serialize, Serializer, ser::SerializeMap};

use super::{Discovery, DiscoveryNode};
use crate::authored::{
    BreadcrumbTrail, CollectionNavigation, ContentView, JourneyMembership, NavigationLink,
    PageProjection,
};

/// A direct-page or aggregate-listing view with discovery-safe generated endpoints.
/// It borrows the already rendered body and metadata; no parsing or lookup is repeated.
pub struct DiscoveryPage<'a, 'data> {
    page: &'a PageProjection<'data>,
    discovery: &'a Discovery<'data>,
    role: Role,
}

#[derive(Clone, Copy)]
enum Role {
    Direct,
    Listing,
}

impl Discovery<'_> {
    pub(crate) fn includes_navigation_node(&self, node: &ContentNode) -> bool {
        self.owns(node)
            && self.contains(&node.id)
            && match &node.structure {
                ContentKind::Journey { members, .. } => {
                    members.iter().all(|member| self.contains(&member.content))
                }
                _ => true,
            }
    }

    pub(crate) fn includes_trail(&self, trail: &BreadcrumbTrail<'_>) -> bool {
        self.contains(trail.via)
            && trail
                .items
                .iter()
                .all(|item| self.includes_navigation_node(item.node))
    }

    pub(crate) fn includes_journey(&self, journey: &JourneyMembership<'_>) -> bool {
        self.includes_navigation_node(journey.journey)
            && matches!(journey.journey.structure, ContentKind::Journey { .. })
            && [journey.previous.as_ref(), journey.next.as_ref()]
                .into_iter()
                .flatten()
                .chain(journey.members.iter())
                .all(|item| self.owns(item.node) && self.contains(&item.node.id))
    }

    pub(crate) fn includes_project(&self, project: &crate::authored::ProjectView<'_>) -> bool {
        project
            .relationship
            .evidence()
            .iter()
            .all(|evidence| self.contains(&evidence.source))
    }

    pub(crate) fn trails<'a>(&'a self, trails: &'a [BreadcrumbTrail<'a>]) -> impl Serialize + 'a {
        Trails(trails, self)
    }
}

/// Metadata-only projection used by feed extensions without duplicating their bodies.
pub struct DiscoveryData<'a, 'data>(DiscoveryPage<'a, 'data>);

/// Search record with the existing plain body/excerpt contract and safe metadata.
pub struct DiscoverySearch<'a, 'data>(DiscoveryPage<'a, 'data>);

impl<'a, 'data> DiscoveryPage<'a, 'data> {
    /// Borrow the same metadata serialization, excluding body and excerpt fields.
    pub fn data(&self) -> DiscoveryData<'a, 'data> {
        DiscoveryData(DiscoveryPage {
            page: self.page,
            discovery: self.discovery,
            role: self.role,
        })
    }

    /// Borrow searchable text and a plain excerpt without copying metadata or HTML.
    /// Even a direct-page view must enter the eligible listing role for search.
    pub fn search(&self) -> Option<DiscoverySearch<'a, 'data>> {
        let node = self.page.data.content.node;
        if self
            .discovery
            .owner
            .compilation()
            .workspace
            .publication_for(&node.site)
            .and_then(|publication| publication.search_page.as_ref())
            == Some(&node.id)
        {
            return None;
        }
        self.page.listing(self.discovery).map(DiscoverySearch)
    }

    /// Borrow explicit ranking fields with the same discovery-safe result metadata.
    /// Commentary contributes plain text under its article's existing identity only.
    pub fn search_with(
        &self,
        selection: &'a crate::workspace::SearchConfiguration,
    ) -> Option<impl Serialize + 'a> {
        self.search()
            .map(|document| crate::authored::search::IndexedSearch {
                document,
                indexed: crate::authored::search::IndexedText {
                    page: self.page,
                    selection,
                    discovery: Some(self.discovery),
                },
            })
    }
}

impl<'data> PageProjection<'data> {
    /// Preserve direct authored meaning while serializing only eligible widget endpoints.
    /// Returns none when this page belongs to another resolved source.
    pub fn discovery_view<'a>(
        &'a self,
        discovery: &'a Discovery<'data>,
    ) -> Option<DiscoveryPage<'a, 'data>> {
        discovery
            .owns(self.data.content.node)
            .then_some(DiscoveryPage {
                page: self,
                discovery,
                role: Role::Direct,
            })
    }

    /// Borrow an eligible aggregate record without raw structural member lists.
    /// Searchable text and explicit summary remain the same rendered meaning. This
    /// is discoverability, not secrecy: authored prose can name an unlisted page.
    pub fn listing<'a>(
        &'a self,
        discovery: &'a Discovery<'data>,
    ) -> Option<DiscoveryPage<'a, 'data>> {
        discovery.node(self.data.content.node)?;
        Some(DiscoveryPage {
            page: self,
            discovery,
            role: Role::Listing,
        })
    }
}

impl Serialize for DiscoveryPage<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.fields(&mut map, true)?;
        if matches!(self.role, Role::Direct) && self.page.data.container_contents_placed {
            map.serialize_entry("container_contents_placed", &true)?;
        }
        map.serialize_entry("body_html", &self.page.body_html)?;
        if let Some(opening) = self.page.opening() {
            map.serialize_entry("opening", &opening)?;
        }
        map.serialize_entry("body_text", &self.page.body_text)?;
        if let Some(summary) = &self.page.summary {
            map.serialize_entry("summary", summary)?;
        }
        map.end()
    }
}

impl Serialize for DiscoverySearch<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.0.fields(&mut map, false)?;
        map.serialize_entry("body", &self.0.page.body_text)?;
        if let Some(summary) = &self.0.page.summary {
            map.serialize_entry("summary", &summary.text)?;
        }
        map.end()
    }
}

impl Serialize for DiscoveryData<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.0.fields(&mut map, false)?;
        map.end()
    }
}

impl DiscoveryPage<'_, '_> {
    /// `page` marks a whole page record, for a page or listing context and the
    /// machine model, which carries notebook cells; search and feed metadata
    /// records keep the Notes text only.
    fn fields<M: SerializeMap>(&self, map: &mut M, page: bool) -> Result<(), M::Error> {
        let data = &self.page.data;
        if matches!(self.role, Role::Direct) && !data.invitations.is_empty() {
            map.serialize_entry("invitations", &data.invitations)?;
        }
        map.serialize_entry("images", &data.images)?;
        if let Some(listing) = &data.listing {
            map.serialize_entry("listing", listing)?;
        }
        if let Some(resources) = &data.project_resources {
            map.serialize_entry("project_resources", resources)?;
        }
        if let Some(invitation) = &data.preferred_source {
            map.serialize_entry("preferred_source", invitation)?;
        }
        if let Some(citation) = &data.citation {
            map.serialize_entry("citation", citation)?;
        }
        if let Some(notes) = &data.notes {
            if page {
                map.serialize_entry("notes", notes)?;
            } else {
                map.serialize_entry("notes", &notes.without_notebooks())?;
            }
        }
        self.discovery
            .validate_page_projects(data, matches!(self.role, Role::Listing))
            .map_err(serde::ser::Error::custom)?;
        match self.role {
            Role::Direct => map.serialize_entry("content", &data.content)?,
            Role::Listing => {
                let node = self.discovery.node(data.content.node).ok_or_else(|| {
                    serde::ser::Error::custom("listing no longer binds an eligible node")
                })?;
                map.serialize_entry("content", &node)?;
            }
        }
        map.serialize_entry("classifications", &data.classifications)?;
        map.serialize_entry("recommendations", &data.recommendations)?;
        if let Some(mount) = &data.documentation_mount
            && self
                .discovery
                .includes_navigation_node(mount.landing.content.node)
        {
            map.serialize_entry("documentation_mount", mount)?;
        }
        map.serialize_entry("site", &data.site)?;
        if let Some(processing) = &data.legal_processing {
            map.serialize_entry("legal_processing", processing)?;
        }
        if let Some(discussion) = &data.discussion {
            map.serialize_entry("discussion", discussion)?;
        }
        map.serialize_entry("attributions", &data.attributions)?;
        if let Some(route) = data.route {
            map.serialize_entry("route", route)?;
        }
        if let Some(provenance) = &data.provenance {
            map.serialize_entry("provenance", provenance)?;
        }
        if let Some(derivation) = data.derivation {
            map.serialize_entry("derivation", derivation)?;
        }
        map.serialize_entry("headings", data.headings)?;
        map.serialize_entry("components", &data.components)?;
        map.serialize_entry("links", &data.links)?;
        map.serialize_entry("assets", &data.assets)?;
        for (key, nodes) in [
            ("collections", &data.collections),
            ("children", &data.children),
            ("related", &data.related),
        ] {
            map.serialize_entry(key, &Nodes(nodes, self.discovery))?;
        }
        map.serialize_entry("journeys", &Journeys(&data.journeys, self.discovery))?;
        map.serialize_entry(
            "collection_navigation",
            &CollectionNavigations(&data.collection_navigation, self.discovery),
        )?;
        if matches!(self.role, Role::Direct)
            && let Some(navigation) = &data.documentation_navigation
            && self
                .discovery
                .includes_navigation_node(navigation.root.node)
        {
            map.serialize_entry(
                "documentation_navigation",
                &DocumentationOutline(navigation, self.discovery),
            )?;
        }
        map.serialize_entry("breadcrumbs", &self.discovery.trails(&data.breadcrumbs))?;
        match self.role {
            Role::Direct => map.serialize_entry("projects", &data.projects)?,
            Role::Listing => {
                map.serialize_entry("projects", &Projects(&data.projects, self.discovery))?
            }
        }
        map.serialize_entry("project_declarations", &data.project_declarations)?;
        if matches!(self.role, Role::Direct) {
            map.serialize_entry("steps", &data.steps)?;
        }
        Ok(())
    }
}

struct Projects<'a>(&'a [crate::authored::ProjectView<'a>], &'a Discovery<'a>);
impl Serialize for Projects<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .filter(|project| self.1.includes_project(project)),
        )
    }
}

struct Nodes<'a>(&'a [ContentView<'a>], &'a Discovery<'a>);
impl Serialize for Nodes<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().filter_map(|view| {
            self.1
                .node(view.node)
                .map(|node| node.with_artwork(view.artwork.as_ref()))
        }))
    }
}

#[derive(Serialize)]
struct Link<'a> {
    node: DiscoveryNode<'a>,
    route: Option<&'a crate::workspace::ResolvedRoute>,
}
fn link<'a>(value: &'a NavigationLink<'a>, discovery: &'a Discovery<'a>) -> Option<Link<'a>> {
    Some(Link {
        node: discovery.node(value.node)?,
        route: value.route,
    })
}

struct CollectionNavigations<'a>(&'a [CollectionNavigation<'a>], &'a Discovery<'a>);

struct DocumentationOutline<'a>(
    &'a crate::authored::DocumentationNavigation<'a>,
    &'a Discovery<'a>,
);
impl Serialize for DocumentationOutline<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("root", &link(&self.0.root, self.1))?;
        map.serialize_entry("current", &self.0.current)?;
        map.serialize_entry("events", &DocumentationEvents(&self.0.events, self.1))?;
        map.end()
    }
}
struct DocumentationEvents<'a>(
    &'a [crate::authored::DocumentationNavigationEvent<'a>],
    &'a Discovery<'a>,
);
struct DocumentationEvent<'a> {
    event: &'a crate::authored::DocumentationNavigationEvent<'a>,
    discovery: &'a Discovery<'a>,
    has_children: bool,
}
impl Serialize for DocumentationEvents<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use crate::authored::DocumentationNavigationEvent as Event;
        use serde::ser::Error as _;
        let mut output: Vec<DocumentationEvent<'_>> = Vec::new();
        // Each entry is a semantic boundary and an optional retained opening.
        // Filtering never reinterprets content parents or reconstructs a tree.
        let mut stack: Vec<(bool, Option<usize>)> = Vec::new();
        for event in self.0 {
            match event {
                Event::GroupOpen { .. } | Event::PageOpen { .. } => {
                    let page = matches!(event, Event::PageOpen { .. });
                    let allowed = stack.last().is_none_or(|(_, opening)| opening.is_some())
                        && match event {
                            Event::PageOpen { destination, .. } => {
                                self.1.includes_navigation_node(destination.node)
                            }
                            _ => true,
                        };
                    let opening = allowed.then_some(output.len());
                    if allowed {
                        output.push(DocumentationEvent {
                            event,
                            discovery: self.1,
                            has_children: false,
                        });
                    }
                    stack.push((page, opening));
                }
                Event::GroupClose | Event::PageClose { .. } => {
                    let Some((page, opening)) = stack.pop() else {
                        return Err(S::Error::custom("unbalanced documentation navigation"));
                    };
                    if page != matches!(event, Event::PageClose { .. }) {
                        return Err(S::Error::custom(
                            "mismatched documentation navigation boundary",
                        ));
                    }
                    if let Some(opening) = opening {
                        let has_children = output.len() > opening + 1;
                        if !page && !has_children {
                            output.truncate(opening);
                        } else {
                            output[opening].has_children = has_children;
                            output.push(DocumentationEvent {
                                event,
                                discovery: self.1,
                                has_children,
                            });
                        }
                    }
                }
            }
        }
        if !stack.is_empty() {
            return Err(S::Error::custom("unclosed documentation navigation"));
        }
        serializer.collect_seq(output)
    }
}
impl Serialize for DocumentationEvent<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use crate::authored::DocumentationNavigationEvent as Event;
        let mut map = serializer.serialize_map(None)?;
        match self.event {
            Event::PageOpen {
                destination,
                current,
                ancestor,
                ..
            } => {
                map.serialize_entry("kind", "page-open")?;
                map.serialize_entry("destination", &link(destination, self.discovery))?;
                map.serialize_entry("current", current)?;
                map.serialize_entry("ancestor", ancestor)?;
                map.serialize_entry("has_children", &self.has_children)?;
            }
            Event::PageClose { .. } => {
                map.serialize_entry("kind", "page-close")?;
                map.serialize_entry("has_children", &self.has_children)?;
            }
            Event::GroupOpen { title } => {
                map.serialize_entry("kind", "group-open")?;
                map.serialize_entry("title", title)?;
            }
            Event::GroupClose => map.serialize_entry("kind", "group-close")?,
        }
        map.end()
    }
}
impl Serialize for CollectionNavigations<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().filter_map(|value| {
            Some(CollectionContents {
                collection: link(&value.collection, self.1)?,
                members: Links(&value.members, self.1),
            })
        }))
    }
}

#[derive(Serialize)]
struct CollectionContents<'a> {
    collection: Link<'a>,
    members: Links<'a>,
}

struct Journeys<'a>(&'a [JourneyMembership<'a>], &'a Discovery<'a>);
impl Serialize for Journeys<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().filter_map(|value| {
            if !self.1.includes_journey(value) {
                return None;
            }
            Some(Membership {
                journey: self.1.node(value.journey)?,
                primary: value.primary,
                route: value.route,
                position: value.position,
                total: value.total,
                members: Links(&value.members, self.1),
                previous: value
                    .previous
                    .as_ref()
                    .and_then(|value| link(value, self.1)),
                next: value.next.as_ref().and_then(|value| link(value, self.1)),
            })
        }))
    }
}

#[derive(Serialize)]
struct Membership<'a> {
    journey: DiscoveryNode<'a>,
    primary: bool,
    route: Option<&'a crate::workspace::ResolvedRoute>,
    position: usize,
    total: usize,
    members: Links<'a>,
    previous: Option<Link<'a>>,
    next: Option<Link<'a>>,
}

struct Trails<'a>(&'a [BreadcrumbTrail<'a>], &'a Discovery<'a>);
impl Serialize for Trails<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .filter(|trail| self.1.includes_trail(trail))
                .map(|trail| Trail(trail, self.1)),
        )
    }
}

struct Trail<'a>(&'a BreadcrumbTrail<'a>, &'a Discovery<'a>);
struct Links<'a>(&'a [NavigationLink<'a>], &'a Discovery<'a>);
impl Serialize for Links<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.iter().filter_map(|value| link(value, self.1)))
    }
}
impl Serialize for Trail<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("via", self.0.via)?;
        map.serialize_entry("items", &Links(&self.0.items, self.1))?;
        map.end()
    }
}
