//! Inert destination-only edits from the current compiler-owned publication model.

use super::{Analysis, NavigationError};
use gazette_core::{
    render::LimitedWriter,
    source::{EntryPath, Location, Precision, Sha256Digest},
};

/// Compiler-owned selected page/fragment identity and exact native definition.
pub use gazette_compiler::authored::SymbolicTarget as SymbolicCandidate;

/// Complete source-bound selector repair; apply a reviewed edit and reanalyze.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct SymbolicReferenceCompletion<'a> {
    /// Current selected source, not an inferred URI.
    pub path: &'a EntryPath,
    /// Complete current preimage including framing and line endings.
    pub source_sha256: Sha256Digest,
    /// Exact destination bytes only; the visible label remains untouched.
    pub location: Location,
    /// Original bytes required at that range before applying a replacement.
    pub preimage: &'a str,
    /// Exact known primary candidate, absent for an incomplete/unresolved selector.
    pub current: Option<&'a SymbolicCandidate>,
    /// Complete current selected page/heading/notes set, without truncation.
    pub candidates: &'a [SymbolicCandidate],
}

impl Analysis {
    /// Complete an AST-recognized `id:` or `site:` wikilink from the selected model.
    /// Publication errors on the queried selector remain independently repairable;
    /// graph/anchor failure never borrows targets from an earlier successful overlay.
    ///
    /// # Errors
    /// Refuses stale/unknown sources, invalid UTF-8 offsets, unavailable current
    /// facts and complete JSON output exhaustion. This method performs no writes.
    pub fn complete_symbolic_reference<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<SymbolicReferenceCompletion<'a>>, NavigationError> {
        let (path, digest) = self.navigation_source(path, source, offset)?;
        let Some(span) = self.symbolic_destinations.get(path).and_then(|slots| {
            slots
                .iter()
                .find(|span| span.start <= offset && offset <= span.end)
        }) else {
            return Ok(None);
        };
        let candidates = self
            .symbolic_targets
            .as_deref()
            .ok_or(NavigationError::Unavailable)?;
        let preimage = span.slice(source).ok_or(NavigationError::InvalidSource)?;
        let result = SymbolicReferenceCompletion {
            path,
            source_sha256: digest,
            location: Location {
                span: *span,
                precision: Precision::Exact,
            },
            preimage,
            current: candidates
                .iter()
                .find(|candidate| candidate.replacement == preimage),
            candidates,
        };
        let mut bounded = LimitedWriter::new(std::io::sink(), maximum_bytes);
        serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
        if bounded.exceeded() {
            return Err(NavigationError::Limit);
        }
        Ok(Some(result))
    }
}
