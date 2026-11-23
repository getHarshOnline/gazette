//! Typed shallow project-resource facts crossing the Markdown component port.
use super::{LinkDestinationKind, LinkRelationship};
use crate::publication::Key;
use serde::Serialize;

/// Semantic purpose, without visible labels or presentation recipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectResourceKind {
    /// Authored use cases and explanation.
    Learn,
    /// Explicit GitHub navigation destination, independent from repository completeness.
    Github,
    /// Native combined reference article index for this project.
    Mentions,
    /// Native article index for this project.
    Blogs,
    /// Native collected-article index for this project.
    Thoughts,
    /// Explicit code-documentation endpoint.
    CodeDocumentation,
    /// The project's explicitly selected acquisition or enquiry relationship.
    AvailableThrough,
}

/// Minimal presentation facts; this does not claim a captured or available theme.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProjectResourcePresentation {
    /// Exact configured theme identity.
    pub theme: Key,
    /// Exact optional palette.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<Key>,
}

/// Existing native resolved-link facts, lowered without URL reconstruction.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProjectResourceDestination {
    /// Native link classification relative to the hosting page.
    pub kind: LinkDestinationKind,
    /// Exact known content target.
    pub content: Option<Key>,
    /// Exact configured destination site.
    pub site: Option<Key>,
    /// Already resolved direct editorial relations.
    pub relationships: Vec<LinkRelationship>,
    /// Already selected destination presentation.
    pub presentation: Option<ProjectResourcePresentation>,
}

/// One configured resource, not a reachability or release claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProjectResourceAction {
    /// Stable semantic purpose; theme owns visible copy.
    pub kind: ProjectResourceKind,
    /// Actual selected serving endpoint or explicit public address.
    pub url: String,
    /// Native destination facts for shared anchor presentation.
    pub destination: ProjectResourceDestination,
}

/// Resources of one exact directly asserted project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProjectResourcesView {
    /// Existing configured project identity.
    pub project: Key,
    /// Explicit name, never inferred from an identifier or URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Available code-documentation, Learn, GitHub and Mentions actions, followed by independently configured article views.
    pub actions: Vec<ProjectResourceAction>,
}
