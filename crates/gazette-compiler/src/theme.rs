//! Theme-owned render and component contracts, independent of package acquisition.

use std::collections::BTreeMap;

use gazette_core::{
    publication::{
        Key,
        content::{ContentKind, ContentNode, PageFamily},
    },
    source::EntryPath,
};
use serde::{Deserialize, Serialize};

mod components;
pub use components::{Component, Components};
mod social_card;
pub use social_card::{SocialCardMediaKind, SocialCardRecipe};
mod semantics;
pub use semantics::ComponentSemantic;
mod interaction;
pub use interaction::{
    ComponentExport, ComponentInteraction, ComponentMount, InvalidComponentExport,
};

/// Theme-owned ordinary rendering entries, independent of site output placement.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenderRecipes {
    /// Default entry for pages without an explicit controlled template override.
    pub page: EntryPath,
    /// Entries selected by the content's declared structure or page family.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub kinds: BTreeMap<PageKind, EntryPath>,
    /// Entries for explicit extension presentation modes, before kind defaults.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub modes: BTreeMap<Key, EntryPath>,
    /// Entry for the exact content selected as the publication's home.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub home: Option<EntryPath>,
    /// Entry for an index whose declared listing selects root collections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_directory: Option<EntryPath>,
    /// Redirect/mirror entry; omission rejects a selected alias rather than guessing.
    pub alias: Option<EntryPath>,
    /// Shared graph-view entry; omission rejects a requested graph output.
    pub graph: Option<EntryPath>,
    /// Optional machine reading-guide entry, inherited from an explicit dependency.
    pub llms: Option<EntryPath>,
    /// Optional inherited page for an unknown address, emitted at 404.html.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_found: Option<EntryPath>,
    /// Optional inherited recipe for a generated publication-home link preview.
    #[serde(
        default,
        deserialize_with = "crate::tables::optional",
        skip_serializing_if = "Option::is_none"
    )]
    pub social_card: Option<SocialCardRecipe>,
}

impl RenderRecipes {
    /// Select the declared presentation mode or kind, then the ordinary page entry.
    pub fn page_for(&self, structure: &ContentKind) -> &EntryPath {
        self.mode_for(structure)
            .or_else(|| self.kinds.get(&PageKind::from(structure)))
            .unwrap_or(&self.page)
    }

    /// Select an entry from declared publication roles before the general kind fallback.
    pub fn page_in_publication(
        &self,
        node: &ContentNode,
        publication: &crate::workspace::BrandPublication,
    ) -> &EntryPath {
        self.mode_for(&node.structure)
            .or_else(|| {
                self.home
                    .as_ref()
                    .filter(|_| publication.home.as_ref() == Some(&node.id))
            })
            .or_else(|| {
                self.collection_directory.as_ref().filter(|_| {
                    publication.listings.iter().any(|listing| {
                        listing.content == node.id
                            && matches!(
                                listing.select,
                                crate::workspace::ListingSelection::RootCollections
                            )
                    })
                })
            })
            .unwrap_or_else(|| self.page_for(&node.structure))
    }

    fn mode_for(&self, structure: &ContentKind) -> Option<&EntryPath> {
        if let ContentKind::Page {
            family:
                PageFamily::Extension {
                    presentation: Some(presentation),
                    ..
                },
        } = structure
            && let Some(entry) = self.modes.get(&presentation.mode)
        {
            return Some(entry);
        }
        None
    }

    /// Every declared render entry for retained-template closure validation.
    pub fn entries(&self) -> impl Iterator<Item = &EntryPath> {
        std::iter::once(&self.page)
            .chain(self.kinds.values())
            .chain(self.modes.values())
            .chain(self.home.as_ref())
            .chain(self.collection_directory.as_ref())
            .chain(self.alias.as_ref())
            .chain(self.graph.as_ref())
            .chain(self.llms.as_ref())
            .chain(self.not_found.as_ref())
            .chain(self.social_card.as_ref().map(|recipe| &recipe.template))
    }
}

/// Content distinctions available to a Theme's default template selection.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum PageKind {
    /// Authored contribution.
    Article,
    /// The existing explicit documentation page family.
    Documentation,
    /// Project landing.
    Project,
    /// Index or landing.
    Index,
    /// Legal, about, contact or other utility page.
    Utility,
    /// A page with a separately declared domain schema.
    Extension,
    /// Editorial or documentation container.
    Collection,
    /// Ordered journey.
    Journey,
}

impl From<&ContentKind> for PageKind {
    fn from(structure: &ContentKind) -> Self {
        match structure {
            ContentKind::Collection { .. } => Self::Collection,
            ContentKind::Journey { .. } => Self::Journey,
            ContentKind::Page { family } => match family {
                PageFamily::Article => Self::Article,
                PageFamily::Documentation => Self::Documentation,
                PageFamily::Project => Self::Project,
                PageFamily::Index => Self::Index,
                PageFamily::Utility => Self::Utility,
                PageFamily::Extension { .. } => Self::Extension,
            },
        }
    }
}
