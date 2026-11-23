//! Gazette's explicit excerpt admission; balanced formatting belongs to the adapter.

use super::{Error, ParsedMarkdown, Span};
use gazette_core::markdown::Html;
use gazette_core::source::Precision;
use serde::Serialize;

/// Explicit rendered prefix, never an inferred description or admission claim.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Summary {
    /// Balanced HTML without heading IDs or footnote machinery.
    pub html: String,
    /// Text from the same emitted prefix, not HTML stripping.
    pub text: String,
    /// Original marker's containing source range.
    pub marker: Span,
    /// Precision of the original marker evidence, not a generated HTML coordinate.
    pub marker_precision: Precision,
    /// URL-ready continuation; the publication compiler may qualify its base.
    pub continuation: String,
}

pub(super) struct Cut {
    pub marker: Html,
}

impl ParsedMarkdown<'_> {
    pub(super) fn summary_cut(&self) -> Result<Option<&Cut>, Error> {
        self.summary.get_or_init(|| {
            let mut cut = None;
            for html in &self.document.syntax().html {
                if !is_marker(&html.literal) { continue; }
                let span = html.location.span;
                if cut.is_some() { return Err(Error::Summary {
                    span, reason: "only one explicit summary boundary is supported",
                }); }
                if html.in_footnote_or_image { return Err(Error::Summary {
                    span, reason: "summary boundary cannot occur inside a footnote or image alt text",
                }); }
                cut = Some(Cut { marker: html.clone() });
            }
            Ok(cut)
        }).as_ref().map(Option::as_ref).map_err(Clone::clone)
    }
}

/// Only an entire parser-recognized HTML construct can become a summary boundary.
fn is_marker(literal: &str) -> bool {
    literal
        .trim()
        .strip_prefix("<!--")
        .and_then(|s| s.strip_suffix("-->"))
        .is_some_and(|s| s.trim() == "more")
}
