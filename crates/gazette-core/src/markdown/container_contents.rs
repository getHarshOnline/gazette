//! Resolved container presentation on one admitted render-plan occurrence.

use crate::publication::Key;
use serde::Serialize;

/// Existing structural order, without a new content identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ContainerContentsKind {
    /// Direct collection children in their resolved order.
    Collection,
    /// Explicit journey steps in their declared order.
    Journey,
}

/// Destination presentation copied from the selected home site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ContainerContentsPresentation {
    /// Exact selected Theme package key.
    pub theme: Key,
    /// Optional selected palette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Key>,
}

/// Already resolved decorative artwork, never inferred from filenames.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ContainerContentsArtwork {
    /// Required default rendition.
    pub default: String,
    /// Optional light rendition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<String>,
    /// Optional dark rendition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<String>,
}

/// Shallow card facts from an existing child or step.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ContainerContentsEntry {
    /// Existing content identity.
    pub id: Key,
    /// Existing publication home.
    pub site: Key,
    /// Authored display title.
    pub title: String,
    /// Optional authored description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Existing serving address; absent endpoints stay unlinked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Selected destination Theme and palette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presentation: Option<ContainerContentsPresentation>,
    /// Decorative structural artwork.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<ContainerContentsArtwork>,
}

/// Balanced authored grouping over displayed direct children only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContainerContentsEvent {
    /// Display the exact matching card from the same view.
    Entry {
        /// Existing child identity, not another content record.
        content: Key,
    },
    /// Begin an authored group without creating a container.
    GroupOpen {
        /// Original plain-text label.
        title: String,
    },
    /// End the matching nonempty group.
    GroupClose,
}

/// Pure presentation of already resolved container membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ContainerContentsView {
    /// Existing collection or journey semantics.
    pub kind: ContainerContentsKind,
    /// Existing documentation category, used for Theme labels.
    pub documentation: bool,
    /// Direct children or steps; never recursively copied content.
    pub entries: Vec<ContainerContentsEntry>,
    /// Authored grouping when the container governs its documentation outline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<Vec<ContainerContentsEvent>>,
}

/// One plan-only payload; public component-use metadata never contains it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ContainerContentsBinding {
    /// Existing direct-page presentation exposed to the selected static template.
    pub presentation: ContainerContentsView,
    /// Bounded compiler-written Markdown under the discovery reading policy.
    /// This is not authored text and must not enter search or template context.
    #[serde(skip)]
    #[schemars(skip)]
    pub reading_markdown: Option<String>,
}
