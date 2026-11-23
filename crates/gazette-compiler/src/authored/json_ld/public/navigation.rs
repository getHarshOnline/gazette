//! Ordered page relationships without native breadcrumb-path bookkeeping.

use super::{ContentKind, Discovery, PageFamily, Sequence};
use crate::authored::public_identity;
use crate::authored::{BreadcrumbTrail, PageData};
use serde::{Serialize, Serializer, ser::SerializeMap};

pub(super) fn breadcrumbs<M: SerializeMap>(
    map: &mut M,
    page: &PageData<'_>,
    discovery: Option<&Discovery<'_>>,
) -> Result<(), M::Error> {
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
                &CanonicalPage(canonical, &route.public_url),
            )?;
        } else {
            map.serialize_entry("mainEntityOfPage", &ArticlePage(page, discovery))?;
        }
    } else {
        trail(map, page, discovery)?;
        related(map, page, discovery)?;
    }
    Ok(())
}

/// A trail names where the page sits, so one that holds only the page itself says nothing.
fn trail<M: SerializeMap>(
    map: &mut M,
    page: &PageData<'_>,
    discovery: Option<&Discovery<'_>>,
) -> Result<(), M::Error> {
    let trails = page
        .breadcrumbs
        .iter()
        .filter(|trail| trail.items.len() > 1)
        .filter(|trail| discovery.is_none_or(|discovery| discovery.includes_trail(trail)));
    if trails.clone().next().is_some() {
        map.serialize_entry("breadcrumb", &Sequence(trails.map(Trail)))?;
    }
    Ok(())
}

fn related<M: SerializeMap>(
    map: &mut M,
    page: &PageData<'_>,
    discovery: Option<&Discovery<'_>>,
) -> Result<(), M::Error> {
    let links = page
        .related
        .iter()
        .filter(|value| discovery.is_none_or(|selection| selection.node(value.node).is_some()))
        .filter_map(|value| value.resolved_route.map(|route| route.url.as_str()));
    if links.clone().next().is_some() {
        map.serialize_entry("relatedLink", &Sequence(links))?;
    }
    Ok(())
}

struct ArticlePage<'a, 'data>(&'a PageData<'data>, Option<&'a Discovery<'data>>);
struct CanonicalPage<'a>(&'a gazette_core::publication::Key, &'a str);
impl Serialize for CanonicalPage<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("@id", self.1)?;
        map.serialize_entry("identifier", self.0)?;
        map.serialize_entry("@type", "WebPage")?;
        map.serialize_entry("url", self.1)?;
        map.end()
    }
}
/// The page that carries an article: its address, name and place. The article's own
/// facts stay on the article.
impl Serialize for ArticlePage<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.0.content.node;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", &public_identity::work(node, self.0.route, true))?;
        map.serialize_entry("@type", "WebPage")?;
        map.serialize_entry("name", &node.title)?;
        if let Some(route) = self.0.route {
            map.serialize_entry("url", &route.url)?;
        }
        trail(&mut map, self.0, self.1)?;
        related(&mut map, self.0, self.1)?;
        map.end()
    }
}

struct Trail<'a, 'data>(&'a BreadcrumbTrail<'data>);
impl Serialize for Trail<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Target {
            #[serde(rename = "@id")]
            identity: String,
        }
        #[derive(Serialize)]
        struct Item<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            position: usize,
            name: &'a str,
            item: Target,
        }
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("@type", "BreadcrumbList")?;
        map.serialize_entry("numberOfItems", &self.0.items.len())?;
        map.serialize_entry(
            "itemListElement",
            &Sequence(self.0.items.iter().enumerate().map(|(index, link)| Item {
                kind: "ListItem",
                position: index + 1,
                name: &link.node.title,
                item: Target {
                    identity: public_identity::work(link.node, link.route, true),
                },
            })),
        )?;
        map.end()
    }
}
