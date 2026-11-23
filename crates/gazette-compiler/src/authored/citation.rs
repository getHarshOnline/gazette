//! Citation metadata resolves through the same confined source and serving routes as body links.

use super::{AssetPlan, Origin, ResolvedAuthored, links::ResolvedReference};
use crate::{
    markdown::{self, LinkForm, LinkKind},
    toml::PathSegment,
};
use gazette_core::{
    publication::{Key, content::CitationFormat},
    source::Span,
};
use serde::Serialize;
use std::fmt;

/// Authored text and compiler-resolved optional downloads.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CitationView<'a> {
    /// Plain authored text, never HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<&'a str>,
    /// Plain labelled formats in authored order.
    pub formats: &'a [CitationFormat],
    /// Resolved BibTeX destination, not proof of external availability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bibtex: Option<String>,
    /// Native destination facts for the same resolved BibTeX URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bibtex_destination: Option<markdown::LinkDestination>,
    /// Resolved PDF destination, not a generated document.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf: Option<String>,
    /// Native destination facts for the same resolved PDF URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_destination: Option<markdown::LinkDestination>,
}

/// A citation field failed; coordinates refer to frontmatter, never a fabricated body link.
#[derive(Debug)]
pub struct CitationError {
    /// Exact content owner.
    pub content: Key,
    /// Authored citation field.
    pub field: &'static str,
    /// Original retained field coordinates, when available.
    pub origin: Option<Origin>,
    /// Resolution or URL-policy failure.
    pub reason: String,
}
impl CitationError {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        "authored_citation"
    }
}
impl fmt::Display for CitationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "citation {}.{}: {}",
            self.content, self.field, self.reason
        )
    }
}
impl std::error::Error for CitationError {}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn citation_reference<'a>(
        &self,
        id: &Key,
        assets: &'a AssetPlan<'a>,
        field: &'static str,
        destination: &str,
    ) -> Result<ResolvedReference<'a>, CitationError> {
        let origin = self.origin(
            id,
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("citation".into()),
                PathSegment::Key(field.into()),
            ],
        );
        let fail = |reason: String| CitationError {
            content: id.clone(),
            field,
            origin: origin.clone(),
            reason,
        };
        let span: Span = origin
            .as_ref()
            .and_then(|o| o.span)
            .ok_or_else(|| fail("authored citation field coordinates missing".into()))?;
        if !markdown::allowed_url(destination, true) || destination.trim() != destination {
            return Err(fail("unsafe citation destination".into()));
        }
        let end = destination.find(['?', '#']).unwrap_or(destination.len());
        let path = &destination[..end];
        if path.is_empty() {
            return Err(fail("citation download requires a destination".into()));
        }
        if !markdown::source_path::is_external(path) {
            let parts =
                markdown::source_path::decode(path, span).map_err(|e| fail(e.to_string()))?;
            let extension = if field == "bibtex" { "bib" } else { "pdf" };
            if !parts
                .last()
                .and_then(|p| p.rsplit_once('.'))
                .is_some_and(|(_, e)| e.eq_ignore_ascii_case(extension))
            {
                return Err(fail(
                    "local citation download must name a retained file of its declared format"
                        .into(),
                ));
            }
        }
        let resolved = self
            .resolve_reference(
                id,
                assets,
                LinkForm::Markdown,
                LinkKind::Link,
                destination,
                span,
            )
            .map_err(|e| fail(e.to_string()))?;
        let mapped = resolved
            .as_ref()
            .map_or(destination, |r| r.destination.as_str());
        let mapped = match self.content.route(id) {
            Some(route) => url::Url::parse(&route.url)
                .and_then(|base| base.join(mapped))
                .map(String::from)
                .map_err(|_| fail("citation cannot resolve against serving URL".into()))?,
            None => mapped.to_owned(),
        };
        Ok(ResolvedReference {
            asset_path: resolved.as_ref().and_then(|r| r.asset_path.clone()),
            destination: mapped,
            asset: resolved.as_ref().and_then(|r| r.asset),
            content: resolved.as_ref().and_then(|r| r.content.clone()),
            resource: resolved.as_ref().is_some_and(|r| r.resource),
        })
    }
}
