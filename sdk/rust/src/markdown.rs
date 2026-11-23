//! Markdown operations over the SDK's selected adapter, without exposing its implementation.

pub use gazette_compiler::markdown::{
    ComponentPresentation, ComponentUse, Error, Heading, HeadingReference, Inspection, Limits,
    Link, LinkKind, ParsedMarkdown, RenderedMarkdown, Summary,
};

#[cfg(feature = "native-local")]
pub mod input;

/// Complete supplied-source inspection and an explicitly requested HTML fragment.
/// This is syntax evidence, not publication admission or a certified artifact.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct Report {
    /// Native inspection with exact source identity and UTF-8 byte coordinates.
    pub inspection: Inspection,
    /// Absent unless requested; a failed fragment never yields a partial report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// Parse once and produce the complete requested supplied-source projections.
/// Framing is never inferred; use `document::inspect` for explicit TOML metadata.
///
/// # Errors
/// Preserves native syntax, resource and HTML safety failures without partial output.
pub fn inspect(source: &str, limits: Limits, include_html: bool) -> Result<Report, Error> {
    let parsed = parse(source, limits)?;
    let inspection = parsed.inspect()?;
    let html = include_html.then(|| parsed.render_html()).transpose()?;
    Ok(Report { inspection, html })
}

/// Parse exact supplied Markdown with the same adapter used by publication builds.
///
/// # Errors
/// Preserves parser, coordinate and resource failures without partial syntax.
pub fn parse(source: &str, limits: Limits) -> Result<ParsedMarkdown<'_>, Error> {
    ParsedMarkdown::parse(source, limits, &gazette_adapter_markdown::Comrak)
}
