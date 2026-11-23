//! Keep a single prior workspace parse for editing one known reference scalar.

use super::*;
use crate::source::{Location, Precision, Sha256Digest, Span};

pub(super) struct Basis {
    analysis: authoring::Analysis,
    id: String,
    pub(super) source: String,
    generation: u64,
}

/// A declaration from the last parsed workspace, without obsolete source positions.
#[derive(Debug, serde::Serialize)]
pub struct PreviousReferenceCandidate {
    /// Name shown by an editor.
    pub label: String,
    /// Complete TOML literal supplied by the compiler.
    pub replacement: String,
}

/// A previous candidate set mapped to one exact current reference edit.
#[derive(Debug, serde::Serialize)]
pub struct PreviousReferenceCompletion<'a> {
    /// Current selected source.
    pub path: &'a EntryPath,
    /// Digest of the complete current buffer.
    pub source_sha256: Sha256Digest,
    /// Current scalar range, including any incomplete text.
    pub location: Location,
    /// Current bytes to replace.
    pub preimage: &'a str,
    /// Source digest from which the candidate declarations were read.
    pub basis_sha256: Sha256Digest,
    /// Session generation that supplied those declarations.
    pub basis_generation: u64,
    /// Complete same-namespace candidate set.
    pub candidates: Vec<PreviousReferenceCandidate>,
}

impl Session {
    pub(super) fn retain_completion_basis(
        &mut self,
        id: &str,
        source: Option<String>,
        analysis: Result<authoring::Analysis, authoring::Error>,
        previous: Option<Basis>,
        generation: u64,
    ) {
        let document = &self.files[id];
        if self.analysis.as_ref().is_ok_and(|analysis| {
            analysis
                .workspace_symbols(document.analysis_path(), document.text())
                .is_ok_and(|symbols| symbols.is_some())
        }) {
            return;
        }
        self.completion_basis = match (source, analysis) {
            (Some(source), Ok(analysis)) => Some(Basis {
                analysis,
                source,
                id: id.into(),
                generation,
            }),
            _ => previous.filter(|basis| basis.id == id),
        }
        .filter(|basis| {
            self.retained_bytes.saturating_add(basis.source.len()) <= self.limits.source_bytes
        });
    }

    /// Complete an interrupted workspace reference using its last parsed scalar.
    /// All text outside that scalar must be unchanged. Diagnostics, navigation and
    /// publication checks continue to use the current analysis alone.
    ///
    /// # Errors
    /// Refuses invalid positions, unavailable compiler facts and oversized results.
    pub fn complete_previous_reference(
        &self,
        id: &str,
        offset: usize,
        maximum: usize,
    ) -> Result<Option<PreviousReferenceCompletion<'_>>, authoring::NavigationError> {
        use authoring::NavigationError;
        let Some(basis) = self
            .completion_basis
            .as_ref()
            .filter(|basis| basis.id == id)
        else {
            return Ok(None);
        };
        let document = &self.files[id];
        let current = document.text();
        if !current.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        let old = basis.source.as_str();
        let mut prefix = old
            .bytes()
            .zip(current.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(prefix) || !current.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let mut suffix = old[prefix..]
            .bytes()
            .rev()
            .zip(current[prefix..].bytes().rev())
            .take_while(|(a, b)| a == b)
            .count();
        while !old.is_char_boundary(old.len() - suffix)
            || !current.is_char_boundary(current.len() - suffix)
        {
            suffix -= 1;
        }
        let old_end = old.len() - suffix;
        let new_end = current.len() - suffix;
        let previous_offset = if offset <= prefix {
            offset
        } else if offset > new_end {
            old_end + (offset - new_end)
        } else {
            prefix
        };
        let Some(proposal) = basis.analysis.complete_reference(
            document.analysis_path(),
            old,
            previous_offset,
            maximum,
        )?
        else {
            return Ok(None);
        };
        let span = proposal.location.span;
        // A typed reference may change; its field, declarations and surrounding
        // configuration may not. Never map a changed declaration to a stale list.
        if prefix < span.start || old_end > span.end {
            return Ok(None);
        }
        let end = new_end + (span.end - old_end);
        if offset < span.start || offset > end {
            return Ok(None);
        }
        let span = Span::checked(current, span.start..end).ok_or(NavigationError::InvalidSource)?;
        let result = PreviousReferenceCompletion {
            path: document.analysis_path(),
            source_sha256: Sha256Digest::of(current.as_bytes()),
            location: Location {
                span,
                precision: Precision::Exact,
            },
            preimage: span.slice(current).ok_or(NavigationError::InvalidSource)?,
            basis_sha256: Sha256Digest::of(old.as_bytes()),
            basis_generation: basis.generation,
            candidates: proposal
                .candidates
                .into_iter()
                .map(|candidate| PreviousReferenceCandidate {
                    label: candidate.symbol.name().into(),
                    replacement: candidate.replacement.into_owned(),
                })
                .collect(),
        };
        let mut bounded = crate::render::LimitedWriter::new(std::io::sink(), maximum);
        serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
        if bounded.exceeded() {
            return Err(NavigationError::Limit);
        }
        Ok(Some(result))
    }
}
