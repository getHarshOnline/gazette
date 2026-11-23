//! Explicit project resources; source identity and hosted availability remain separate.
use super::Key;
use serde::{Deserialize, Serialize};

/// Owner-configured destinations, independent from repository acquisition and release state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectResources {
    /// Exact canonical project-family page or root help collection, directly About this project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing: Option<Key>,
    /// Existing authored documentation page or the same project root help collection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub learn: Option<Key>,
    /// Explicit GitHub navigation destination when no repository is declared.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<String>,
    /// Existing generated all-residence project-reference article index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mentions: Option<Key>,
    /// Existing generated project-article index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blogs: Option<Key>,
    /// Existing generated collected-article index for this project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thoughts: Option<Key>,
    /// Explicit public HTTPS code-manual address; no availability assertion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_documentation: Option<String>,
}
