//! One browsing-location value for declared paths and generated index slices.

use std::io;

use gazette_core::{
    publication::{
        Key,
        content::{ContentKind, PageFamily},
    },
    render::LimitedWriter,
};
use serde::Serialize;

use super::{Discovery, ProjectionError, ResolvedAuthored};

/// The exact source of an offered browsing path, never a containment assertion.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BrowsingKind {
    /// Direct association declared by the selected page's Content metadata.
    Declared,
    /// Exact selected page or editorial ancestor in a generated index slice.
    Listing,
}

/// A shallow resolved browsing destination, independent of editorial residence.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct BrowsingLocation<'a> {
    /// Existing directory/index content identity.
    pub index: &'a Key,
    /// Declaring page or exact generated-listing item; never an inferred parent.
    pub via: &'a Key,
    /// Literal directory title; presentation may retain its configured navigation label.
    pub title: &'a str,
    /// Compiler-resolved serving URL, including any selected preview base.
    pub url: &'a str,
    /// Declared path or generated index slice.
    pub kind: BrowsingKind,
    /// One-based generated slice only; absent for authored directory paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<usize>,
    /// Complete generated slice count only; absent for authored directory paths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<usize>,
}

impl ResolvedAuthored<'_, '_> {
    /// Resolve a page's directly authored browse paths from the one graph and route table.
    ///
    /// # Errors
    /// Refuses unknown pages, foreign discovery owners or exhausted complete-view bounds.
    pub fn project_browsing_locations(
        &self,
        id: &Key,
        discovery: Option<&Discovery<'_>>,
    ) -> Result<Vec<BrowsingLocation<'_>>, ProjectionError> {
        self.project_browsing_locations_with(id, discovery, std::iter::empty())
    }

    /// Join existing generated locations and direct declarations under one complete
    /// per-page occurrence and encoded-text ceiling. No branch receives a fresh budget.
    ///
    /// # Errors
    /// Refuses the same binding errors as declared projection, or an exhausted union.
    pub fn project_browsing_locations_with<'a>(
        &'a self,
        id: &Key,
        discovery: Option<&Discovery<'_>>,
        listings: impl IntoIterator<Item = BrowsingLocation<'a>>,
    ) -> Result<Vec<BrowsingLocation<'a>>, ProjectionError> {
        if discovery.is_some_and(|view| !view.matches(self)) {
            return Err(ProjectionError::Binding {
                id: id.clone(),
                reason: "discovery belongs to another resolved source",
            });
        }
        let graph = self.content.graph();
        let source = graph
            .node(id)
            .ok_or_else(|| ProjectionError::UnknownContent(id.clone()))?;
        let eligible = |id: &Key| discovery.is_none_or(|view| view.contains(id));
        let mut result = Vec::new();
        let mut remaining = self.projection_limits.edges;
        if !eligible(id) {
            return Ok(result);
        }
        for mut location in listings {
            remaining = remaining
                .checked_sub(1)
                .ok_or(ProjectionError::Limit("browsing location occurrences"))?;
            // The existing listing DTO is in-process input, not disclosure
            // authority. Rebind its identities/title and serving endpoint to this
            // exact graph; suppressed endpoints cannot enter through the union.
            if !eligible(location.index) || !eligible(location.via) {
                continue;
            }
            let refuse = || ProjectionError::Binding {
                id: location.index.clone(),
                reason: "generated browsing location does not bind to this native graph",
            };
            let index = graph.node(location.index).ok_or_else(refuse)?;
            let via = graph.node(location.via).ok_or_else(refuse)?;
            if index.site != source.site
                || via.site != source.site
                || !matches!(
                    index.structure,
                    ContentKind::Page {
                        family: PageFamily::Index
                    }
                )
                || !graph
                    .editorial_ancestry(id)
                    .is_some_and(|mut path| path.any(|node| node.id == via.id))
                || !matches!(location.kind, BrowsingKind::Listing)
            {
                return Err(refuse());
            }
            let declaration = self
                .content
                .compilation()
                .workspace
                .publication_for(&source.site)
                .and_then(|publication| {
                    publication
                        .listings
                        .iter()
                        .find(|listing| listing.content == index.id)
                })
                .ok_or_else(refuse)?;
            // Search is an alternate view of the corpus, never a page's editorial ancestry.
            if matches!(
                declaration.select,
                crate::workspace::ListingSelection::AllPages
            ) {
                continue;
            }
            let candidate = graph.editorial_parent(&via.id).is_none()
                && via
                    .route
                    .as_ref()
                    .is_none_or(|route| route.canonical.is_none())
                && match &declaration.select {
                    crate::workspace::ListingSelection::StandaloneArticles => matches!(
                        via.structure,
                        ContentKind::Page {
                            family: PageFamily::Article
                        }
                    ),
                    crate::workspace::ListingSelection::ProjectArticles { .. }
                    | crate::workspace::ListingSelection::AllPages => false,
                    crate::workspace::ListingSelection::RootCollections => {
                        let mut remaining = self.projection_limits.edges;
                        self.listing_candidate(&declaration.select, index, via, &mut remaining)
                            .map_err(|_| refuse())?
                    }
                };
            let number = location.page_number.ok_or_else(refuse)?;
            let count = location.page_count.ok_or_else(refuse)?;
            if !candidate || number == 0 || number > count || count > self.projection_limits.nodes {
                return Err(refuse());
            }
            let authored_route = index
                .route
                .as_ref()
                .filter(|route| route.canonical.is_none())
                .ok_or_else(refuse)?;
            let native = self.content.route(&index.id).ok_or_else(refuse)?;
            if number == 1 {
                if location.url != native.url {
                    return Err(refuse());
                }
            } else {
                let continuation = declaration
                    .pagination
                    .continuation(number)
                    .map_err(|_| refuse())?;
                let path = authored_route
                    .path
                    .join_directory(Some(&continuation))
                    .map_err(|_| refuse())?;
                let base = self.content.base_url(&source.site).ok_or_else(refuse)?;
                if location.url != base.resolve(&path) {
                    return Err(refuse());
                }
            }
            location.title = &index.title;
            result.push(location);
        }
        for index in graph.browsing_indexes(id) {
            remaining = remaining
                .checked_sub(1)
                .ok_or(ProjectionError::Limit("browsing location occurrences"))?;
            if !eligible(index) {
                continue;
            }
            let node = graph
                .node(index)
                .ok_or_else(|| ProjectionError::UnknownContent(index.clone()))?;
            let route = self
                .content
                .route(index)
                .ok_or_else(|| ProjectionError::Binding {
                    id: index.clone(),
                    reason: "declared browsing index has no resolved route",
                })?;
            let location = BrowsingLocation {
                index: &node.id,
                via: &source.id,
                title: &node.title,
                url: &route.url,
                kind: BrowsingKind::Declared,
                page_number: None,
                page_count: None,
            };
            result.push(location);
        }
        let mut bytes = LimitedWriter::new(io::sink(), self.projection_limits.text_bytes);
        let encoded = serde_json::to_writer(&mut bytes, &result);
        if bytes.exceeded() {
            return Err(ProjectionError::Limit("browsing location JSON bytes"));
        }
        encoded.map_err(ProjectionError::Encoding)?;
        Ok(result)
    }
}
