//! Public contract-schema discovery over Gazette's native typed owners.

/// Stable selector for a currently generated contract schema.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Contract {
    /// Complete Theme-package declaration.
    ThemeManifest,
    /// Exact page context supplied to Theme templates.
    TemplateContext,
    /// Complete unresolved workspace declaration.
    Workspace,
    /// Exact authored publication frontmatter, excluding Markdown fences and body.
    AuthoredContent,
    /// Reduced owner-held legal frontmatter, not a composed site notice or approval.
    LegalSource,
}

/// Generate a Draft 2020-12 schema from the native compiler/application type.
#[must_use]
pub fn contract(contract: Contract) -> schemars::Schema {
    match contract {
        Contract::ThemeManifest => gazette_compiler::contracts::theme_manifest(),
        Contract::TemplateContext => gazette_application::build::template_context_schema(),
        Contract::Workspace => gazette_compiler::contracts::workspace(),
        Contract::AuthoredContent => gazette_compiler::contracts::authored_content(),
        Contract::LegalSource => gazette_compiler::contracts::legal_source(),
    }
}
