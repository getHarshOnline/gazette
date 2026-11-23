//! Borrowed site-page selection and bounded discovery projections; no native reads.

use std::{
    collections::BTreeSet,
    fmt,
    io::{self, Write},
};

use gazette_core::{
    publication::{Key, content::ContentNode},
    render::LimitedWriter,
};

use super::{ProjectedTaxonomyPage, ResolvedAuthored, SourceBindingError};
use crate::workspace::{BrandPublication, ResolvedContent, ResolvedMirror, ResolvedRoute};

mod alternates;
mod listings;
mod llms;
pub use llms::{LlmsDocument, LlmsPage, LlmsReadingSection};
mod node;
mod page;
mod projects;
mod sitemap;
pub use alternates::MarkdownAlternate;
pub use node::DiscoveryNode;
pub use page::{DiscoveryData, DiscoveryPage, DiscoverySearch};
pub use projects::ProjectOriginError;

/// Source-bound automatic discovery selection over one exact resolved owner.
/// This does not change authored meaning, render obligations or public admission.
#[derive(Debug)]
pub struct Discovery<'a> {
    owner: &'a ResolvedContent<'a>,
    eligible: BTreeSet<&'a Key>,
}

impl Discovery<'_> {
    /// Whether the exact selected owner admits this identity to automatic indexes.
    pub fn contains(&self, id: &Key) -> bool {
        self.eligible.contains(id)
    }

    /// Value-equivalent but independently resolved graphs are not this selection's owner.
    pub fn matches(&self, authored: &ResolvedAuthored<'_, '_>) -> bool {
        std::ptr::eq(self.owner, authored.content())
    }

    /// Borrow an eligible native record without expanding its structural endpoints.
    pub fn node<'n>(&'n self, node: &'n ContentNode) -> Option<DiscoveryNode<'n>> {
        (self.owns(node) && self.contains(&node.id)).then_some(DiscoveryNode::new(
            node,
            self.owner
                .presentation_for(&node.site)
                .map(super::SitePresentation::from),
            self.owner.route(&node.id),
            self.owner.graph().is_documentation(&node.id),
            self.owner.is_listing(node),
        ))
    }

    /// Whether this is the owner's exact node, including undiscoverable direct pages.
    pub fn owns(&self, node: &ContentNode) -> bool {
        self.owner
            .graph()
            .node(&node.id)
            .is_some_and(|native| std::ptr::eq(native, node))
    }

    /// Borrow the already resolved address of an eligible exact native endpoint.
    pub(crate) fn navigation_route(&self, node: &ContentNode) -> Option<&ResolvedRoute> {
        self.includes_navigation_node(node)
            .then(|| self.owner.route(&node.id))
            .flatten()
    }
}

/// One resolved node and its canonical/alias routes; no copied publication meaning.
#[derive(Clone, Copy, Debug)]
pub struct RoutedPage<'a> {
    /// The exact content record used by the page projection.
    pub node: &'a ContentNode,
    /// The existing route result; discovery never reconstructs a URL.
    pub route: &'a ResolvedRoute,
}

/// One foreign redirect presentation, borrowing the single canonical object and
/// its compiled addresses. It is not another local page or discovery entry.
#[derive(Clone, Copy, Debug)]
pub struct MirroredPage<'a> {
    /// The canonical content object, never a copied or re-homed body.
    pub node: &'a ContentNode,
    /// Sole canonical route and redirect target.
    pub route: &'a ResolvedRoute,
    /// Exact address on the selected serving site.
    pub mirror: &'a ResolvedMirror,
}

/// Complete local, draft-inclusive page set in canonical-URL order.
/// This is structural selection, not discoverability or public admission.
#[derive(Debug)]
pub struct SitePages<'a> {
    owner: &'a ResolvedContent<'a>,
    name: &'a str,
    publication: &'a BrandPublication,
    pages: Vec<RoutedPage<'a>>,
    mirrors: Vec<MirroredPage<'a>>,
    taxonomies: Vec<TaxonomyDiscovery>,
    listing_pages: Vec<String>,
}

#[derive(Debug)]
struct TaxonomyDiscovery {
    url: String,
}

/// Site selection or a required discovery output could not complete.
#[derive(Debug)]
pub enum SitePagesError {
    /// Explicit publication metadata could not bind to this selected site.
    Metadata(super::PublicationMetadataError),
    /// The selected site has no content.
    Empty,
    /// A selected content record lacks its required canonical route.
    RouteRequired(Key),
    /// The selected site has no explicit brand/publication binding.
    BrandRequired,
    /// A page selection belongs to another resolved owner.
    Discovery,
    /// Derived listings do not exactly match this owner's selected taxonomy routes.
    Taxonomy,
    /// Generated reading views do not match their configured index and pagination.
    Listing,
    /// Machine output configuration is missing or inconsistent with its representation.
    Machine(&'static str),
    /// A curated reference is not a selected discoverable canonical page of this site.
    LlmsReference(Key),
    /// The selected description source lacks its explicit authored description.
    DescriptionRequired(Key),
    /// A canonical or derived discovery URL/output duplicates another site route.
    Collision,
    /// A declared or protocol resource ceiling was exceeded; nothing is truncated.
    Limit(&'static str),
    /// A serialization write failed, with no partial successful artifact.
    Write(io::Error),
    /// Complete generated inventory or output-path validation failed.
    Output(gazette_core::source::Error),
}

impl SitePagesError {
    /// Stable diagnostic class, independent from display formatting.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Metadata(error) => error.code(),
            Self::Empty => "site_empty",
            Self::RouteRequired(_) => "site_route_required",
            Self::BrandRequired => "site_brand_required",
            Self::Discovery => "site_discovery_owner",
            Self::Taxonomy => "site_taxonomy_binding",
            Self::Listing => "site_listing_binding",
            Self::Machine(_) => "site_machine_configuration",
            Self::LlmsReference(_) => "site_llms_reference",
            Self::DescriptionRequired(_) => "site_llms_description_required",
            Self::Collision => "site_discovery_collision",
            Self::Limit(_) => "discovery_limit",
            Self::Write(_) => "discovery_write",
            Self::Output(error) => error.code(),
        }
    }
}
impl fmt::Display for SitePagesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => error.fmt(f),
            Self::Empty => f.write_str("selected site has no authored objects"),
            Self::RouteRequired(id) => write!(f, "site object {id} needs an explicit route"),
            Self::BrandRequired => f.write_str("site needs an explicit brand/publication binding"),
            Self::Discovery => f.write_str("discovery selection belongs to another resolved owner"),
            Self::Taxonomy => {
                f.write_str("taxonomy listings must match the complete configured site selection")
            }
            Self::Listing => {
                f.write_str("reading views must match the complete configured index pagination")
            }
            Self::Machine(reason) => write!(f, "machine output configuration: {reason}"),
            Self::LlmsReference(id) => write!(
                f,
                "curated reference {id} needs a selected discoverable canonical page on this site"
            ),
            Self::DescriptionRequired(id) => write!(
                f,
                "llms page {id} needs an explicit nonempty authored description"
            ),
            Self::Collision => {
                f.write_str("discovery URL or output collides with another selected site route")
            }
            Self::Limit(resource) => write!(
                f,
                "discovery limit exceeded: {resource}; no artifact produced"
            ),
            Self::Write(error) => write!(f, "discovery serialization failed: {error}"),
            Self::Output(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for SitePagesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::Output(error) => Some(error),
            _ => None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Resolve literal configured discovery flags once from every exact source binding.
    ///
    /// # Errors
    /// Rejects any missing or wrong-site binding, even for undiscoverable content.
    /// Undiscoverable sources remain required compilation and rendering inputs.
    pub fn discovery(&self) -> Result<Discovery<'_>, SourceBindingError> {
        let mut eligible = BTreeSet::new();
        for node in self.content().graph().nodes() {
            if self.source_binding(&node.id)?.discoverable {
                eligible.insert(&node.id);
            }
        }
        Ok(Discovery {
            owner: self.content(),
            eligible,
        })
    }

    /// Select the exact local pages once for HTML and discovery consumers.
    /// Foreign references do not become local pages. Explicit mirrors form a
    /// separate redirect set, never duplicate primary/discovery records.
    ///
    /// # Errors
    /// Rejects an empty site, any missing route or absent brand binding. This does
    /// not filter drafts, infer rights or read sources/registries again.
    pub fn project_site_pages(&self, site: &Key) -> Result<SitePages<'_>, SitePagesError> {
        self.publication_metadata(site)
            .map_err(SitePagesError::Metadata)?;
        let mut pages = self
            .content()
            .graph()
            .nodes()
            .filter(|node| &node.site == site)
            .map(|node| {
                let route = self
                    .content()
                    .route(&node.id)
                    .ok_or_else(|| SitePagesError::RouteRequired(node.id.clone()))?;
                Ok(RoutedPage { node, route })
            })
            .collect::<Result<Vec<_>, SitePagesError>>()?;
        let mut mirrors = Vec::new();
        for node in self.content().graph().nodes() {
            let Some(route) = self.content().route(&node.id) else {
                continue;
            };
            let target = node
                .route
                .as_ref()
                .and_then(|route| route.canonical.as_ref())
                .unwrap_or(&node.id);
            let canonical = self
                .content()
                .graph()
                .node(target)
                .ok_or_else(|| SitePagesError::RouteRequired(target.clone()))?;
            for mirror in route.mirrors.iter().filter(|mirror| &mirror.site == site) {
                mirrors.push(MirroredPage {
                    node: canonical,
                    route,
                    mirror,
                });
            }
        }
        if pages.is_empty() && mirrors.is_empty() {
            return Err(SitePagesError::Empty);
        }
        let (brand, publication) = self
            .content()
            .compilation()
            .workspace
            .brands()
            .find_map(|brand| {
                brand
                    .publications
                    .iter()
                    .find(|p| &p.site == site)
                    .map(|p| (brand, p))
            })
            .ok_or(SitePagesError::BrandRequired)?;
        pages.sort_unstable_by(|a, b| a.route.url.cmp(&b.route.url));
        mirrors.sort_unstable_by(|a, b| a.mirror.url.cmp(&b.mirror.url));
        Ok(SitePages {
            owner: self.content(),
            name: &brand.name,
            publication,
            pages,
            mirrors,
            taxonomies: Vec::new(),
            listing_pages: Vec::new(),
        })
    }
}

impl<'a> SitePages<'a> {
    /// Select automatic indexes without weakening this site's full page materialization.
    /// An empty discovery set is valid; no source, route or publication is reconstructed.
    ///
    /// # Errors
    /// Rejects page records from an independently resolved owner.
    pub fn discoverable(&self, discovery: &Discovery<'_>) -> Result<Self, SitePagesError> {
        if !std::ptr::eq(self.owner, discovery.owner)
            || self.pages.iter().any(|page| !discovery.owns(page.node))
            || self.mirrors.iter().any(|page| !discovery.owns(page.node))
        {
            return Err(SitePagesError::Discovery);
        }
        Ok(Self {
            owner: self.owner,
            name: self.name,
            publication: self.publication,
            pages: self
                .pages
                .iter()
                .filter(|page| discovery.contains(&page.node.id))
                .copied()
                .collect(),
            // Redirects are materialization, not automatic index entries.
            mirrors: Vec::new(),
            // Derived listings must be acquired again from this exact selection.
            taxonomies: Vec::new(),
            listing_pages: Vec::new(),
        })
    }

    /// Join the complete derived taxonomy route set without inventing content nodes.
    /// Replaces any previous join; call after selecting discoverable authored pages.
    ///
    /// # Errors
    /// Rejects missing/duplicate declarations, foreign identities, altered labels,
    /// routes or templates, and collisions with any authored/redirect site route.
    pub fn with_taxonomy_pages(
        &self,
        listings: &[ProjectedTaxonomyPage<'_>],
    ) -> Result<Self, SitePagesError> {
        if listings.len() != self.publication.taxonomy_pages.len() {
            return Err(SitePagesError::Taxonomy);
        }
        let mut urls = BTreeSet::new();
        let mut outputs = BTreeSet::new();
        for node in self.owner.graph().nodes() {
            if let Some(route) = self.owner.route(&node.id) {
                if node.site == self.publication.site {
                    urls.insert(route.url.as_str());
                    outputs.insert(&route.output);
                    for alias in &route.aliases {
                        urls.insert(alias.url.as_str());
                        outputs.insert(&alias.output);
                    }
                }
                for mirror in route
                    .mirrors
                    .iter()
                    .filter(|mirror| mirror.site == self.publication.site)
                {
                    urls.insert(mirror.url.as_str());
                    outputs.insert(&mirror.output);
                }
            }
        }
        let mut identities = BTreeSet::new();
        let mut taxonomies = Vec::with_capacity(listings.len());
        for listing in listings {
            let model = &listing.model;
            let term = model.term.map(|term| &term.id);
            let declaration = self
                .publication
                .taxonomy_pages
                .iter()
                .find(|declaration| {
                    &declaration.taxonomy == model.taxonomy.id && declaration.term.as_ref() == term
                })
                .ok_or(SitePagesError::Taxonomy)?;
            let taxonomy = self
                .owner
                .compilation()
                .workspace
                .taxonomy(model.taxonomy.id)
                .ok_or(SitePagesError::Taxonomy)?;
            let native_term = term
                .map(|term| taxonomy.term(term).ok_or(SitePagesError::Taxonomy))
                .transpose()?;
            let title = native_term.map_or(taxonomy.label.as_str(), |term| term.label.as_str());
            let base = self
                .owner
                .base_url(&self.publication.site)
                .ok_or(SitePagesError::BrandRequired)?;
            if !identities.insert((model.taxonomy.id, term))
                || model.taxonomy.label != taxonomy.label
                || model.taxonomy.language != taxonomy.language.as_ref()
                || model.term != native_term
                || model.title != title
                || model.canonical_url != base.resolve(&declaration.route)
                || model.output != *declaration.route.output()
                || listing.template != declaration.template
            {
                return Err(SitePagesError::Taxonomy);
            }
            if !urls.insert(model.canonical_url.as_str()) || !outputs.insert(&model.output) {
                return Err(SitePagesError::Collision);
            }
            taxonomies.push(TaxonomyDiscovery {
                url: model.canonical_url.clone(),
            });
        }
        taxonomies.sort_unstable_by(|a, b| a.url.cmp(&b.url));
        Ok(Self {
            owner: self.owner,
            name: self.name,
            publication: self.publication,
            pages: self.pages.clone(),
            mirrors: self.mirrors.clone(),
            taxonomies,
            listing_pages: self.listing_pages.clone(),
        })
    }

    /// The sole brand-owned output selection, borrowed from compiled configuration.
    pub fn publication(&self) -> &BrandPublication {
        self.publication
    }

    /// The same public metadata contract supplied to pages and Theme, excluding private machinery.
    ///
    /// # Errors
    /// Refuses a home endpoint that does not bind to this selected graph and site's routes.
    pub fn metadata(
        &self,
    ) -> Result<Option<super::PublicationMetadata<'_>>, super::PublicationMetadataError> {
        self.owner.publication_metadata(&self.publication.site)
    }

    /// Borrow the same records and routes used by every output from this selection.
    pub fn pages(&self) -> impl ExactSizeIterator<Item = &RoutedPage<'_>> {
        self.pages.iter()
    }

    /// Explicit redirect presentations in serving-URL order, excluded from indexes.
    pub fn mirrors(&self) -> &[MirroredPage<'a>] {
        &self.mirrors
    }

    /// Emit canonical URLs only, using the sitemap protocol's namespace and bounds.
    /// No wall-clock dates, aliases, inferred roots or foreign pages are added.
    ///
    /// # Errors
    /// Rejects more than 50,000 URLs, URLs of 2,048 characters or more, and output
    /// exceeding the lesser of `maximum_bytes` and 52,428,800 uncompressed bytes.
    /// For a complete partitioned publication use [`Self::sitemaps`].
    pub fn sitemap(&self, maximum_bytes: usize) -> Result<Vec<u8>, SitePagesError> {
        sitemap::single(self, maximum_bytes)
    }

    /// Emit one complete sitemap inventory from this exact selected page set.
    /// Small sets retain `sitemap.xml`; larger sets use that path for an index.
    /// File bytes determine partitioning; aggregate/entry exhaustion fails the set.
    ///
    /// # Errors
    /// Rejects invalid URL/packaging bounds, an unfit entry or index, and complete
    /// inventory exhaustion. No URLs are omitted and no native inputs are acquired.
    pub fn sitemaps(
        &self,
        limits: gazette_core::source::Limits,
    ) -> Result<gazette_core::source::CapturedSource, SitePagesError> {
        sitemap::assemble(self, limits)
    }
}

pub(super) fn plain(output: &mut impl Write, value: &str) -> io::Result<()> {
    let mut leading = true;
    let mut ordinal = true;
    escaped(output, value, |c| {
        let escape = matches!(
            c,
            '\\' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '{' | '}' | '#' | '~' | '|' | '$'
        ) || leading && matches!(c, '-' | '+' | '=' | ':')
            || ordinal && matches!(c, '.' | ')');
        if matches!(c, '\n' | '\r') {
            leading = true;
            ordinal = true;
        } else if !c.is_whitespace() {
            leading = false;
            ordinal &= c.is_ascii_digit();
        }
        escape
    })
}

pub(super) fn escaped(
    output: &mut impl Write,
    value: &str,
    mut escape: impl FnMut(char) -> bool,
) -> io::Result<()> {
    let mut utf8 = [0; 4];
    for c in value.chars() {
        let escaped_character = escape(c);
        // Preserve literal entity-looking text through Markdown's entity decoding.
        // A backslash alone is not sufficient in a link destination.
        if c == '&' {
            output.write_all(b"&amp;")?;
            continue;
        }
        if escaped_character {
            output.write_all(b"\\")?;
        }
        output.write_all(c.encode_utf8(&mut utf8).as_bytes())?;
    }
    Ok(())
}

pub(super) fn bounded(
    maximum: usize,
    resource: &'static str,
    write: impl FnOnce(&mut LimitedWriter<Vec<u8>>) -> io::Result<()>,
) -> Result<Vec<u8>, SitePagesError> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    let result = write(&mut output);
    if output.exceeded() {
        return Err(SitePagesError::Limit(resource));
    }
    result.map_err(SitePagesError::Write)?;
    Ok(output.into_inner())
}
