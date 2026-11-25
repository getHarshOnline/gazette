//! Publication-owned listing views over existing, source-bound content identities.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    fmt, io,
};

use gazette_core::{
    publication::{
        Key, LanguageTag, Principal,
        content::{AttributionRole, ContentKind, ContentNode, EditorialDate, PageFamily},
        routing::RoutePath,
    },
    render::LimitedWriter,
    source::EntryPath,
};
use serde::Serialize;

use super::{BrowsingKind, BrowsingLocation, Discovery, ResolvedAuthored, SitePresentation};
use crate::workspace::{
    ListingDirection, ListingOrder, ListingSelection, ListingVariant, ResolvedRoute,
};

/// One shallow existing-content reference, with no body or structural expansion.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingLink {
    /// Canonical public identity and actual structured-data kind of the referenced work.
    pub identity: String,
    /// Actual structured-data type of this referenced work.
    pub work_type: String,
    /// Optional owner-selected decorative image URLs, never private source coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<super::ListingArtwork>,
    /// Stable identity of the existing selected object.
    pub id: Key,
    /// Exact publication owning the referenced article or collection.
    pub site: Key,
    /// Native destination facts relative to the index's owning site.
    pub destination: crate::markdown::LinkDestination,
    /// Literal authored title, not generated visible copy.
    pub title: String,
    /// Literal authored description, absent rather than inferred from the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Existing canonical serving address, without aliases or mirrors.
    pub url: String,
    /// Exact authored language, with no site-language fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
    /// Explicit authored publication chronology, retaining its precision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<EditorialDate>,
    /// Explicit authored modification chronology; never filesystem time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified: Option<EditorialDate>,
    /// Exact direct attribution order, resolved through the shared principal catalog.
    /// Empty declarations never acquire an author from the listing or publisher.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributions: Vec<ListingAttribution>,
    /// Selected destination Theme identity, independent from URL or content kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
}

/// Owned shallow role metadata for a listing item, using the page attribution shape.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingAttribution {
    /// Same configured principal value as the directly rendered page.
    pub principal: Principal,
    /// Optional selected serving/public website pair, never a guessed profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<super::PrincipalWebsiteView>,
    /// Role declared on the item itself.
    pub role: AttributionRole,
}

/// A bounded output view; continuations retain the original index identity.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingPage {
    /// Public identity independent from the selected serving endpoint.
    pub canonical_url: String,
    /// Existing index identity, shared by every continuation.
    pub content: Key,
    /// Exact title of that retained index.
    pub title: String,
    /// Compiler-resolved site-relative route for this output view.
    pub route: RoutePath,
    /// Serving address through the selected public or validated preview base.
    pub url: String,
    /// Literal output file without a duplicated deployment prefix.
    pub output: EntryPath,
    /// One-based ordinal; the existing index is page one.
    pub page_number: usize,
    /// Configured maximum items per page, shared by native slicing and enhancement.
    #[schemars(range(min = 1))]
    pub page_size: usize,
    /// Exact configured ordering view, never another Content identity.
    pub variant: ListingVariant,
    /// Authored chronology direction; absent for priority-based lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<ListingDirection>,
    /// Bounded page endpoints: first, last and the current ordinal plus/minus two.
    pub pages: Vec<ListingPageLink>,
    /// Available opposite chronology endpoints, each resetting to page one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sorts: Vec<ListingSortLink>,
    /// Complete number of views, including one view for an empty selection.
    pub page_count: usize,
    /// Complete eligible item count before slicing.
    pub total_items: usize,
    /// Latest dated eligible item under the index's editorial-day convention, independent
    /// of this slice or its display order. Equal dates use title then ID.
    /// Absent with no dated items or mixed precision on the newest editorial day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<ListingLink>,
    /// Explicit project subject, absent on unfiltered indexes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<ListingSubject>,
    /// Previous serving endpoint, absent on the first page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    /// Next serving endpoint, absent on the last page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// References in the single compiler-proved order for this slice.
    pub items: Vec<ListingLink>,
}

/// Explicit project context of one filtered index.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingSubject {
    /// Same public entity address and declared kind used on directly rendered pages.
    pub identity: String,
    /// Explicit subject kind; omission remains unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<gazette_core::publication::ProjectKind>,
    #[serde(skip)]
    #[schemars(skip)]
    pub(crate) software: Option<gazette_core::publication::ProjectSoftware>,
    /// Explicit authored editorial-residence filter.
    pub residence: crate::workspace::ListingProjectResidence,
    /// Existing project identity.
    pub project: Key,
    /// Explicit configured project name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Exact routed product landing.
    pub landing: ListingSubjectLanding,
    /// Explicitly selected own-publication reading index, without URL inference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explore: Option<ListingSubjectLanding>,
}

/// Existing shallow destination metadata, without expanding its body.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingSubjectLanding {
    /// Exact retained content identity.
    pub id: Key,
    /// Literal authored title.
    pub title: String,
    /// Selected serving endpoint.
    pub url: String,
    /// Native facts for the same destination.
    pub destination: crate::markdown::LinkDestination,
}

/// One native static pagination endpoint.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingPageLink {
    /// One-based ordinal in this ordering.
    pub number: usize,
    /// Serving endpoint in this ordering.
    pub url: String,
    /// Whether it is the current output view.
    pub current: bool,
    /// Whether one or more ordinals were omitted before this endpoint.
    pub gap_before: bool,
}

/// One available native chronology, linking to its first page.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ListingSortLink {
    /// Authored publication chronology direction.
    pub direction: ListingDirection,
    /// First page in this ordering under the selected serving base.
    pub url: String,
    /// Whether this chronology owns the current output view.
    pub current: bool,
}

impl ListingPage {
    /// Ordered typed work references with the same identities as their direct pages.
    pub fn public_item_list(&self) -> serde_json::Value {
        self.item_list_with(true)
    }

    /// Whether this is the original authored index output rather than a generated view.
    pub fn is_primary(&self) -> bool {
        self.variant == ListingVariant::Primary && self.page_number == 1
    }

    /// Join by exact selected item identity; no URL or folder ancestry.
    pub fn location_for(&self, id: &Key) -> Option<BrowsingLocation<'_>> {
        // Product-specific views never replace a page's authored/global browse locations.
        if self.subject.is_some() {
            return None;
        }
        self.items
            .iter()
            .find(|item| &item.id == id)
            .map(|item| BrowsingLocation {
                index: &self.content,
                via: &item.id,
                title: &self.title,
                url: &self.url,
                kind: BrowsingKind::Listing,
                page_number: Some(self.page_number),
                page_count: Some(self.page_count),
            })
    }

    /// Borrowed owner metadata uses this exact compiler-resolved serving endpoint.
    pub fn serving_route(&self) -> ResolvedRoute {
        ResolvedRoute {
            public_url: self.canonical_url.clone(),
            url: self.url.clone(),
            canonical_url: self.url.clone(),
            output: self.output.clone(),
            aliases: Vec::new(),
            mirrors: Vec::new(),
        }
    }

    /// Ordered references to existing objects, without new article identities.
    pub fn json_ld(&self) -> serde_json::Value {
        let mut value = serde_json::json!({
            "@context": super::json_ld::Context, "@type": "CollectionPage",
            "@id": self.url, "identifier": self.content, "name": self.title,
            "url": self.url,
            "gazette:route": {"@type": "@json", "@value": self.serving_route()},
            "mainEntity": self.item_list(),
        });
        if let Some(subject) = &self.subject {
            value["about"] =
                serde_json::json!(super::json_ld::Reference("project", &subject.project));
            value["relatedLink"] = match &subject.explore {
                Some(explore) => serde_json::json!([subject.landing.url, explore.url]),
                None => serde_json::json!(subject.landing.url),
            };
        }
        value
    }

    /// Ordered item metadata shared with the existing owner's linked-data model.
    pub fn item_list(&self) -> serde_json::Value {
        self.item_list_with(false)
    }

    fn item_list_with(&self, public: bool) -> serde_json::Value {
        let items: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut reference = serde_json::json!({
                    "@id": if public { &item.identity } else { &item.url },
                    "identifier": item.id,
                    "name": item.title,
                });
                if public {
                    reference["url"] = serde_json::json!(item.url);
                    if let Some(kind) = super::public_identity::reference_type(&item.work_type) {
                        reference["@type"] = serde_json::json!(kind);
                    }
                }
                serde_json::json!({"@type": "ListItem", "position": index + 1, "item": reference})
            })
            .collect();
        let mut value = serde_json::json!({
            "@type": "ItemList",
            "numberOfItems": items.len(),
            "itemListElement": items,
        });
        if let Some(direction) = self.direction {
            value["itemListOrder"] = serde_json::json!(match direction {
                ListingDirection::Ascending => "https://schema.org/ItemListOrderAscending",
                ListingDirection::Descending => "https://schema.org/ItemListOrderDescending",
            });
        }
        value
    }
}

/// No complete listing can be derived; no partial page prefix is returned.
#[derive(Debug)]
pub enum ListingError {
    /// Exact selected artwork could not bind to retained native assets.
    Artwork(super::ListingArtworkError),
    /// A source-bound reference needed by project article selection failed.
    Reference(Box<super::ProjectionError>),
    /// Discovery belongs to another immutable resolved graph.
    Discovery,
    /// A native owner, candidate or ordering constraint cannot bind.
    Binding {
        /// Exact index identity implicated in the refusal.
        content: Key,
        /// Stable explanation of the violated constraint.
        reason: &'static str,
    },
    /// A continuation would overwrite an existing route or file ancestor.
    Collision {
        /// Existing listing owner, never a synthesized page identity.
        content: Key,
        /// Conflicting output view.
        output: EntryPath,
    },
    /// Complete derived expansion exceeded its independent projection ceiling.
    Limit(&'static str),
    /// The shared typed model could not be encoded.
    Encoding(serde_json::Error),
}

impl ListingError {
    /// Stable refusal class for application and interface consumers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Discovery => "listing_discovery_owner",
            Self::Artwork(error) => error.code(),
            Self::Reference(error) => error.code(),
            Self::Binding { .. } => "listing_binding",
            Self::Collision { .. } => "listing_route_collision",
            Self::Limit(_) => "listing_limit",
            Self::Encoding(_) => "listing_encoding",
        }
    }
    /// Exact index identity when the failure is owner-specific.
    pub fn content(&self) -> Option<&Key> {
        match self {
            Self::Binding { content, .. } | Self::Collision { content, .. } => Some(content),
            Self::Artwork(error) => Some(&error.content),
            Self::Reference(_) => None,
            _ => None,
        }
    }
}
impl fmt::Display for ListingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Artwork(error) => error.fmt(f),
            Self::Reference(error) => error.fmt(f),
            Self::Discovery => f.write_str("listing discovery belongs to another resolved owner"),
            Self::Binding { content, reason } => write!(f, "listing {content}: {reason}"),
            Self::Collision { content, output } => write!(
                f,
                "listing {content} output {output} collides with an existing route"
            ),
            Self::Limit(name) => write!(f, "listing {name} ceiling exceeded"),
            Self::Encoding(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for ListingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Encoding(error) => Some(error),
            Self::Artwork(error) => Some(error),
            Self::Reference(error) => Some(error),
            _ => None,
        }
    }
}

fn binding(content: &Key, reason: &'static str) -> ListingError {
    ListingError::Binding {
        content: content.clone(),
        reason,
    }
}

fn title_order(left: &&ContentNode, right: &&ContentNode) -> Ordering {
    (&left.title, &left.id).cmp(&(&right.title, &right.id))
}

fn editorial_day_order(left: &EditorialDate, right: &EditorialDate) -> Ordering {
    let day = |date: &EditorialDate| {
        date.as_date().copied().unwrap_or_else(|| {
            date.as_timestamp()
                .expect("an editorial date is a calendar date or a timestamp")
                .naive_utc()
                .date()
        })
    };
    day(left).cmp(&day(right))
}

fn published_order(
    left: &ContentNode,
    right: &ContentNode,
    direction: ListingDirection,
) -> Ordering {
    match (&left.published, &right.published) {
        (Some(left_date), Some(right_date)) => {
            let directed = |order: Ordering| match direction {
                ListingDirection::Descending => order.reverse(),
                ListingDirection::Ascending => order,
            };
            // UTC keeps equal instants in one day despite their authored offsets.
            // Unknown times follow known times within that day in either direction;
            // this display convention does not invent a publication time.
            directed(editorial_day_order(left_date, right_date))
                .then_with(
                    || match (left_date.as_timestamp(), right_date.as_timestamp()) {
                        (Some(left), Some(right)) => directed(left.cmp(right)),
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => Ordering::Equal,
                    },
                )
                .then_with(|| title_order(&left, &right))
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => title_order(&left, &right),
    }
}

fn latest_candidate<'a>(candidates: &[&'a ContentNode]) -> Option<&'a ContentNode> {
    let dated = || {
        candidates
            .iter()
            .filter_map(|candidate| candidate.published.as_ref().map(|date| (*candidate, date)))
    };
    let (_, newest_day) =
        dated().max_by(|(_, left), (_, right)| editorial_day_order(left, right))?;
    let mut latest: Option<&ContentNode> = None;
    let mut precision = None;
    for (candidate, date) in
        dated().filter(|(_, date)| editorial_day_order(date, newest_day) == Ordering::Equal)
    {
        if precision.is_some_and(|previous| previous != date.is_date_only()) {
            // The newest editorial day is known, but its display convention
            // cannot establish which unknown-time or timed entry came last.
            return None;
        }
        precision = Some(date.is_date_only());
        if latest.is_none_or(|previous| {
            published_order(candidate, previous, ListingDirection::Descending) == Ordering::Less
        }) {
            latest = Some(candidate);
        }
    }
    latest
}

pub(super) fn page_numbers(current: usize, count: usize) -> BTreeSet<usize> {
    let mut numbers = BTreeSet::from([1, count]);
    numbers.extend(current.saturating_sub(2).max(1)..=current.saturating_add(2).min(count));
    numbers
}

fn charge(
    remaining: &mut usize,
    lengths: impl IntoIterator<Item = usize>,
) -> Result<(), ListingError> {
    for length in lengths {
        *remaining = remaining
            .checked_sub(length)
            .ok_or(ListingError::Limit("retained text bytes"))?;
    }
    Ok(())
}

fn reserve(
    outputs: &mut BTreeSet<EntryPath>,
    output: &EntryPath,
    content: &Key,
) -> Result<(), ListingError> {
    if outputs.iter().any(|existing| {
        existing == output
            || output
                .as_str()
                .strip_prefix(existing.as_str())
                .is_some_and(|tail| tail.starts_with('/'))
            || existing
                .as_str()
                .strip_prefix(output.as_str())
                .is_some_and(|tail| tail.starts_with('/'))
    }) {
        return Err(ListingError::Collision {
            content: content.clone(),
            output: output.clone(),
        });
    }
    outputs.insert(output.clone());
    Ok(())
}

impl ResolvedAuthored<'_, '_> {
    /// One bounded shallow projection shared by configured listings and ranked suggestions.
    pub(super) fn project_listing_link(
        &self,
        owner: &ContentNode,
        node: &ContentNode,
        links_remaining: &mut usize,
        retained_remaining: &mut usize,
    ) -> Result<ListingLink, ListingError> {
        let content = self.content();
        let route = content
            .route(&node.id)
            .ok_or_else(|| binding(&owner.id, "selected candidate has no resolved route"))?;
        charge(
            retained_remaining,
            [
                node.id.as_str().len(),
                node.site.as_str().len(),
                node.title.len(),
                node.description.as_ref().map_or(0, String::len),
                route.canonical_url.len(),
                node.language
                    .as_ref()
                    .map_or(0, |language| language.as_str().len()),
            ],
        )?;
        let selection = content.presentation_for(&node.site);
        if let Some(selection) = selection {
            charge(
                retained_remaining,
                [
                    selection.package.as_str().len(),
                    selection
                        .palette
                        .as_ref()
                        .map_or(0, |palette| palette.as_str().len()),
                ],
            )?;
        }
        let artwork =
            self.project_listing_artwork(&node.id, links_remaining, retained_remaining)?;
        *links_remaining = links_remaining
            .checked_sub(node.attributions.len())
            .ok_or(ListingError::Limit("link occurrences"))?;
        let attributions = node
            .attributions
            .iter()
            .map(|attribution| {
                let principal = content
                    .compilation()
                    .workspace
                    .principal(&attribution.principal)
                    .ok_or_else(|| binding(&owner.id, "selected attribution has no principal"))?;
                let website = content.principal_website(principal, &owner.site);
                charge(
                    retained_remaining,
                    [
                        principal.id.as_str().len(),
                        principal.name.as_ref().map_or(0, String::len),
                        principal
                            .website
                            .as_ref()
                            .map_or(0, |website| match website {
                                gazette_core::publication::PrincipalWebsite::Site { site } => {
                                    site.as_str().len()
                                }
                                gazette_core::publication::PrincipalWebsite::External { url } => {
                                    url.len()
                                }
                            }),
                    ],
                )?;
                charge(
                    retained_remaining,
                    principal.same_as.iter().map(String::len),
                )?;
                if let Some(website) = &website {
                    charge(
                        retained_remaining,
                        [
                            website.url.len(),
                            website.canonical_url.len(),
                            website
                                .destination
                                .site
                                .as_ref()
                                .map_or(0, |site| site.as_str().len()),
                            website
                                .destination
                                .presentation
                                .as_ref()
                                .map_or(0, |value| {
                                    value.theme.as_str().len()
                                        + value.palette.as_ref().map_or(0, |key| key.as_str().len())
                                }),
                        ],
                    )?;
                }
                Ok(ListingAttribution {
                    principal: principal.clone(),
                    website,
                    role: attribution.role,
                })
            })
            .collect::<Result<Vec<_>, ListingError>>()?;
        let reference = super::links::ResolvedReference {
            asset_path: None,
            destination: route.canonical_url.clone(),
            asset: None,
            content: Some(node.id.clone()),
            resource: false,
        };
        let destination = self.link_destination(
            owner,
            &route.canonical_url,
            &route.canonical_url,
            Some(&reference),
        );
        let mut destination_bytes = LimitedWriter::new(Vec::new(), *retained_remaining);
        serde_json::to_writer(&mut destination_bytes, &destination)
            .map_err(ListingError::Encoding)?;
        if destination_bytes.exceeded() {
            return Err(ListingError::Limit("retained text bytes"));
        }
        charge(retained_remaining, [destination_bytes.into_inner().len()])?;
        let identity = super::public_identity::work(node, Some(route), false);
        let work_type =
            super::public_identity::work_type(node, false, self.content().is_listing(node));
        charge(retained_remaining, [identity.len(), work_type.len()])?;
        Ok(ListingLink {
            identity,
            work_type: work_type.into(),
            artwork,
            id: node.id.clone(),
            site: node.site.clone(),
            destination,
            title: node.title.clone(),
            description: node.description.clone(),
            url: route.canonical_url.clone(),
            language: node.language.clone(),
            published: node.published.clone(),
            modified: node.modified.clone(),
            attributions,
            presentation: selection.map(SitePresentation::from),
        })
    }

    /// Select from the complete graph, filter discovery, order once, then paginate.
    /// No source paths, new Content nodes, memberships or lifecycle defaults are derived.
    ///
    /// # Errors
    /// Refuses foreign discovery, invalid owners/pins, unconfined
    /// continuations, route collisions and whole-projection expansion ceilings.
    pub fn project_listing_pages(
        &self,
        site: &Key,
        discovery: &Discovery<'_>,
    ) -> Result<Vec<ListingPage>, ListingError> {
        if !discovery.matches(self) {
            return Err(ListingError::Discovery);
        }
        let content = self.content();
        let publication = content
            .compilation()
            .workspace
            .publication_for(site)
            .ok_or_else(|| binding(site, "site has no publication binding"))?;
        if publication.listings.is_empty() {
            return Ok(Vec::new());
        }
        let base = content
            .base_url(site)
            .ok_or_else(|| binding(site, "site has no serving base"))?;
        let graph = content.graph();
        let mut outputs = BTreeSet::new();
        for node in graph.nodes() {
            if let Some(route) = content.route(&node.id) {
                if &node.site == site {
                    outputs.insert(route.output.clone());
                    outputs.extend(route.aliases.iter().map(|alias| alias.output.clone()));
                }
                outputs.extend(
                    route
                        .mirrors
                        .iter()
                        .filter(|mirror| &mirror.site == site)
                        .map(|mirror| mirror.output.clone()),
                );
            }
        }
        let mut owners = BTreeSet::new();
        let mut pages = Vec::new();
        let mut links_remaining = self.projection_limits.edges;
        let mut retained_remaining = self.projection_limits.text_bytes;
        let mut bytes = LimitedWriter::new(io::sink(), self.projection_limits.text_bytes);
        for page in &publication.taxonomy_pages {
            charge(
                &mut retained_remaining,
                [page.route.output().as_str().len()],
            )?;
            outputs.insert(page.route.output().clone());
        }
        for declaration in &publication.listings {
            let id = &declaration.content;
            if !owners.insert(id) {
                return Err(binding(id, "index has multiple listing declarations"));
            }
            let owner = graph
                .node(id)
                .ok_or_else(|| binding(id, "index owner is absent from the selected graph"))?;
            if &owner.site != site
                || !matches!(
                    owner.structure,
                    ContentKind::Page {
                        family: PageFamily::Index
                    }
                )
            {
                return Err(binding(
                    id,
                    "owner must be an index page on the configured site",
                ));
            }
            self.source_binding(id)
                .map_err(|_| binding(id, "index has no exact retained source binding"))?;
            let body = self
                .file(id)
                .and_then(crate::corpus::File::body)
                .ok_or_else(|| binding(id, "index has no retained Markdown body"))?;
            if !body.source().trim().is_empty() {
                return Err(binding(
                    id,
                    "listing index must have a declaration-only body",
                ));
            }
            let authored_route = owner
                .route
                .as_ref()
                .ok_or_else(|| binding(id, "index has no explicit route"))?;
            if authored_route.canonical.is_some() {
                return Err(binding(id, "listing index cannot be a republished copy"));
            }
            let primary = content
                .route(id)
                .ok_or_else(|| binding(id, "index route is unresolved"))?;
            if publication.taxonomy_pages.iter().any(|page| {
                let existing = page.route.output().as_str();
                let output = primary.output.as_str();
                existing == output
                    || output
                        .strip_prefix(existing)
                        .is_some_and(|tail| tail.starts_with('/'))
                    || existing
                        .strip_prefix(output)
                        .is_some_and(|tail| tail.starts_with('/'))
            }) {
                return Err(ListingError::Collision {
                    content: id.clone(),
                    output: primary.output.clone(),
                });
            }
            charge(
                &mut retained_remaining,
                [declaration.pagination.next_pages.len()],
            )?;
            let continuation = declaration.pagination.continuation(2)
                .map_err(|_| binding(id, "continuation requires a confined relative directory with one number placeholder"))?;
            authored_route
                .path
                .join_directory(Some(&continuation))
                .map_err(|_| binding(id, "pagination requires a directory index route"))?;
            let page_size = declaration.pagination.page_size;
            if page_size == 0 {
                return Err(binding(id, "page size must be positive"));
            }
            let subject = self.listing_subject(&declaration.select, owner)?;
            let mut candidates = Vec::new();
            for candidate in graph.nodes() {
                if matches!(
                    &declaration.select,
                    ListingSelection::ProjectArticles { .. }
                ) && !discovery.contains(&candidate.id)
                {
                    continue;
                }
                if self.listing_candidate(
                    &declaration.select,
                    owner,
                    candidate,
                    &mut links_remaining,
                )? {
                    candidates.push(candidate);
                }
            }
            let mut priorities = BTreeMap::new();
            if let ListingOrder::Priority { first, .. } = &declaration.order {
                let candidate_ids: BTreeSet<_> = candidates.iter().map(|node| &node.id).collect();
                for (position, pin) in first.iter().enumerate() {
                    if priorities.insert(pin, position).is_some() || !candidate_ids.contains(pin) {
                        return Err(binding(
                            id,
                            "priority must name unique same-site candidates of the selected kind",
                        ));
                    }
                }
            }
            declaration
                .validate_alternate()
                .map_err(|reason| binding(id, reason))?;
            let selected_candidates: Vec<_> = candidates
                .into_iter()
                .filter(|node| discovery.contains(&node.id))
                .collect();
            let latest_candidate = latest_candidate(&selected_candidates);
            let mut roots = Vec::new();
            for (variant, order, path) in declaration.orders() {
                // Charge worst-case URL encoding and its copies before joining
                // the configured relative directory or expanding the serving base.
                let joined = path
                    .map_or(0, |path| path.as_str().len())
                    .checked_mul(3)
                    .and_then(|length| length.checked_add(authored_route.path.as_str().len()))
                    .and_then(|length| length.checked_add(12))
                    .ok_or(ListingError::Limit("route text bytes"))?;
                charge(
                    &mut retained_remaining,
                    [joined, joined, base.as_str().len(), joined],
                )?;
                let root = authored_route
                    .path
                    .join_directory(path)
                    .map_err(|_| binding(id, "alternate order requires a directory index route"))?;
                if variant == ListingVariant::Alternate {
                    reserve(&mut outputs, root.output(), id)?;
                }
                let url = base.resolve(&root);
                roots.push((variant, order, root, url));
            }
            for (variant, order, root, root_url) in &roots {
                let mut selected = selected_candidates.clone();
                match order {
                    ListingOrder::Published { direction, .. } => {
                        selected.sort_unstable_by(|left, right| {
                            published_order(left, right, *direction)
                        });
                    }
                    ListingOrder::Priority { .. } => {
                        selected.sort_unstable_by(|left, right| {
                            match (priorities.get(&left.id), priorities.get(&right.id)) {
                                (Some(left), Some(right)) => left.cmp(right),
                                (Some(_), None) => Ordering::Less,
                                (None, Some(_)) => Ordering::Greater,
                                (None, None) => title_order(left, right),
                            }
                        })
                    }
                }
                let total_items = selected.len();
                let page_count = total_items.div_ceil(page_size).max(1);
                if page_count > self.projection_limits.nodes.saturating_sub(pages.len()) {
                    return Err(ListingError::Limit("page occurrences"));
                }
                let mut endpoints = Vec::with_capacity(page_count);
                charge(
                    &mut retained_remaining,
                    [
                        root.as_str().len(),
                        root.output().as_str().len(),
                        root_url.len(),
                    ],
                )?;
                endpoints.push((root.clone(), root_url.clone()));
                for number in 2..=page_count {
                    charge(
                        &mut retained_remaining,
                        [declaration.pagination.next_pages.len(), 20],
                    )?;
                    let entry = declaration
                        .pagination
                        .continuation(number)
                        .map_err(|_| binding(id, "invalid continuation directory"))?;
                    // Joining may encode each byte into three URL characters. Charge
                    // both route strings and the expanded base before allocating them.
                    let joined = root
                        .as_str()
                        .len()
                        .checked_add(
                            entry
                                .as_str()
                                .len()
                                .checked_mul(3)
                                .ok_or(ListingError::Limit("route text bytes"))?,
                        )
                        .and_then(|length| length.checked_add(12))
                        .ok_or(ListingError::Limit("route text bytes"))?;
                    charge(
                        &mut retained_remaining,
                        [joined, joined, base.as_str().len(), joined],
                    )?;
                    let route = root
                        .join_directory(Some(&entry))
                        .map_err(|_| binding(id, "invalid continuation route"))?;
                    reserve(&mut outputs, route.output(), id)?;
                    let url = base.resolve(&route);
                    endpoints.push((route, url));
                }
                for (index, (route, url)) in endpoints.iter().enumerate() {
                    let start = index
                        .checked_mul(page_size)
                        .ok_or(ListingError::Limit("page offsets"))?;
                    let end = start.saturating_add(page_size).min(total_items);
                    let members = selected.get(start..end).unwrap_or(&[]);
                    links_remaining = links_remaining
                        .checked_sub(members.len())
                        .and_then(|remaining| {
                            remaining.checked_sub(usize::from(latest_candidate.is_some()))
                        })
                        .ok_or(ListingError::Limit("link occurrences"))?;
                    let items = members
                        .iter()
                        .map(|node| {
                            self.project_listing_link(
                                owner,
                                node,
                                &mut links_remaining,
                                &mut retained_remaining,
                            )
                        })
                        .collect::<Result<Vec<_>, ListingError>>()?;
                    let latest = latest_candidate
                        .map(|node| {
                            self.project_listing_link(
                                owner,
                                node,
                                &mut links_remaining,
                                &mut retained_remaining,
                            )
                        })
                        .transpose()?;
                    charge(
                        &mut retained_remaining,
                        [
                            id.as_str().len(),
                            owner.title.len(),
                            route.as_str().len(),
                            route.output().as_str().len(),
                            url.len(),
                            route.output().as_str().len(),
                            index
                                .checked_sub(1)
                                .map_or(0, |previous| endpoints[previous].1.len()),
                            endpoints.get(index + 1).map_or(0, |next| next.1.len()),
                        ],
                    )?;
                    let ordinals = page_numbers(index + 1, page_count);
                    let mut previous_number = 0;
                    let mut navigation = Vec::with_capacity(ordinals.len());
                    links_remaining = links_remaining
                        .checked_sub(ordinals.len())
                        .ok_or(ListingError::Limit("link occurrences"))?;
                    for number in ordinals {
                        let endpoint = &endpoints[number - 1].1;
                        charge(&mut retained_remaining, [endpoint.len()])?;
                        navigation.push(ListingPageLink {
                            number,
                            url: endpoint.clone(),
                            current: number == index + 1,
                            gap_before: previous_number > 0 && number > previous_number + 1,
                        });
                        previous_number = number;
                    }
                    let mut sorts = Vec::new();
                    if roots.len() > 1 {
                        links_remaining = links_remaining
                            .checked_sub(roots.len())
                            .ok_or(ListingError::Limit("link occurrences"))?;
                        for (other_variant, other_order, _, other_url) in &roots {
                            if let ListingOrder::Published { direction, .. } = other_order {
                                charge(&mut retained_remaining, [other_url.len()])?;
                                sorts.push(ListingSortLink {
                                    direction: *direction,
                                    url: other_url.clone(),
                                    current: other_variant == variant,
                                });
                            }
                        }
                    }
                    if let Some(subject) = &subject {
                        let mut subject_bytes = LimitedWriter::new(Vec::new(), retained_remaining);
                        let encoded = serde_json::to_writer(&mut subject_bytes, subject);
                        if subject_bytes.exceeded() {
                            return Err(ListingError::Limit("retained text bytes"));
                        }
                        encoded.map_err(ListingError::Encoding)?;
                        charge(&mut retained_remaining, [subject_bytes.into_inner().len()])?;
                    }
                    let page = ListingPage {
                        canonical_url: content
                            .compilation()
                            .workspace
                            .public_base_url(site)
                            .ok_or_else(|| binding(site, "site has no public base"))?
                            .resolve(route),
                        content: id.clone(),
                        title: owner.title.clone(),
                        route: route.clone(),
                        url: url.clone(),
                        output: route.output().clone(),
                        page_number: index + 1,
                        page_size,
                        variant: *variant,
                        direction: match order {
                            ListingOrder::Published { direction, .. } => Some(*direction),
                            ListingOrder::Priority { .. } => None,
                        },
                        pages: navigation,
                        sorts,
                        page_count,
                        total_items,
                        latest,
                        subject: subject.clone(),
                        previous: index
                            .checked_sub(1)
                            .map(|previous| endpoints[previous].1.clone()),
                        next: endpoints.get(index + 1).map(|next| next.1.clone()),
                        items,
                    };
                    let encoded = serde_json::to_writer(&mut bytes, &page);
                    if bytes.exceeded() {
                        return Err(ListingError::Limit("JSON bytes"));
                    }
                    encoded.map_err(ListingError::Encoding)?;
                    pages.push(page);
                }
            }
        }
        pages.sort_unstable_by(|left, right| left.output.cmp(&right.output));
        Ok(pages)
    }
}
