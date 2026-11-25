//! One scoped taxonomy join for page and graph consumers; no catalog-wide export.

use gazette_core::{
    markdown::LinkDestinationKind,
    publication::{Key, LanguageTag, Taxonomy, TaxonomyTerm, content::ContentNode},
};
use serde::Serialize;

use super::{ProjectionError, ResolvedAuthored, SiteGraphError};

/// Taxonomy identity and label without unrelated terms from the workspace catalog.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct TaxonomyView<'a> {
    /// Exact configured taxonomy key, not its display label or slug.
    pub id: &'a Key,
    /// Literal configured display label; presentation performs contextual escaping.
    pub label: &'a str,
    /// Exact optional language constraint, not a language inferred for the page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<&'a LanguageTag>,
    /// Explicitly configured taxonomy index, relative to the rendering publication.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<ClassificationDestination>,
}

/// One declared classification with its exact bound labels; no relation inference.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ClassificationView<'a> {
    /// Selected taxonomy metadata, never its complete term catalog.
    pub taxonomy: TaxonomyView<'a>,
    /// Selected term in that taxonomy's namespace.
    pub term: &'a TaxonomyTerm,
    /// Exact configured term listing; no taxonomy-index or label-derived fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<ClassificationDestination>,
}

/// A configured taxonomy listing address, not a new content or concept identity.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ClassificationDestination {
    /// Selected public or validated local-preview address for navigation.
    pub url: String,
    /// Stable address under the explicitly configured public base.
    pub canonical_url: String,
    /// Native relationship and world facts relative to the rendering publication.
    pub destination: crate::markdown::LinkDestination,
}

pub(super) enum ClassificationError {
    Binding(Key),
    Limit,
}
impl ClassificationError {
    pub(super) fn projection(self) -> ProjectionError {
        match self {
            Self::Binding(id) => ProjectionError::Binding {
                id,
                reason: "bound taxonomy classification missing",
            },
            Self::Limit => ProjectionError::Limit("classification destination URI bytes"),
        }
    }
    pub(super) fn graph(self) -> SiteGraphError {
        match self {
            Self::Binding(id) => SiteGraphError::Binding(id),
            Self::Limit => SiteGraphError::Limit("classification destination URI bytes"),
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    fn classification_destination(
        &self,
        source_site: &Key,
        owner_site: &Key,
        taxonomy: &Key,
        term: Option<&Key>,
    ) -> Option<ClassificationDestination> {
        let publication = self
            .content
            .compilation()
            .workspace
            .publication_for(owner_site)?;
        let page = publication
            .taxonomy_pages
            .iter()
            .find(|page| &page.taxonomy == taxonomy && page.term.as_ref() == term)?;
        let serving = self.content.base_url(owner_site)?;
        let public = self
            .content
            .compilation()
            .workspace
            .public_base_url(owner_site)?;
        Some(ClassificationDestination {
            url: serving.resolve(&page.route),
            canonical_url: public.resolve(&page.route),
            destination: crate::markdown::LinkDestination {
                kind: if source_site == owner_site {
                    LinkDestinationKind::InternalNative
                } else {
                    LinkDestinationKind::CrossSite
                },
                content: None,
                site: Some(owner_site.clone()),
                site_name: None,
                relationships: Vec::new(),
                presentation: self
                    .content
                    .presentation_for(owner_site)
                    .map(super::SitePresentation::from),
            },
        })
    }

    pub(super) fn taxonomy_view<'a>(
        &'a self,
        taxonomy: &'a Taxonomy,
        owner_site: &Key,
        source_site: &Key,
    ) -> TaxonomyView<'a> {
        TaxonomyView {
            id: &taxonomy.id,
            label: &taxonomy.label,
            language: taxonomy.language.as_ref(),
            destination: self.classification_destination(
                source_site,
                owner_site,
                &taxonomy.id,
                None,
            ),
        }
    }

    pub(super) fn classification_views(
        &self,
        node: &ContentNode,
    ) -> Result<Vec<ClassificationView<'_>>, ClassificationError> {
        let mut remaining = self.projection_limits.text_bytes;
        self.classification_views_from(node, &node.site, &mut remaining)
    }

    pub(super) fn classification_views_from(
        &self,
        node: &ContentNode,
        source_site: &Key,
        remaining: &mut usize,
    ) -> Result<Vec<ClassificationView<'_>>, ClassificationError> {
        // Address fan-out is new owned data. Charge its complete raw byte count
        // before allocating the repeated destination strings; final JSON retains
        // its existing independently bounded serializer.
        if let (Some(publication), Some(serving), Some(public)) = (
            self.content
                .compilation()
                .workspace
                .publication_for(&node.site),
            self.content.base_url(&node.site),
            self.content
                .compilation()
                .workspace
                .public_base_url(&node.site),
        ) {
            for membership in &node.taxonomies {
                for term in [None, Some(&membership.term)] {
                    if let Some(page) = publication.taxonomy_pages.iter().find(|page| {
                        page.taxonomy == membership.taxonomy && page.term.as_ref() == term
                    }) {
                        for base in [serving, public] {
                            for size in [
                                base.as_str().trim_end_matches('/').len(),
                                page.route.as_str().len(),
                            ] {
                                *remaining = remaining
                                    .checked_sub(size)
                                    .ok_or(ClassificationError::Limit)?;
                            }
                        }
                    }
                }
            }
        }
        node.taxonomies
            .iter()
            .map(|membership| {
                let taxonomy = self
                    .content
                    .compilation()
                    .workspace
                    .taxonomy(&membership.taxonomy)
                    .ok_or_else(|| ClassificationError::Binding(membership.taxonomy.clone()))?;
                let term = taxonomy
                    .term(&membership.term)
                    .ok_or_else(|| ClassificationError::Binding(membership.term.clone()))?;
                Ok(ClassificationView {
                    taxonomy: self.taxonomy_view(taxonomy, &node.site, source_site),
                    term,
                    destination: self.classification_destination(
                        source_site,
                        &node.site,
                        &taxonomy.id,
                        Some(&term.id),
                    ),
                })
            })
            .collect()
    }
}
