//! Alternate routes belong to the same canonical rendering selection, not a sitemap query.

use std::{
    collections::BTreeSet,
    io::{self, Write},
};

use gazette_core::{publication::routing::RoutePath, render::LimitedWriter, source::EntryPath};
use serde::Serialize;

use super::{SitePages, SitePagesError};
use crate::{
    authored::{ListingPage, ProjectedTaxonomyPage},
    workspace::{AlternateRelation, MarkdownAlternates, MarkdownMediaType},
};

/// One verified alternate of an actual served canonical representation.
/// No source path or invented generated-view content identity is exposed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MarkdownAlternate {
    /// Actual serving HTML address, not a syndicated original's address.
    pub html_url: String,
    /// Native normalized site-relative Markdown address.
    pub route: RoutePath,
    /// Address under the same public or explicitly local serving base.
    pub url: String,
    /// Literal target without any deployment prefix.
    pub output: EntryPath,
    /// Truthful producer media profile.
    pub media_type: MarkdownMediaType,
    /// Configured human-page discovery relationship.
    pub discovery_rel: AlternateRelation,
}

impl SitePages<'_> {
    /// Plan all rendered canonical alternates, independently from discoverability.
    /// Generated inputs must exactly match the configured complete views.
    ///
    /// # Errors
    /// Rejects a discovery-filtered materialization subset, incomplete generated views,
    /// native route collisions, and complete serialized-plan byte exhaustion.
    pub fn markdown_alternates(
        &self,
        taxonomies: &[ProjectedTaxonomyPage<'_>],
        listings: &[ListingPage],
        maximum_bytes: usize,
    ) -> Result<Vec<MarkdownAlternate>, SitePagesError> {
        let Some(profile) = self
            .publication
            .machine
            .markdown_alternates
            .as_ref()
            .filter(|p| p.enabled)
        else {
            return Ok(Vec::new());
        };
        let expected: BTreeSet<_> = self
            .owner
            .graph()
            .nodes()
            .filter(|node| node.site == self.publication.site)
            .map(|node| &node.id)
            .collect();
        let actual: BTreeSet<_> = self.pages.iter().map(|page| &page.node.id).collect();
        if actual != expected || actual.len() != self.pages.len() {
            return Err(SitePagesError::Machine(
                "alternates require the complete rendered page selection",
            ));
        }
        self.with_taxonomy_pages(taxonomies)?
            .with_listing_pages(listings)?;
        let base = self
            .owner
            .base_url(&self.publication.site)
            .ok_or(SitePagesError::BrandRequired)?;
        let mut occupied = BTreeSet::new();
        let mut urls = BTreeSet::new();
        for node in self.owner.graph().nodes() {
            if let Some(route) = self.owner.route(&node.id) {
                if node.site == self.publication.site {
                    occupied.insert(route.output.clone());
                    urls.insert(route.url.clone());
                    for alias in &route.aliases {
                        occupied.insert(alias.output.clone());
                        urls.insert(alias.url.clone());
                    }
                }
                for mirror in route
                    .mirrors
                    .iter()
                    .filter(|mirror| mirror.site == self.publication.site)
                {
                    occupied.insert(mirror.output.clone());
                    urls.insert(mirror.url.clone());
                }
            }
        }
        for taxonomy in taxonomies {
            occupied.insert(taxonomy.model.output.clone());
            urls.insert(taxonomy.model.canonical_url.clone());
        }
        for listing in listings.iter().filter(|page| !page.is_primary()) {
            occupied.insert(listing.output.clone());
            urls.insert(listing.url.clone());
        }
        let mut candidates = Vec::new();
        for page in &self.pages {
            let declared = page
                .node
                .route
                .as_ref()
                .ok_or_else(|| SitePagesError::RouteRequired(page.node.id.clone()))?;
            if declared.canonical.is_none() {
                candidates.push((page.route.url.as_str(), &declared.path));
            }
        }
        for listing in listings.iter().filter(|page| !page.is_primary()) {
            candidates.push((listing.url.as_str(), &listing.route));
        }
        for taxonomy in taxonomies {
            let declaration = self
                .publication
                .taxonomy_pages
                .iter()
                .find(|declaration| {
                    &declaration.taxonomy == taxonomy.model.taxonomy.id
                        && declaration.term.as_ref() == taxonomy.model.term.map(|term| &term.id)
                })
                .ok_or(SitePagesError::Taxonomy)?;
            candidates.push((taxonomy.model.canonical_url.as_str(), &declaration.route));
        }
        candidates.sort_unstable_by_key(|(url, _)| *url);
        let mut output = LimitedWriter::new(io::sink(), maximum_bytes);
        output
            .write_all(b"[")
            .map_err(|_| SitePagesError::Limit("alternate plan bytes"))?;
        let mut result = Vec::new();
        for (html_url, path) in candidates {
            let alternate = alternate(profile, base, html_url, path)?;
            reserve(&mut occupied, &alternate.output)?;
            if !urls.insert(alternate.url.clone()) {
                return Err(SitePagesError::Collision);
            }
            if !result.is_empty() {
                output
                    .write_all(b",")
                    .map_err(|_| SitePagesError::Limit("alternate plan bytes"))?;
            }
            if let Err(error) = serde_json::to_writer(&mut output, &alternate) {
                if output.exceeded() {
                    return Err(SitePagesError::Limit("alternate plan bytes"));
                }
                return Err(SitePagesError::Write(io::Error::other(error)));
            }
            result.push(alternate);
        }
        output
            .write_all(b"]")
            .map_err(|_| SitePagesError::Limit("alternate plan bytes"))?;
        Ok(result)
    }
}

pub(super) fn alternate(
    profile: &MarkdownAlternates,
    base: &gazette_core::publication::routing::PublicBaseUrl,
    html_url: &str,
    path: &RoutePath,
) -> Result<MarkdownAlternate, SitePagesError> {
    let projected = profile
        .project(base, path)?
        .ok_or(SitePagesError::Machine("alternate profile is disabled"))?;
    if projected.html_url != html_url {
        return Err(SitePagesError::Machine(
            "alternate serving route does not match the resolved base",
        ));
    }
    Ok(projected)
}

impl MarkdownAlternates {
    /// Project one exact serving route for page-level Theme discovery.
    /// The complete site planner additionally proves selection and output collisions.
    ///
    /// # Errors
    /// Rejects an alternate route that cannot use the native publication route grammar.
    pub fn project(
        &self,
        base: &gazette_core::publication::routing::PublicBaseUrl,
        path: &RoutePath,
    ) -> Result<Option<MarkdownAlternate>, SitePagesError> {
        if !self.enabled {
            return Ok(None);
        }
        let route = if path.as_str().ends_with('/') {
            path.join_file(&"index.md".parse().map_err(SitePagesError::Output)?)
        } else {
            format!("{}.md", path.as_str()).parse()
        }
        .map_err(|_| SitePagesError::Machine("invalid alternate route"))?;
        Ok(Some(MarkdownAlternate {
            html_url: base.resolve(path),
            url: base.resolve(&route),
            output: route.output().clone(),
            route,
            media_type: self.media_type,
            discovery_rel: self.discovery_rel,
        }))
    }
}

fn reserve(occupied: &mut BTreeSet<EntryPath>, path: &EntryPath) -> Result<(), SitePagesError> {
    let prefix = format!("{path}/");
    if occupied.contains(path)
        || path
            .as_str()
            .match_indices('/')
            .any(|(offset, _)| occupied.contains(&path.as_str()[..offset]))
        || occupied
            .range::<str, _>((
                std::ops::Bound::Included(prefix.as_str()),
                std::ops::Bound::Unbounded,
            ))
            .next()
            .is_some_and(|other| other.as_str().starts_with(&prefix))
    {
        return Err(SitePagesError::Collision);
    }
    occupied.insert(path.clone());
    Ok(())
}
