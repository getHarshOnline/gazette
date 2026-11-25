//! Linked-data projection of existing resolved page facts; no identity resolution or effects.

use crate::workspace::ResolvedRoute;
use gazette_core::{
    publication::{
        Key, Principal, PrincipalKind, Project, ProjectOrigin,
        content::{
            AttributionRole, ContentKind, ContentNode, EditorialDate, JourneyMember, PageFamily,
            ProjectRelation,
        },
    },
    source::Sha256Digest,
};
use serde::{Serialize, Serializer, ser::SerializeMap};

use super::{
    AttributionView, ClassificationView, ContentView, Discovery, JourneyMembership, PageProjection,
    ProjectDeclarationView, ProjectView,
};

mod faq;
mod navigation;
mod public;
mod subject;
pub use public::{PublicJsonLd, PublicListingJsonLd};

/// Borrowed local JSON-LD 1.1 projection, not public admission or identity evidence.
///
/// Standard vocabulary describes supported publication relationships. Gazette's
/// technical vocabulary retains distinctions that must not be coerced into
/// Schema.org ownership, offers or approval. Internal keys identify document-scoped
/// blank nodes; they are never reinterpreted as public IRIs or canonical URLs.
/// Serialization streams through the caller's chosen writer and byte allowance.
pub struct JsonLd<'a, 'data> {
    page: &'a PageProjection<'data>,
    discovery: Option<&'a Discovery<'data>>,
    listing: bool,
}

impl<'data> PageProjection<'data> {
    /// Project retained facts without another lookup, inheritance or Markdown pass.
    pub fn json_ld(&self) -> JsonLd<'_, '_> {
        JsonLd {
            page: self,
            discovery: None,
            listing: false,
        }
    }

    /// Retain direct authored declarations with discovery-safe generated navigation.
    pub fn discovery_json_ld<'a>(
        &'a self,
        discovery: &'a Discovery<'data>,
    ) -> Option<JsonLd<'a, 'data>> {
        discovery.owns(self.data.content.node).then_some(JsonLd {
            page: self,
            discovery: Some(discovery),
            listing: false,
        })
    }

    /// Project an eligible record for an automatic index, not its full direct-page declaration.
    /// Foreign-owner or undiscoverable records cannot enter this projection.
    pub fn listing_json_ld<'a>(
        &'a self,
        discovery: &'a Discovery<'data>,
    ) -> Option<JsonLd<'a, 'data>> {
        discovery.node(self.data.content.node)?;
        Some(JsonLd {
            page: self,
            discovery: Some(discovery),
            listing: true,
        })
    }
}

impl Serialize for JsonLd<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let page = &self.page.data;
        if let Some(discovery) = self.discovery {
            discovery
                .validate_page_projects(page, self.listing)
                .map_err(serde::ser::Error::custom)?;
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@context", &Context)?;
        content(&mut map, page.content.node)?;
        if let Some(listing) = &page.listing {
            map.serialize_entry("mainEntity", &listing.item_list())?;
        } else if let Some(subject) = &page.subject {
            map.serialize_entry("mainEntity", &subject::MainEntity(subject, None))?;
        }
        navigation::breadcrumbs(&mut map, page, self.discovery)?;
        if let Some(description) = &page.content.description {
            map.serialize_entry("description", description)?;
        }
        if let Some(summary) = &self.page.summary {
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
        map.serialize_entry("text", &self.page.body_text)?;
        let images = page.images.iter().filter(|image| !image.presentation_only);
        if images.clone().next().is_some() {
            map.serialize_entry("image", &Sequence(images.map(Image)))?;
        }
        if let Some(route) = page.route {
            map.serialize_entry("url", &route.url)?;
            if let Some(canonical) = page
                .content
                .route
                .as_ref()
                .and_then(|route| route.canonical.as_ref())
            {
                map.serialize_entry("gazette:canonical", &Reference("content", canonical))?;
            }
            map.serialize_entry("gazette:route", &Literal(route))?;
        }
        map.serialize_entry("publisher", &Reference("principal", page.site.publisher))?;
        map.serialize_entry(
            "gazette:siteRoles",
            &List(Sequence(page.site.roles.iter().map(|role| SiteRole {
                site: page.site.id,
                role,
            }))),
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
                            .map(|value| PrincipalNode(value.principal, value.website.as_ref())),
                    ),
                )?;
            }
        }
        map.serialize_entry(
            "gazette:attributions",
            &List(Sequence(page.attributions.iter().map(Attribution))),
        )?;
        let projects = page.projects.iter().filter(|project| {
            self.discovery
                .filter(|_| self.listing)
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
                            .map(|value| ProjectNode(value.project, value.resources.as_ref())),
                    ),
                )?;
            }
        }
        map.serialize_entry(
            "gazette:projectRelations",
            &Sequence(projects.map(Relationship)),
        )?;
        map.serialize_entry(
            "gazette:projectDeclarations",
            &Sequence(page.project_declarations.iter().map(Declaration)),
        )?;
        map.serialize_entry(
            "isPartOf",
            &Sequence(
                page.collections
                    .iter()
                    .filter(|view| {
                        self.discovery
                            .is_none_or(|discovery| discovery.node(view.node).is_some())
                    })
                    .map(|view| (view.node, view.resolved_route))
                    .chain(
                        page.journeys
                            .iter()
                            .filter(|value| {
                                self.discovery
                                    .is_none_or(|discovery| discovery.includes_journey(value))
                            })
                            .map(|value| (value.journey, value.route)),
                    )
                    .map(|(node, route)| Content(node, route)),
            ),
        )?;
        map.serialize_entry(
            "gazette:collections",
            &Sequence(
                page.collections
                    .iter()
                    .filter(|view| {
                        self.discovery
                            .is_none_or(|discovery| discovery.node(view.node).is_some())
                    })
                    .map(|view| (view.node, view.resolved_route))
                    .map(|(node, route)| Content(node, route)),
            ),
        )?;
        map.serialize_entry(
            "gazette:journeys",
            &Sequence(
                page.journeys
                    .iter()
                    .filter(|value| {
                        self.discovery
                            .is_none_or(|discovery| discovery.includes_journey(value))
                    })
                    .map(Membership),
            ),
        )?;
        map.serialize_entry(
            "hasPart",
            &faq::Parts {
                page: self.page,
                discovery: self.discovery,
                include_faq: !self.listing,
            },
        )?;
        map.serialize_entry(
            "gazette:editorialRelated",
            &Sequence(
                page.related
                    .iter()
                    .filter(|view| {
                        self.discovery
                            .is_none_or(|discovery| discovery.node(view.node).is_some())
                    })
                    .map(|view| (view.node, view.resolved_route))
                    .map(|(node, route)| Content(node, route)),
            ),
        )?;
        if let ContentKind::Journey { members, .. } = &page.content.structure {
            map.serialize_entry(
                "mainEntity",
                &Journey(
                    members,
                    &page.steps,
                    self.discovery.filter(|_| self.listing),
                ),
            )?;
        }
        map.serialize_entry(
            "gazette:classifications",
            &Sequence(page.classifications.iter().map(Classification)),
        )?;
        if let Some(source) = page.provenance {
            map.serialize_entry("gazette:provenance", &Provenance(source))?;
        }
        if let Some(derivation) = page.derivation {
            map.serialize_entry("gazette:derivation", &Literal(derivation))?;
        }
        // These are exact rendering coordinates, not independently asserted RDF
        // identities, MIME claims or a reconstructed document structure.
        if let Some(discovery) = self.discovery.filter(|_| self.listing) {
            let node = discovery
                .node(page.content.node)
                .ok_or_else(|| serde::ser::Error::custom("JSON-LD discovery owner mismatch"))?;
            map.serialize_entry("gazette:structure", &Literal(&node.structure()))?;
        } else {
            map.serialize_entry("gazette:structure", &Literal(&page.content.structure))?;
        }
        map.serialize_entry("gazette:headings", &Literal(page.headings))?;
        map.end()
    }
}

/// Authored image occurrences, independent of the site's transport inventory.
struct Image<'a>(&'a crate::markdown::ImageUse);
impl Serialize for Image<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@type", "ImageObject")?;
        map.serialize_entry("contentUrl", &self.0.destination)?;
        if !self.0.alt.is_empty() {
            map.serialize_entry("description", &self.0.alt)?;
        }
        if let Some(title) = &self.0.title {
            map.serialize_entry("name", title)?;
        }
        if let Some(caption) = &self.0.caption {
            map.serialize_entry("caption", caption)?;
        }
        map.end()
    }
}

pub(super) struct Context;
struct DateLiteral<'a>(&'a EditorialDate);
impl Serialize for DateLiteral<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@value", self.0)?;
        map.serialize_entry(
            "@type",
            if self.0.is_date_only() {
                "http://www.w3.org/2001/XMLSchema#date"
            } else {
                "http://www.w3.org/2001/XMLSchema#dateTime"
            },
        )?;
        map.end()
    }
}

impl Serialize for Context {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct UrlTerm {
            #[serde(rename = "@id")]
            id: &'static str,
            #[serde(rename = "@type")]
            kind: &'static str,
        }
        let mut map = serializer.serialize_map(Some(4))?;
        map.serialize_entry("@version", &1.1)?;
        map.serialize_entry("@vocab", "https://schema.org/")?;
        // Only resolved canonical URLs are IRIs. Configured keys stay literals.
        map.serialize_entry(
            "url",
            &UrlTerm {
                id: "https://schema.org/url",
                kind: "@id",
            },
        )?;
        // A technical term namespace, not a registered URN namespace, resolvable
        // public schema endpoint or authority for any referenced subject.
        map.serialize_entry("gazette", "urn:gazette:publication:")?;
        map.end()
    }
}

// Serialization adapters borrow their inputs. They neither clone a second model
// nor materialize unbounded intermediate JSON Values before the output writer.
struct Sequence<I>(I);
impl<I: Clone + Iterator> Serialize for Sequence<I>
where
    I::Item: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.clone())
    }
}

struct List<T>(T);
impl<T: Serialize> Serialize for List<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@list", &self.0)?;
        map.end()
    }
}

struct Literal<'a, T: ?Sized>(&'a T);
impl<T: ?Sized + Serialize> Serialize for Literal<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("@type", "@json")?;
        map.serialize_entry("@value", self.0)?;
        map.end()
    }
}

pub(super) struct Reference<'a>(pub(super) &'static str, pub(super) &'a Key);
fn identity<M: SerializeMap>(map: &mut M, space: &str, id: &Key) -> Result<(), M::Error> {
    map.serialize_entry(
        "@id",
        &format!("_:{space}_{}", Sha256Digest::of(id.as_str().as_bytes())),
    )?;
    map.serialize_entry("identifier", id)
}
impl Serialize for Reference<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        identity(&mut map, self.0, self.1)?;
        map.end()
    }
}

fn content<M: SerializeMap>(map: &mut M, node: &ContentNode) -> Result<(), M::Error> {
    identity(map, "content", &node.id)?;
    match &node.structure {
        ContentKind::Page {
            family: PageFamily::Article,
        } => {
            map.serialize_entry("headline", &node.title)?;
            map.serialize_entry(
                "@type",
                &node
                    .schema_type
                    .unwrap_or(gazette_core::publication::content::ArticleType::Article),
            )?;
        }
        ContentKind::Collection { .. } | ContentKind::Journey { .. } => {
            map.serialize_entry("@type", "CollectionPage")?
        }
        ContentKind::Page {
            family: PageFamily::Utility,
        } if node.schema_type
            == Some(gazette_core::publication::content::SchemaType::ProfilePage) =>
        {
            map.serialize_entry("@type", "ProfilePage")?;
        }
        ContentKind::Page {
            family: PageFamily::Utility,
        } if node.schema_type
            == Some(gazette_core::publication::content::SchemaType::AboutPage) =>
        {
            map.serialize_entry("@type", "AboutPage")?;
        }
        ContentKind::Page { .. } => map.serialize_entry("@type", "WebPage")?,
    }
    content_facts(map, node)
}

fn content_facts<M: SerializeMap>(map: &mut M, node: &ContentNode) -> Result<(), M::Error> {
    map.serialize_entry("name", &node.title)?;
    if !node.keywords.is_empty() {
        map.serialize_entry("keywords", &node.keywords)?;
    }
    if let Some(license) = &node.license {
        #[derive(Serialize)]
        struct License<'a> {
            #[serde(rename = "@id")]
            uri: &'a str,
        }
        map.serialize_entry(
            "license",
            &License {
                uri: license.as_str(),
            },
        )?;
    }
    map.serialize_entry("creativeWorkStatus", &node.lifecycle)?;
    if let Some(language) = &node.language {
        map.serialize_entry("inLanguage", language)?;
    }
    map.serialize_entry("gazette:site", &Reference("site", &node.site))
}
struct Content<'a>(&'a ContentNode, Option<&'a ResolvedRoute>);
impl Serialize for Content<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        content(&mut map, self.0)?;
        if let Some(route) = self.1 {
            map.serialize_entry("url", &route.url)?;
        }
        map.end()
    }
}

struct PrincipalNode<'a>(&'a Principal, Option<&'a super::PrincipalWebsiteView>);

/// Principals one public document has already described; later mentions name them by
/// identity alone, so a person who is both publisher and author is described once.
#[derive(Default)]
struct Described(std::cell::RefCell<Vec<String>>);

/// A principal mentioned within one document, described in full only the first time.
struct PrincipalMention<'a>(PrincipalNode<'a>, Option<&'a Described>);

impl<'a> PrincipalNode<'a> {
    /// A principal with a configured website is one public entity across every page and
    /// site: its website address plus a role fragment. Without one it keeps a
    /// document-local blank node.
    fn identity(&self) -> String {
        match self.1 {
            Some(website) => format!(
                "{}#{}",
                website.identity_url,
                match self.0.kind {
                    PrincipalKind::Person => "person",
                    PrincipalKind::Organization => "organization",
                }
            ),
            None => format!(
                "_:principal_{}",
                Sha256Digest::of(self.0.id.as_str().as_bytes())
            ),
        }
    }

    fn within(self, described: Option<&'a Described>) -> PrincipalMention<'a> {
        PrincipalMention(self, described)
    }
}

impl Serialize for PrincipalMention<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(described) = self.1 else {
            return self.0.serialize(serializer);
        };
        let identity = self.0.identity();
        if described.0.borrow().contains(&identity) {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("@id", &identity)?;
            return map.end();
        }
        described.0.borrow_mut().push(identity);
        self.0.serialize(serializer)
    }
}

impl PrincipalNode<'_> {
    fn write_fields<M: SerializeMap>(&self, map: &mut M) -> Result<(), M::Error> {
        let kind = match self.0.kind {
            PrincipalKind::Person => "Person",
            PrincipalKind::Organization => "Organization",
        };
        map.serialize_entry("@id", &self.identity())?;
        map.serialize_entry("identifier", &self.0.id)?;
        map.serialize_entry("@type", kind)?;
        if let Some(name) = &self.0.name {
            map.serialize_entry("name", name)?;
        }
        if let Some(website) = self.1 {
            map.serialize_entry("url", &website.canonical_url)?;
        }
        if !self.0.same_as.is_empty() {
            map.serialize_entry(
                "sameAs",
                &Sequence(self.0.same_as.iter().map(|url| IdentityUrl(url))),
            )?;
        }
        Ok(())
    }
}

impl Serialize for PrincipalNode<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.write_fields(&mut map)?;
        map.end()
    }
}
#[derive(Clone)]
struct IdentityUrl<'a>(&'a str);
impl Serialize for IdentityUrl<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("@id", self.0)?;
        map.end()
    }
}
struct Attribution<'a, 'data>(&'a AttributionView<'data>);
impl Serialize for Attribution<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry(
            "gazette:principal",
            &PrincipalNode(self.0.principal, self.0.website.as_ref()),
        )?;
        map.serialize_entry("gazette:role", &self.0.role)?;
        map.end()
    }
}

struct SiteRole<'a, 'data> {
    site: &'data Key,
    role: &'a super::PublicationRoleView<'data>,
}
impl Serialize for SiteRole<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("gazette:site", &Reference("site", self.site))?;
        map.serialize_entry(
            "gazette:principal",
            &PrincipalNode(self.role.principal, self.role.website.as_ref()),
        )?;
        map.serialize_entry("gazette:role", &self.role.role)?;
        map.end()
    }
}

struct ProjectNode<'a>(
    &'a Project,
    Option<&'a gazette_core::markdown::ProjectResourcesView>,
);
impl Serialize for ProjectNode<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(
            6 + usize::from(self.0.name.is_some())
                + usize::from(self.0.kind.is_some())
                + usize::from(self.0.identity.is_some())
                + usize::from(self.0.repository.is_some())
                + usize::from(self.0.license.is_some())
                + usize::from(self.0.origin.is_some())
                + usize::from(self.0.available_through.is_some())
                + usize::from(self.1.is_some()),
        ))?;
        identity(&mut map, "project", &self.0.id)?;
        map.serialize_entry("@type", "gazette:Project")?;
        if let Some(name) = &self.0.name {
            map.serialize_entry("name", name)?;
        }
        if let Some(repository) = &self.0.repository {
            map.serialize_entry("codeRepository", repository)?;
        }
        if let Some(resources) = self.1 {
            let related: Vec<_> = resources
                .actions
                .iter()
                .filter(|action| {
                    use gazette_core::markdown::ProjectResourceKind::{Github, License, Notice};
                    !matches!(action.kind, Github | License | Notice)
                })
                .map(|action| serde_json::json!({"@id":action.url}))
                .collect();
            map.serialize_entry("subjectOf", &related)?;
        }
        if let Some(license) = &self.0.license {
            map.serialize_entry("license", license)?;
        }
        map.serialize_entry("gazette:lifecycle", &self.0.lifecycle)?;
        if let Some(kind) = self.0.kind {
            map.serialize_entry("gazette:subjectKind", &kind)?;
        }
        if let Some(identity) = &self.0.identity {
            map.serialize_entry("gazette:identity", identity)?;
        }
        map.serialize_entry("gazette:visibility", &self.0.visibility)?;
        map.serialize_entry("gazette:availability", &self.0.availability)?;
        if let Some(through) = &self.0.available_through {
            map.serialize_entry("gazette:availableThrough", &AvailableThrough(through))?;
        }
        if let Some(origin) = &self.0.origin {
            map.serialize_entry("gazette:origin", &Origin(origin))?;
        }
        map.end()
    }
}
struct AvailableThrough<'a>(&'a gazette_core::publication::ProjectAvailability);
impl Serialize for AvailableThrough<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("gazette:site", &Reference("site", &self.0.site))?;
        map.serialize_entry("gazette:content", &Reference("content", &self.0.content))?;
        map.end()
    }
}
struct Origin<'a>(&'a ProjectOrigin);
impl Serialize for Origin<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("gazette:kind", self.0.kind())?;
        if let Some(upstream) = self.0.upstream() {
            map.serialize_entry("gazette:upstream", &Reference("project", upstream))?;
        }
        map.end()
    }
}
struct Relationship<'a, 'data>(&'a ProjectView<'data>);
impl Serialize for Relationship<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Evidence<'a> {
            #[serde(rename = "gazette:source")]
            source: Reference<'a>,
            #[serde(rename = "gazette:kind")]
            kind: gazette_core::publication::content::ProjectDeclarationKind,
        }
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry(
            "gazette:project",
            &ProjectNode(self.0.project, self.0.resources.as_ref()),
        )?;
        map.serialize_entry("gazette:relation", &self.0.relationship.relation())?;
        map.serialize_entry(
            "gazette:evidence",
            &Sequence(self.0.relationship.evidence().iter().map(|value| Evidence {
                source: Reference("content", &value.source),
                kind: value.kind,
            })),
        )?;
        map.end()
    }
}
struct Declaration<'a, 'data>(&'a ProjectDeclarationView<'data>);
impl Serialize for Declaration<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("gazette:project", &ProjectNode(self.0.project, None))?;
        map.serialize_entry(
            "gazette:source",
            &Reference("content", &self.0.declaration.content),
        )?;
        map.serialize_entry("gazette:kind", &self.0.declaration.kind)?;
        map.end()
    }
}

struct Membership<'a, 'data>(&'a JourneyMembership<'data>);
impl Serialize for Membership<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("gazette:journey", &Content(self.0.journey, self.0.route))?;
        map.serialize_entry("gazette:primary", &self.0.primary)?;
        map.serialize_entry("gazette:position", &self.0.position)?;
        map.serialize_entry("gazette:total", &self.0.total)?;
        if let Some(route) = self.0.route {
            map.serialize_entry("url", &route.canonical_url)?;
        }
        if let Some(previous) = &self.0.previous {
            map.serialize_entry("gazette:previous", &navigation::PageLink(previous))?;
        }
        if let Some(next) = &self.0.next {
            map.serialize_entry("gazette:next", &navigation::PageLink(next))?;
        }
        map.end()
    }
}
struct JourneyContent<'a>(&'a Key, &'a [ContentView<'a>]);
impl Serialize for JourneyContent<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let endpoint = self
            .1
            .iter()
            .find(|step| &step.id == self.0)
            .ok_or_else(|| {
                serde::ser::Error::custom("resolved journey member endpoint is missing")
            })?;
        Content(endpoint.node, endpoint.resolved_route).serialize(serializer)
    }
}
struct Journey<'a>(
    &'a [JourneyMember],
    &'a [ContentView<'a>],
    Option<&'a Discovery<'a>>,
);
impl Serialize for Journey<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Item<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            position: usize,
            item: JourneyContent<'a>,
            #[serde(rename = "gazette:primary")]
            primary: bool,
        }
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("@type", "ItemList")?;
        let selected = self.0.iter().enumerate().filter(|(_, value)| {
            self.2
                .is_none_or(|discovery| discovery.contains(&value.content))
        });
        map.serialize_entry("numberOfItems", &selected.clone().count())?;
        map.serialize_entry(
            "itemListElement",
            &Sequence(selected.map(|(index, value)| Item {
                kind: "ListItem",
                position: index + 1,
                item: JourneyContent(&value.content, self.1),
                primary: value.primary,
            })),
        )?;
        map.end()
    }
}

struct Classification<'a, 'data>(&'a ClassificationView<'data>);
impl Serialize for Classification<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Taxonomy<'a> {
            #[serde(flatten)]
            identity: Reference<'a>,
            #[serde(rename = "@type")]
            kind: &'static str,
            name: &'a str,
        }
        let value = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry(
            "@id",
            &format!(
                "_:term_{}_{}",
                Sha256Digest::of(value.taxonomy.id.as_str().as_bytes()),
                Sha256Digest::of(value.term.id.as_str().as_bytes())
            ),
        )?;
        map.serialize_entry("@type", "DefinedTerm")?;
        map.serialize_entry("identifier", &value.term.id)?;
        map.serialize_entry("name", &value.term.label)?;
        map.serialize_entry(
            "inDefinedTermSet",
            &Taxonomy {
                identity: Reference("taxonomy", value.taxonomy.id),
                kind: "DefinedTermSet",
                name: value.taxonomy.label,
            },
        )?;
        if let Some(language) = value.taxonomy.language {
            map.serialize_entry("gazette:languageScope", language)?;
        }
        map.end()
    }
}

struct Provenance<'a>(crate::workspace::ContentSource<'a>);
impl Serialize for Provenance<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("gazette:site", &Reference("site", self.0.site))?;
        if let Some(project) = self.0.project {
            map.serialize_entry("gazette:project", &Reference("project", project))?;
        }
        map.end()
    }
}
