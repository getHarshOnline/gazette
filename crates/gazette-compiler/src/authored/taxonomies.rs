//! Configured taxonomy listings over the existing exact discoverable site-page set.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::{
    publication::{Key, LanguageTag, TaxonomyTerm},
    source::EntryPath,
};
use serde::Serialize;

use super::{Discovery, ResolvedAuthored, SitePagesError, TaxonomyView};

/// Private rendering selection; template custody never enters the public listing model.
#[derive(Debug)]
pub struct ProjectedTaxonomyPage<'a> {
    /// Exact configured template in the selected retained Theme.
    pub template: EntryPath,
    /// One shared human/machine listing projection.
    pub model: TaxonomyListing<'a>,
}

/// Catalog metadata and eligible links, without synthetic content or authority records.
#[derive(Debug, Serialize)]
pub struct TaxonomyListing<'a> {
    /// Exact taxonomy metadata; unrelated catalog terms are not exported.
    pub taxonomy: TaxonomyView<'a>,
    /// Selected term, absent for a used-term index.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub term: Option<&'a TaxonomyTerm>,
    /// Literal term label or taxonomy label, never a generated display name.
    pub title: &'a str,
    /// Canonical URL derived from the explicit public base and configured route.
    pub canonical_url: String,
    /// Exact generated file beneath the site's output root.
    pub output: EntryPath,
    /// Key-ordered terms; each group's links use canonical-URL order.
    pub groups: Vec<TaxonomyGroup<'a>>,
}

/// One selected term's local eligible pages.
#[derive(Debug, Serialize)]
pub struct TaxonomyGroup<'a> {
    /// Exact metadata from the same taxonomy namespace.
    pub term: &'a TaxonomyTerm,
    /// Present only when this term has an explicitly configured listing route.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    /// Existing content references, not copied content bodies or generated memberships.
    pub pages: Vec<TaxonomyLink>,
}

/// A canonical reference to an existing eligible page.
#[derive(Debug, Serialize)]
pub struct TaxonomyLink {
    /// Original content identity.
    pub id: Key,
    /// Original authored title.
    pub title: String,
    /// Original canonical route; aliases and mirrors never enter the listing.
    pub canonical_url: String,
    /// Explicit authored language, without taxonomy-language inheritance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
}

/// A complete listing cannot be projected from these exact owners and bindings.
#[derive(Debug)]
pub enum TaxonomyError {
    /// The discovery belongs to another resolved content owner.
    Discovery,
    /// The native site selection could not complete.
    Site(SitePagesError),
    /// Configured catalog identity or term is absent; no label fallback exists.
    Catalog(Key),
    /// Configured listings lack their explicit public base.
    Base,
}

impl TaxonomyError {
    /// Stable diagnostic code, independent of display formatting.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Discovery => "taxonomy_discovery_owner",
            Self::Site(error) => error.code(),
            Self::Catalog(_) => "taxonomy_catalog_reference",
            Self::Base => "taxonomy_public_base",
        }
    }
}
impl fmt::Display for TaxonomyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery => f.write_str("taxonomy discovery belongs to another resolved owner"),
            Self::Site(error) => error.fmt(f),
            Self::Catalog(key) => write!(
                f,
                "taxonomy listing references absent catalog identity {key}"
            ),
            Self::Base => f.write_str("taxonomy listings require an explicit public base"),
        }
    }
}
impl std::error::Error for TaxonomyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Site(error) => Some(error),
            _ => None,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Project only explicitly configured listings from the shared discovery selection.
    /// Empty selected-term listings remain deliberate outputs, not inferred catalog exports.
    ///
    /// # Errors
    /// Rejects foreign discovery owners, missing site routes and unresolved catalog bindings.
    pub fn project_taxonomy_pages(
        &self,
        site: &Key,
        discovery: &Discovery<'_>,
    ) -> Result<Vec<ProjectedTaxonomyPage<'_>>, TaxonomyError> {
        if !discovery.matches(self) {
            return Err(TaxonomyError::Discovery);
        }
        let workspace = &self.content().compilation().workspace;
        let publication = workspace
            .publication_for(site)
            .ok_or(TaxonomyError::Site(SitePagesError::BrandRequired))?;
        if publication.taxonomy_pages.is_empty() {
            return Ok(Vec::new());
        }
        let base = self.content().base_url(site).ok_or(TaxonomyError::Base)?;
        let selected = self
            .project_site_pages(site)
            .map_err(TaxonomyError::Site)?
            .discoverable(discovery)
            .map_err(TaxonomyError::Site)?;
        let mut listings = Vec::new();
        for configured in &publication.taxonomy_pages {
            let taxonomy = workspace
                .taxonomy(&configured.taxonomy)
                .ok_or_else(|| TaxonomyError::Catalog(configured.taxonomy.clone()))?;
            let term = configured
                .term
                .as_ref()
                .map(|key| {
                    taxonomy
                        .term(key)
                        .ok_or_else(|| TaxonomyError::Catalog(key.clone()))
                })
                .transpose()?;
            let mut groups: BTreeMap<&Key, TaxonomyGroup<'_>> = BTreeMap::new();
            let group = |term| TaxonomyGroup {
                term,
                canonical_url: publication
                    .taxonomy_pages
                    .iter()
                    .find(|page| {
                        page.taxonomy == configured.taxonomy && page.term.as_ref() == Some(&term.id)
                    })
                    .map(|page| base.resolve(&page.route)),
                pages: Vec::new(),
            };
            if let Some(term) = term {
                groups.insert(&term.id, group(term));
            }
            for page in selected.pages() {
                for membership in &page.node.taxonomies {
                    if membership.taxonomy != taxonomy.id
                        || term.is_some_and(|term| term.id != membership.term)
                    {
                        continue;
                    }
                    let term = taxonomy
                        .term(&membership.term)
                        .ok_or_else(|| TaxonomyError::Catalog(membership.term.clone()))?;
                    groups
                        .entry(&term.id)
                        .or_insert_with(|| group(term))
                        .pages
                        .push(TaxonomyLink {
                            id: page.node.id.clone(),
                            title: page.node.title.clone(),
                            canonical_url: page.route.canonical_url.clone(),
                            language: page.node.language.clone(),
                        });
                }
            }
            listings.push(ProjectedTaxonomyPage {
                template: configured.template.clone(),
                model: TaxonomyListing {
                    taxonomy: self.taxonomy_view(taxonomy, site, site),
                    term,
                    title: term.map_or(taxonomy.label.as_str(), |term| term.label.as_str()),
                    canonical_url: base.resolve(&configured.route),
                    output: configured.route.output().clone(),
                    groups: groups.into_values().collect(),
                },
            });
        }
        listings.sort_by(|left, right| left.model.output.cmp(&right.model.output));
        Ok(listings)
    }
}

impl TaxonomyListing<'_> {
    /// Project the same eligible references without inventing publisher, author or content IDs.
    pub fn json_ld(&self) -> serde_json::Value {
        let pages: BTreeSet<_> = self
            .groups
            .iter()
            .flat_map(|group| &group.pages)
            .map(|page| &page.canonical_url)
            .collect();
        let mut value = serde_json::json!({
            "@context":"https://schema.org", "@type":"CollectionPage",
            "@id":self.canonical_url, "name":self.title,
            "hasPart":pages.into_iter().map(|url| serde_json::json!({"@id":url})).collect::<Vec<_>>(),
        });
        if let Some(language) = self.taxonomy.language {
            value["inLanguage"] = serde_json::json!(language);
        }
        value
    }
}
