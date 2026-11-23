//! Whole-source inspection is distinct from normalized publication output.

use super::{Budget, Error, Heading, LinkForm, LinkKind, ParsedMarkdown, Span};
use gazette_core::markdown::{Footnote, FootnoteReference};
use gazette_core::source::{Location, Precision, Sha256Digest};
use serde::Serialize;

/// Source-inspection view of a reference, not a semantic publication relation.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Link {
    /// Native syntactic language; a URI spelling alone never grants symbolic semantics.
    pub form: LinkForm,
    /// Navigational and embedded references remain distinct.
    pub kind: LinkKind,
    /// Parser-decoded URI; not a safety or admission claim.
    pub destination: String,
    /// Decoded title, empty when absent.
    pub title: String,
    /// Containing source construct in body-relative coordinates.
    pub span: Span,
    /// A containing range must not be promoted into an exact source edit.
    pub precision: Precision,
    /// False for occurrences absent from normalized destination-bearing markup.
    pub emitted: bool,
}

/// Bounded source evidence, not certified search or publication output.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact original input digest.
    pub source_sha256: Sha256Digest,
    /// Inspected syntax nodes, including the document root.
    #[schemars(range(max = usize::MAX))]
    pub node_count: usize,
    /// Whole-source text/code/alt; only the recognized excerpt marker is omitted.
    pub text: String,
    /// Same emitted headings and anchors consumed by HTML and navigation.
    pub headings: Vec<Heading>,
    /// Whole-source declarations, including headings absent from normalized output.
    /// These are evidence, not a second navigation or anchor allocation.
    pub source_headings: Vec<gazette_core::markdown::Heading>,
    /// Whole-source explicit occurrences, including non-emitted references.
    pub links: Vec<Link>,
    /// Whole-source located definitions, including unused source evidence.
    pub footnotes: Vec<Footnote>,
    /// Located references joined by normalized label, with honest source precision.
    pub footnote_references: Vec<FootnoteReference>,
    /// Untrusted HTML ranges other than the admitted excerpt marker.
    pub raw_html: Vec<Location>,
    /// Explicit excerpt boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary_marker: Option<Location>,
}

impl ParsedMarkdown<'_> {
    /// Borrow native definition occurrences without rendering or re-parsing.
    /// Labels are parser-normalized document-local associations, not DOM IDs.
    /// Locations remain body-relative constructs unless independently exact.
    pub fn footnote_definitions(&self) -> &[Footnote] {
        &self.document.syntax().footnotes
    }

    /// Borrow all native reference occurrences, including non-emitted source evidence.
    /// Undefined text recognized as ordinary Markdown is not invented as a reference.
    pub fn footnote_references(&self) -> &[FootnoteReference] {
        &self.document.syntax().footnote_references
    }

    /// Inspect source text, including unused definitions; never use as published search text.
    ///
    /// # Errors
    /// Fails on invalid summary admission or output exhaustion.
    pub fn text(&self) -> Result<String, Error> {
        let cut = self.summary_cut()?;
        let mut budget = Budget(self.limits.output_bytes, "text bytes");
        let mut text = String::new();
        for piece in &self.document.syntax().text {
            if cut.is_some_and(|c| c.marker.location == piece.location) {
                continue;
            }
            budget.push(&mut text, &piece.value)?;
        }
        Ok(text)
    }

    /// Project bounded syntax evidence from the retained exact-source analysis.
    ///
    /// # Errors
    /// Rejects invalid anchors, excerpt boundaries or aggregate string exhaustion.
    pub fn inspect(&self) -> Result<Inspection, Error> {
        let cut = self.summary_cut()?;
        let mut budget = Budget(self.limits.output_bytes, "inspection string bytes");
        let text = self.text()?;
        budget.charge(text.len())?;
        let mut headings = Vec::new();
        for heading in self.headings()? {
            budget.charge(heading.text.len() + heading.id.len() + heading.fragment.len())?;
            headings.push(heading.clone());
        }
        let mut links = Vec::new();
        let mut source_headings = Vec::new();
        for heading in &self.document.syntax().headings {
            budget.charge(heading.text.len())?;
            if let Some(id) = &heading.id {
                budget.charge(id.len())?;
            }
            for class in &heading.classes {
                budget.charge(class.len())?;
            }
            for (key, value) in &heading.attributes {
                budget.charge(key.len() + value.len())?;
            }
            source_headings.push(heading.clone());
        }
        for link in &self.document.syntax().links {
            links.push(Link {
                form: link.form,
                kind: link.kind,
                destination: budget.copy(&link.destination)?,
                title: budget.copy(&link.title)?,
                span: link.location.span,
                precision: link.location.precision,
                emitted: link.emitted,
            });
        }
        let mut footnotes = Vec::new();
        for note in &self.document.syntax().footnotes {
            budget.charge(note.name.len())?;
            footnotes.push(note.clone());
        }
        let mut footnote_references = Vec::new();
        for reference in &self.document.syntax().footnote_references {
            budget.charge(
                reference.name.len() + reference.definition.as_ref().map_or(0, String::len),
            )?;
            footnote_references.push(reference.clone());
        }
        Ok(Inspection {
            source_sha256: self.source_sha256(),
            node_count: self.node_count(),
            text,
            headings,
            source_headings,
            links,
            footnotes,
            footnote_references,
            raw_html: self
                .document
                .syntax()
                .html
                .iter()
                .filter(|h| !cut.is_some_and(|c| c.marker == **h))
                .map(|h| h.location)
                .collect(),
            summary_marker: cut.map(|c| c.marker.location),
        })
    }
}
