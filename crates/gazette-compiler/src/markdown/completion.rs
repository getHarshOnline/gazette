//! Exact edit locations recovered from parser-recognized inline fragment links.

use super::{Budget, Error, LinkForm, LinkKind, ParsedMarkdown};
use gazette_core::source::Span;

impl ParsedMarkdown<'_> {
    /// Locate a verbatim `id:` or `site:` destination in a recognized wikilink.
    ///
    /// Incomplete or unresolved selectors remain repairable. The native AST owns
    /// link recognition; only its exact original destination bytes are editable.
    /// Labels, code, images and escaped/entity-normalized destinations are never
    /// replacement ranges. No selector is resolved or admitted by this operation.
    ///
    /// # Errors
    /// Refuses inconsistent source coordinates and complete-result exhaustion.
    pub fn symbolic_completion_slots(&self) -> Result<Vec<Span>, Error> {
        let mut budget = Budget(self.limits.output_bytes, "symbolic completion slots");
        let mut slots = Vec::new();
        for link in &self.document.syntax().links {
            if link.form != LinkForm::WikiLink
                || link.kind != LinkKind::Link
                || !link.emitted
                || !(link.destination.starts_with("site:") || link.destination.starts_with("id:"))
            {
                continue;
            }
            let invalid = || Error::Adapter(gazette_core::markdown::Error::InvalidSource);
            let source = link.location.span.slice(&self.source).ok_or_else(invalid)?;
            let Some(inner) = source
                .strip_prefix("[[")
                .and_then(|value| value.strip_suffix("]]"))
            else {
                continue;
            };
            let Some(rest) = inner.strip_prefix(&link.destination) else {
                continue;
            };
            if !rest.is_empty() && !rest.starts_with('|') {
                continue;
            }
            let start = link.location.span.start + 2;
            let span = Span::checked(&self.source, start..start + link.destination.len())
                .ok_or_else(invalid)?;
            budget.charge(std::mem::size_of::<Span>())?;
            slots.push(span);
        }
        slots.sort_unstable();
        slots.dedup();
        Ok(slots)
    }

    /// Locate literal inline heading destinations, including an unresolved or empty
    /// fragment. This does not recognize Markdown independently: each candidate
    /// must be an emitted link from the retained syntax tree, with a verbatim
    /// `](#fragment)` suffix matching its decoded destination exactly.
    ///
    /// Titles, reference-style links, images, escaped/entity destinations and other
    /// source forms have no edit location in this profile. A containing construct
    /// is never returned as a replacement range. No target is invented or admitted.
    ///
    /// # Errors
    /// Refuses inconsistent native coordinates or exhausted complete-result bounds.
    pub fn heading_completion_slots(&self) -> Result<Vec<Span>, Error> {
        let mut budget = Budget(self.limits.output_bytes, "heading completion slots");
        let mut slots = Vec::new();
        for link in &self.document.syntax().links {
            if link.form != LinkForm::Markdown
                || link.kind != LinkKind::Link
                || !link.emitted
                || !link.title.is_empty()
                || !link.destination.starts_with('#')
                || link.destination.contains('\\')
            {
                continue;
            }
            let invalid = || Error::Adapter(gazette_core::markdown::Error::InvalidSource);
            let source = link.location.span.slice(&self.source).ok_or_else(invalid)?;
            let Some(prefix) = source.strip_suffix(')') else {
                continue;
            };
            let Some(label) = prefix.strip_suffix(&link.destination) else {
                continue;
            };
            if !label.ends_with("](") {
                continue;
            }
            let start = link.location.span.start + label.len();
            let span = Span::checked(&self.source, start..start + link.destination.len())
                .ok_or_else(invalid)?;
            budget.charge(std::mem::size_of::<Span>())?;
            slots.push(span);
        }
        slots.sort_unstable();
        slots.dedup();
        Ok(slots)
    }
}
