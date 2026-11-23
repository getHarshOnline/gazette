//! Source heading associations share publication fragment decoding and anchor identity.

use std::{borrow::Cow, collections::BTreeMap};

use gazette_core::source::{Location, Span};
use percent_encoding::percent_decode_str;

use super::{AnchorPolicy, Budget, Error, LinkForm, LinkKind, ParsedMarkdown};

/// A native link construct associated with one emitted heading, not a text edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeadingReference {
    /// Body-relative containing occurrence, retaining native source precision.
    pub location: Location,
    /// Index into this document's headings under the same anchor policy as the query.
    pub heading: usize,
}

impl ParsedMarkdown<'_> {
    pub(crate) fn source_links(&self) -> &[gazette_core::markdown::Link] {
        &self.document.syntax().links
    }

    /// Associate native path-empty fragment links with the existing heading index.
    ///
    /// Includes recognized non-emitted source links. Images and destinations with
    /// any path are outside this document-local association. Empty fragments and
    /// valid non-heading anchors have no heading target. No reparsing, rendering,
    /// source-token scanning or public-URL reconstruction occurs.
    ///
    /// # Errors
    /// Refuses invalid anchors, malformed/unknown fragments and exhausted projection
    /// bytes without a partial result. Borrowed lookup entries and returned records
    /// are charged before insertion; fragment rules match publication.
    pub fn heading_references(&self) -> Result<Vec<HeadingReference>, Error> {
        self.heading_references_with_policy(AnchorPolicy::Publication)
    }

    /// Associate local links with headings allocated by the selected source owner.
    /// Valid named HTML anchors in manuals have no heading association.
    ///
    /// # Errors
    /// Refuses invalid declarations or fragments and exhausted projection bounds.
    pub fn heading_references_with_policy(
        &self,
        policy: AnchorPolicy,
    ) -> Result<Vec<HeadingReference>, Error> {
        let headings = self.headings_with_policy(policy)?;
        let mut budget = Budget(self.limits.output_bytes, "heading reference bytes");
        let mut indices = BTreeMap::new();
        for (index, heading) in headings.iter().enumerate() {
            budget.charge(std::mem::size_of::<(&str, usize)>())?;
            indices.insert(heading.id.as_str(), index);
        }
        let mut references = Vec::new();
        for link in &self.document.syntax().links {
            if link.form != LinkForm::Markdown
                || link.kind != LinkKind::Link
                || link
                    .destination
                    .find(['?', '#'])
                    .unwrap_or(link.destination.len())
                    != 0
            {
                continue;
            }
            let Some(anchor) =
                self.fragment_anchor_with_policy(&link.destination, link.location.span, policy)?
            else {
                continue;
            };
            let Some(&heading) = indices.get(anchor.as_ref()) else {
                continue;
            };
            budget.charge(std::mem::size_of::<HeadingReference>())?;
            references.push(HeadingReference {
                location: link.location,
                heading,
            });
        }
        Ok(references)
    }

    /// Resolve one suffix against the same emitted heading/footnote/continuation IDs.
    /// This internal operation also serves configured cross-document publication.
    pub(crate) fn fragment_anchor<'a>(
        &self,
        suffix: &'a str,
        span: Span,
    ) -> Result<Option<Cow<'a, str>>, Error> {
        self.fragment_anchor_with_policy(suffix, span, AnchorPolicy::Publication)
    }

    pub(crate) fn fragment_anchor_with_policy<'a>(
        &self,
        suffix: &'a str,
        span: Span,
        policy: AnchorPolicy,
    ) -> Result<Option<Cow<'a, str>>, Error> {
        if policy == AnchorPolicy::Manual
            && suffix
                .split_once('#')
                .is_some_and(|(_, fragment)| fragment.is_empty())
        {
            return Err(Error::LinkResolution {
                span,
                reason: "manual link has an empty fragment",
            });
        }
        let Some((_, fragment)) = suffix.split_once('#').filter(|(_, part)| !part.is_empty())
        else {
            return Ok(None);
        };
        let failure = |reason| Error::LinkResolution { span, reason };
        let anchor = decode_reference(fragment, span)?;
        if !self
            .anchor_index_with_policy(policy)
            .map_err(|_| failure("target document anchor index is invalid"))?
            .contains(&anchor)
        {
            return Err(failure(
                "fragment is absent from the target document anchor index",
            ));
        }
        Ok(Some(anchor))
    }
}

// URL parsers permissively retain malformed percent sequences. Native source
// and anchor coordinates require valid escapes and exactly one UTF-8 decode.
pub(crate) fn decode_reference(value: &str, span: Span) -> Result<Cow<'_, str>, Error> {
    let failure = |reason| Error::LinkResolution { span, reason };
    for (offset, byte) in value.bytes().enumerate() {
        if byte == b'%'
            && !value
                .as_bytes()
                .get(offset + 1..offset + 3)
                .is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(failure("malformed percent escape in document reference"));
        }
    }
    let decoded = percent_decode_str(value)
        .decode_utf8()
        .map_err(|_| failure("document reference is not UTF-8"))?;
    if decoded.chars().any(char::is_control) {
        return Err(failure("control character in document reference"));
    }
    Ok(decoded)
}
