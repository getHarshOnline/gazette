//! Exact document framing with the SDK's selected Markdown adapter.

pub use gazette_compiler::document::{Error, Inspection, Limits, ParsedDocument};

/// Complete explicitly framed document inspection and optional body HTML.
/// Metadata remains syntax evidence and cannot grant rendering or publication rights.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct Report {
    /// Full-document, metadata and body identities with their native coordinates.
    pub document: Inspection,
    /// Absent unless requested; failure of this projection rejects the whole report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// Parse one explicitly framed document and produce all requested projections.
/// Whole-source, metadata, body and output bounds retain their independent meanings.
///
/// # Errors
/// Preserves framing, parser and HTML safety failures without a partial report or fallback.
pub fn inspect(source: &str, limits: Limits, include_html: bool) -> Result<Report, Error> {
    let parsed = parse(source, limits)?;
    let document = parsed.inspect()?;
    let html = include_html.then(|| parsed.render_html()).transpose()?;
    Ok(Report { document, html })
}

/// Parse explicitly framed source through the same adapter as ordinary Markdown.
///
/// # Errors
/// Preserves framing, metadata and body failures with original coordinates.
pub fn parse(source: &str, limits: Limits) -> Result<ParsedDocument<'_>, Error> {
    ParsedDocument::parse(source, limits, &gazette_adapter_markdown::Comrak)
}
