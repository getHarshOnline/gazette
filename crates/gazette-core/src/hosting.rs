//! Explicit hosting intent, independent of delivery and account authority.

/// Hosting provider selected by a workspace for generated deployment files.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum HostingProvider {
    /// GitHub Pages intent, not evidence of a deployed site.
    GithubPages,
}
