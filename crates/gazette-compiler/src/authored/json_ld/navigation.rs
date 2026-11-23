//! Standard page breadcrumbs borrow the compiler's ordered paths, never the graph.

use super::{
    ContentKind, ContentNode, Literal, PageFamily, Reference, Sequence, content_facts, identity,
};
use crate::{
    authored::{BreadcrumbTrail, Discovery, NavigationLink, PageData},
    workspace::ResolvedRoute,
};
use serde::{Serialize, Serializer, ser::SerializeMap};

pub(super) fn breadcrumbs<M: SerializeMap>(
    map: &mut M,
    page: &PageData<'_>,
    discovery: Option<&Discovery<'_>>,
) -> Result<(), M::Error> {
    // Retain the exact technical path context as well as standard linked-data
    // ordering. A journey path never becomes a direct collection membership.
    match discovery {
        Some(discovery) => map.serialize_entry(
            "gazette:breadcrumbs",
            &Literal(&discovery.trails(&page.breadcrumbs)),
        )?,
        None => map.serialize_entry("gazette:breadcrumbs", &Literal(&page.breadcrumbs))?,
    }
    if matches!(
        page.content.structure,
        ContentKind::Page {
            family: PageFamily::Article
        }
    ) {
        if let Some((canonical, route)) = page
            .content
            .route
            .as_ref()
            .and_then(|route| route.canonical.as_ref())
            .zip(page.route)
        {
            map.serialize_entry(
                "mainEntityOfPage",
                &CanonicalPage(canonical, &route.canonical_url),
            )
        } else {
            map.serialize_entry("mainEntityOfPage", &ArticlePage(page, discovery))
        }
    } else {
        map.serialize_entry("breadcrumb", &Trails(&page.breadcrumbs, discovery))
    }
}

fn page_header<M: SerializeMap>(
    map: &mut M,
    node: &ContentNode,
    route: Option<&ResolvedRoute>,
) -> Result<(), M::Error> {
    let (space, kind) = match node.structure {
        ContentKind::Page {
            family: PageFamily::Article,
        } => ("page", "WebPage"),
        ContentKind::Collection { .. } | ContentKind::Journey { .. } => {
            ("content", "CollectionPage")
        }
        ContentKind::Page { .. } => ("content", "WebPage"),
    };
    identity(map, space, &node.id)?;
    map.serialize_entry("@type", kind)?;
    content_facts(map, node)?;
    if let Some(route) = route {
        map.serialize_entry("url", &route.url)?;
    }
    Ok(())
}

pub(super) struct PageLink<'a, 'data>(pub(super) &'a NavigationLink<'data>);
impl Serialize for PageLink<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        page_header(&mut map, self.0.node, self.0.route)?;
        map.end()
    }
}

struct ArticlePage<'a, 'data>(&'a PageData<'data>, Option<&'a Discovery<'data>>);
// A syndicated article names its original page without borrowing the copy's
// breadcrumbs, publisher or attribution as facts about that original.
struct CanonicalPage<'a>(&'a gazette_core::publication::Key, &'a str);
impl Serialize for CanonicalPage<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        identity(&mut map, "page", self.0)?;
        map.serialize_entry("@type", "WebPage")?;
        map.serialize_entry("url", self.1)?;
        map.end()
    }
}
impl Serialize for ArticlePage<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        // Article and its presentation page are distinct; breadcrumb's domain
        // is WebPage, not Article. All references reuse this namespace choice.
        page_header(&mut map, self.0.content.node, self.0.route)?;
        map.serialize_entry("breadcrumb", &Trails(&self.0.breadcrumbs, self.1))?;
        map.end()
    }
}

struct Trails<'a>(&'a [BreadcrumbTrail<'a>], Option<&'a Discovery<'a>>);
impl Serialize for Trails<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(
            self.0
                .iter()
                .filter(|trail| {
                    self.1
                        .is_none_or(|discovery| discovery.includes_trail(trail))
                })
                .map(Trail),
        )
    }
}

struct Trail<'a, 'data>(&'a BreadcrumbTrail<'data>);
impl Serialize for Trail<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Item<'a, 'data> {
            #[serde(rename = "@type")]
            kind: &'static str,
            position: usize,
            item: PageLink<'a, 'data>,
        }
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("@type", "BreadcrumbList")?;
        map.serialize_entry("gazette:via", &Reference("content", self.0.via))?;
        map.serialize_entry("numberOfItems", &self.0.items.len())?;
        map.serialize_entry(
            "itemListElement",
            &Sequence(self.0.items.iter().enumerate().map(|(index, link)| Item {
                kind: "ListItem",
                position: index + 1,
                item: PageLink(link),
            })),
        )?;
        map.end()
    }
}
