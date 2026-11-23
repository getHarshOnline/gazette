//! Destination presentation selected by the resolved workspace.

use gazette_core::publication::{Key, content::ContentNode};
use serde::Serialize;
use std::ops::Deref;

use crate::{
    theme_package::ThemePackageId,
    workspace::{ResolvedRoute, ThemeSelection},
};

/// Minimal presentation identity for one configured site.
///
/// This is neither a provider binding nor proof that a Theme package was captured,
/// compiled or approved. Consumers use it to present an already resolved crossing
/// without inferring visual identity from a domain or content identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SitePresentation {
    /// Exact effective root Theme package selected by workspace composition.
    pub theme: ThemePackageId,
    /// Optional Theme-owned palette selected with that same binding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Key>,
}

impl From<&ThemeSelection> for SitePresentation {
    fn from(selection: &ThemeSelection) -> Self {
        Self {
            theme: selection.package.clone(),
            palette: selection.palette.clone(),
        }
    }
}

/// One content record plus its compiled destination and home-site presentation.
///
/// Flattening preserves authored metadata, including the original route declaration.
/// Resolved fields annotate a destination; they neither relocate it nor add meaning.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct ContentView<'a> {
    /// Whether a native listing declaration selects this content identity.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[schemars(default)]
    pub listing: bool,
    /// Existing semantic record from the resolved content graph.
    #[serde(flatten)]
    pub node: &'a ContentNode,
    /// Graph-owned documentation category, including collection descendants.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    #[schemars(default)]
    pub documentation: bool,
    /// Effective home-site presentation, absent when no Theme was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<SitePresentation>,
    /// Compiler-resolved serving and canonical addresses. The authored `route`
    /// remains unchanged; absence never authorizes a template to invent a URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_route: Option<&'a ResolvedRoute>,
    /// Decorative artwork for structural cards only; does not change node metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<super::ListingArtwork>,
}

impl<'a> ContentView<'a> {
    pub(super) fn new(
        node: &'a ContentNode,
        selection: Option<&'a ThemeSelection>,
        resolved_route: Option<&'a ResolvedRoute>,
        documentation: bool,
        listing: bool,
    ) -> Self {
        Self {
            listing,
            node,
            documentation,
            resolved_route,
            artwork: None,
            presentation: selection.map(SitePresentation::from),
        }
    }
}

impl Deref for ContentView<'_> {
    type Target = ContentNode;

    fn deref(&self) -> &Self::Target {
        self.node
    }
}
