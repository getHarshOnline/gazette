//! Schema.org publication facts, borrowed independently from native inspection metadata.

mod brands;
mod navigation;
mod parts;
mod terms;

use super::{
    AttributionRole, ContentKind, ContentNode, DateLiteral, Described, Discovery, IdentityUrl,
    Image, JsonLd, PageFamily, PrincipalNode, ProjectRelation, Reference, Sequence, subject,
};
use crate::authored::PageData;
use crate::authored::public_identity;
use crate::{authored::ListingPage, workspace::ResolvedRoute};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// Standard linked data for a page, with the same discovery selection as its native view.
/// Internal source bindings, relationship evidence and operational state stay in that view.
pub struct PublicJsonLd<'a, 'data> {
    view: JsonLd<'a, 'data>,
    generation: Option<(&'a str, Option<&'a str>)>,
    include_actions: bool,
}

impl<'a, 'data> JsonLd<'a, 'data> {
    /// Select public Schema.org facts without changing the page or its discovery boundaries.
    pub fn public(self) -> PublicJsonLd<'a, 'data> {
        PublicJsonLd {
            include_actions: !self.listing,
            view: self,
            generation: None,
        }
    }
}

impl<'a> PublicJsonLd<'a, '_> {
    /// Carry an authored page's actions when representing it in the site's discovery graph.
    pub fn with_authored_actions(mut self) -> Self {
        self.include_actions = true;
        self
    }

    /// Attach the selected footer's generator and optional theme to the public graph.
    pub fn with_generation(mut self, generation: Option<(&'a str, Option<&'a str>)>) -> Self {
        self.generation = generation;
        self
    }
}

impl Serialize for PublicJsonLd<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let page = &self.view.page.data;
        let website = Website::for_home(page);
        if website.is_none() && page.brand_graph.is_none() {
            return PublicPage {
                view: &self.view,
                include_context: true,
                include_actions: self.include_actions,
                generation: self.generation,
            }
            .serialize(serializer);
        }
        let data = page;
        let page = PublicPage {
            view: &self.view,
            include_context: false,
            include_actions: self.include_actions,
            generation: self.generation,
        };
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@context", &Context)?;
        map.serialize_entry("@graph", &PublicationGraph(page, website, data))?;
        map.end()
    }
}

struct PublicationGraph<'a, 'view, 'data>(
    PublicPage<'a, 'view, 'data>,
    Option<Website<'a, 'data>>,
    &'a PageData<'data>,
);

impl Serialize for PublicationGraph<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let mut sequence = serializer.serialize_seq(None)?;
        sequence.serialize_element(&self.0)?;
        if let Some(website) = &self.1 {
            sequence.serialize_element(website)?;
        }
        if let Some(graph) = &self.2.brand_graph {
            brands::append(&mut sequence, graph)?;
        }
        sequence.end()
    }
}

struct PublicPage<'a, 'view, 'data> {
    view: &'a JsonLd<'view, 'data>,
    include_context: bool,
    include_actions: bool,
    generation: Option<(&'a str, Option<&'a str>)>,
}

impl Serialize for PublicPage<'_, '_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let view = self.view;
        let page = &view.page.data;
        if let Some(discovery) = view.discovery {
            discovery
                .validate_page_projects(page, view.listing)
                .map_err(serde::ser::Error::custom)?;
        }
        let described = Described::default();
        if let Some(owner) = page.brand_graph.as_ref().and_then(brands::owner) {
            described.0.borrow_mut().push(owner.identity());
        }
        let mut map = serializer.serialize_map(None)?;
        if self.include_context {
            map.serialize_entry("@context", &Context)?;
        }
        header(
            &mut map,
            page.content.node,
            page.route,
            page.content.listing,
        )?;
        // Site terms describe use of every page; the content licence never inherits them.
        if let Some(terms) = &page.site.terms_url {
            map.serialize_entry("usageInfo", &super::IdentityUrl(terms))?;
        }
        if let Some((generator, theme)) = self.generation {
            map.serialize_entry(
                "http://www.w3.org/ns/prov#wasGeneratedBy",
                &serde_json::json!({
                    "@type": "http://www.w3.org/ns/prov#Activity",
                    "http://www.w3.org/ns/prov#used": std::iter::once(generator)
                        .chain(theme)
                        .map(|url| serde_json::json!({"@id": url}))
                        .collect::<Vec<_>>()
                }),
            )?;
        }
        if !page.content.keywords.is_empty() || !page.classifications.is_empty() {
            map.serialize_entry("keywords", &terms::Keywords(page))?;
        }
        if let Some(listing) = &page.listing {
            map.serialize_entry("mainEntity", &listing.public_item_list())?;
        } else if let Some(subject) = &page.subject {
            map.serialize_entry(
                "mainEntity",
                &subject::MainEntity(subject, Some(&described)),
            )?;
        } else if page
            .site
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.about)
            == Some(&page.content.node.id)
            && let Some(graph) = &page.brand_graph
        {
            map.serialize_entry("mainEntity", &IdentityUrl(&graph.current.identity))?;
        }
        navigation::breadcrumbs(&mut map, page, view.discovery)?;
        if let Some(description) = &page.content.description {
            map.serialize_entry("description", description)?;
        }
        if let Some(citation) = page.citation.as_ref().and_then(|citation| citation.text) {
            map.serialize_entry("citation", citation)?;
        }
        if self.include_actions {
            let actions = view
                .page
                .component_uses()
                .flat_map(|component| component.actions.iter().map(String::as_str))
                .chain(
                    page.invitations
                        .values()
                        .map(|invitation| invitation.destination.route.public_url.as_str()),
                )
                .filter(|target| {
                    target.starts_with("http://")
                        || target.starts_with("https://")
                        || target.starts_with("mailto:")
                })
                .collect::<std::collections::BTreeSet<_>>();
            if !actions.is_empty() {
                map.serialize_entry("potentialAction", &Sequence(actions.iter().copied().map(|target| {
                    serde_json::json!({
                        "@type": if target.starts_with("mailto:") { "CommunicateAction" } else { "ViewAction" },
                        "target": target,
                    })
                })))?;
            }
        }
        if let Some(summary) = &view.page.summary {
            map.serialize_entry("abstract", &summary.text)?;
        }
        for (name, date) in [
            ("datePublished", &page.content.published),
            ("dateModified", &page.content.modified),
        ] {
            if let Some(date) = date {
                map.serialize_entry(name, &DateLiteral(date))?;
            }
        }
        map.serialize_entry("text", &view.page.body_text)?;
        let images = page.images.iter().filter(|image| !image.presentation_only);
        if images.clone().next().is_some() {
            map.serialize_entry("image", &Sequence(images.map(Image)))?;
        }
        if let Some(route) = page.route {
            map.serialize_entry("url", &route.url)?;
        }
        map.serialize_entry(
            "publisher",
            &PrincipalNode(
                page.site.publisher_principal,
                page.site.publisher_website.as_ref(),
            )
            .within(Some(&described)),
        )?;
        for (name, role) in [
            ("author", AttributionRole::Author),
            ("copyrightHolder", AttributionRole::RightsHolder),
        ] {
            if page.attributions.iter().any(|value| value.role == role) {
                map.serialize_entry(
                    name,
                    &Sequence(
                        page.attributions
                            .iter()
                            .filter(move |value| value.role == role)
                            .map(|value| {
                                PrincipalNode(value.principal, value.website.as_ref())
                                    .within(Some(&described))
                            }),
                    ),
                )?;
            }
        }
        let projects = page.projects.iter().filter(|project| {
            view.discovery
                .filter(|_| view.listing)
                .is_none_or(|discovery| discovery.includes_project(project))
        });
        for (name, relation) in [
            ("about", ProjectRelation::About),
            ("mentions", ProjectRelation::Mentions),
        ] {
            if projects
                .clone()
                .any(|value| value.relationship.relation() == relation)
            {
                map.serialize_entry(
                    name,
                    &Sequence(
                        projects
                            .clone()
                            .filter(move |value| value.relationship.relation() == relation)
                            .map(|project| terms::Project(project, &described)),
                    ),
                )?;
            }
        }
        let wholes = page
            .collections
            .iter()
            .filter(|value| {
                view.discovery
                    .is_none_or(|discovery| discovery.node(value.node).is_some())
            })
            .map(|value| Content(value.node, value.resolved_route, value.listing))
            .chain(
                page.journeys
                    .iter()
                    .filter(|value| {
                        view.discovery
                            .is_none_or(|discovery| discovery.includes_journey(value))
                    })
                    .map(|value| Content(value.journey, value.route, false)),
            );
        if wholes.clone().next().is_some() {
            map.serialize_entry("isPartOf", &Sequence(wholes))?;
        }
        let parts = parts::Parts {
            page: view.page,
            discovery: view.discovery,
            include_faq: !view.listing,
        };
        if !parts.is_empty() {
            map.serialize_entry("hasPart", &parts)?;
        }
        if let ContentKind::Journey { members, .. } = &page.content.structure {
            map.serialize_entry(
                "mainEntity",
                &parts::Journey(
                    members,
                    &page.steps,
                    view.discovery.filter(|_| view.listing),
                ),
            )?;
        }
        map.end()
    }
}

struct Website<'a, 'data> {
    page: &'a PageData<'data>,
    name: &'a str,
    url: &'a str,
}

impl<'a, 'data> Website<'a, 'data> {
    fn for_home(page: &'a PageData<'data>) -> Option<Self> {
        let metadata = page.site.metadata.as_ref()?;
        let home = metadata.home.as_ref()?;
        if home.id != &page.content.node.id {
            return None;
        }
        Some(Self {
            page,
            name: metadata.brand.name,
            url: &page.route?.public_url,
        })
    }
}

impl Serialize for Website<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let site = &self.page.site;
        let publisher = PrincipalNode(site.publisher_principal, site.publisher_website.as_ref());
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", &format!("{}#website", self.url))?;
        map.serialize_entry("@type", "WebSite")?;
        map.serialize_entry("name", self.name)?;
        map.serialize_entry("url", self.url)?;
        map.serialize_entry("publisher", &IdentityUrl(&publisher.identity()))?;
        let subjects = self.page.projects.iter().filter(|project| {
            project.relationship.relation() == ProjectRelation::About
                && project.project.visibility == gazette_core::publication::Visibility::Public
        });
        let brand = self
            .page
            .brand_graph
            .as_ref()
            .map(|graph| IdentityUrl(&graph.current.identity));
        if brand.is_some() || subjects.clone().next().is_some() {
            map.serialize_entry(
                "about",
                &Sequence(
                    brand
                        .into_iter()
                        .chain(subjects.map(|project| IdentityUrl(&project.identity))),
                ),
            )?;
        }
        if let Some(terms) = &site.terms_url {
            map.serialize_entry("usageInfo", &IdentityUrl(terms))?;
        }
        map.end()
    }
}

struct Context;
impl Serialize for Context {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct UrlTerm {
            #[serde(rename = "@id")]
            id: &'static str,
            #[serde(rename = "@type")]
            kind: &'static str,
        }
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("@version", &1.1)?;
        map.serialize_entry("@vocab", "https://schema.org/")?;
        map.serialize_entry(
            "url",
            &UrlTerm {
                id: "https://schema.org/url",
                kind: "@id",
            },
        )?;
        map.end()
    }
}

/// The page's own work, described once per document.
fn header<M: SerializeMap>(
    map: &mut M,
    node: &ContentNode,
    route: Option<&ResolvedRoute>,
    listing: bool,
) -> Result<(), M::Error> {
    reference(map, node, route, listing)?;
    if matches!(
        node.structure,
        ContentKind::Page {
            family: PageFamily::Article
        }
    ) {
        map.serialize_entry("headline", &node.title)?;
    }
    if let Some(license) = &node.license {
        map.serialize_entry("license", &super::IdentityUrl(license.as_str()))?;
    }
    map.serialize_entry("creativeWorkStatus", &node.lifecycle)?;
    if let Some(language) = &node.language {
        map.serialize_entry("inLanguage", language)?;
    }
    Ok(())
}

/// The identity, kind and name another work needs to mention this one; its facts stay
/// with its own page, as in a listing's item list.
fn reference<M: SerializeMap>(
    map: &mut M,
    node: &ContentNode,
    route: Option<&ResolvedRoute>,
    listing: bool,
) -> Result<(), M::Error> {
    map.serialize_entry("@id", &public_identity::work(node, route, false))?;
    map.serialize_entry("identifier", &node.id)?;
    map.serialize_entry("@type", public_identity::work_type(node, false, listing))?;
    map.serialize_entry("name", &node.title)
}

struct Content<'a>(&'a ContentNode, Option<&'a ResolvedRoute>, bool);
impl Serialize for Content<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        reference(&mut map, self.0, self.1, self.2)?;
        if let Some(route) = self.1 {
            map.serialize_entry("url", &route.url)?;
        }
        map.end()
    }
}

/// Standard fields for one generated listing view, borrowing its existing ordered items.
pub struct PublicListingJsonLd<'a>(&'a ListingPage);

impl ListingPage {
    /// Project a generated view without exposing its native route bookkeeping.
    pub fn public_json_ld(&self) -> PublicListingJsonLd<'_> {
        PublicListingJsonLd(self)
    }
}

impl Serialize for PublicListingJsonLd<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let page = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@context", &Context)?;
        map.serialize_entry("@type", "CollectionPage")?;
        map.serialize_entry("@id", &page.canonical_url)?;
        map.serialize_entry("identifier", &page.content)?;
        map.serialize_entry("name", &page.title)?;
        map.serialize_entry("url", &page.url)?;
        map.serialize_entry("mainEntity", &page.public_item_list())?;
        if let Some(subject) = &page.subject {
            let mut entity = serde_json::json!({"@id":subject.identity,"@type":public_identity::project_type(subject.kind),"identifier":subject.project});
            if let Some(name) = &subject.name {
                entity["name"] = serde_json::json!(name);
            }
            map.serialize_entry("about", &entity)?;
            map.serialize_entry(
                "relatedLink",
                &Sequence(
                    std::iter::once(subject.landing.url.as_str())
                        .chain(subject.explore.as_ref().map(|value| value.url.as_str())),
                ),
            )?;
        }
        map.end()
    }
}
