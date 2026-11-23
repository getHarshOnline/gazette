//! Read-only replacement proposals over exact native source locations.

use gazette_compiler::workspace::{WorkspaceSymbol, WorkspaceSymbolKind};
use gazette_core::{
    publication::Key,
    render::LimitedWriter,
    source::{EntryPath, Location, Precision, Sha256Digest},
};
use std::borrow::Cow;

use super::{Analysis, NavigationError, SourceLocation, WorkspaceNavigation};

/// One same-namespace symbol with a valid complete native TOML reference literal.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ReferenceCandidate<'a> {
    /// Sole compiler declaration owner; not an editor-maintained catalog.
    pub symbol: &'a WorkspaceSymbol,
    /// Native definition bound to its exact selected source and complete preimage.
    pub definition: SourceLocation<'a>,
    /// Complete replacement scalar; alias keys are natively formatted as strings.
    pub replacement: Cow<'a, str>,
}

/// Complete source-bound replacement set, not a rename or permission to publish.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ReferenceCompletion<'a> {
    /// Selected workspace source only.
    pub path: &'a EntryPath,
    /// Exact complete preimage, not normalized TOML.
    pub source_sha256: Sha256Digest,
    /// Verified whole string scalar to replace; never the unquoted decoded key.
    pub location: Location,
    /// Exact bytes required at the replacement range.
    pub preimage: &'a str,
    /// Namespace required by the native field, never inferred from its key prefix.
    pub kind: WorkspaceSymbolKind,
    /// Original target if resolved; candidates may violate other workspace constraints.
    pub current: Option<&'a WorkspaceSymbol>,
    /// All same-namespace declarations in source order, without truncation.
    pub candidates: Vec<ReferenceCandidate<'a>>,
    /// Native decoded identity proof for binding an otherwise unlocated diagnostic.
    #[serde(skip)]
    #[schemars(skip)]
    identity_sha256: Sha256Digest,
    /// An unlocated diagnostic cannot identify one of several identical reference slots.
    #[serde(skip)]
    #[schemars(skip)]
    unique_identity: bool,
}

impl ReferenceCompletion<'_> {
    /// Bind an unlocated native diagnostic only to one unique typed reference identity.
    pub fn matches_unlocated_diagnostic(&self, identity: &Key) -> bool {
        matches!(self.kind, WorkspaceSymbolKind::Declaration(_))
            && self.unique_identity
            && self.identity_sha256 == Sha256Digest::of(identity.as_str().as_bytes())
    }
}

/// Same-document headings from the publication compiler's anchor allocation.
/// The proposal replaces only a verified inline fragment destination, never its
/// label or the containing Markdown construct. It performs no editor/native write.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct HeadingCompletion<'a> {
    /// Exact selected source; no path discovery or cross-document inference.
    pub path: &'a EntryPath,
    /// Complete current source preimage identity, including framing and line endings.
    pub source_sha256: Sha256Digest,
    /// Exact full-source destination range, including the leading fragment marker.
    pub location: Location,
    /// Original destination bytes required before any proposed replacement.
    pub preimage: &'a str,
    /// Complete emitted heading set in compiler order; `fragment` is the replacement.
    pub candidates: &'a [gazette_compiler::markdown::Heading],
}

impl Analysis {
    /// Complete a literal inline same-document fragment using the current shared
    /// heading model. Unresolved fragments can be repaired without borrowing an
    /// earlier successful link analysis. Unsupported source forms return none.
    ///
    /// # Errors
    /// Refuses changed/unknown sources, invalid UTF-8 positions, unavailable current
    /// heading facts and exhausted complete-result bounds, never a partial list.
    pub fn complete_heading<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<HeadingCompletion<'a>>, NavigationError> {
        let (path, digest) = self.navigation_source(path, source, offset)?;
        let Some(headings) = self.heading_symbols(path, source)? else {
            return Ok(None);
        };
        let Some(span) = self.heading_destinations.get(path).and_then(|slots| {
            slots
                .iter()
                .find(|span| span.start <= offset && offset <= span.end)
        }) else {
            return Ok(None);
        };
        let preimage = span.slice(source).ok_or(NavigationError::InvalidSource)?;
        let result = HeadingCompletion {
            path,
            source_sha256: digest,
            location: Location {
                span: *span,
                precision: Precision::Exact,
            },
            preimage,
            candidates: &headings.values,
        };
        let mut bounded = LimitedWriter::new(std::io::sink(), maximum_bytes);
        serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
        if bounded.exceeded() {
            return Err(NavigationError::Limit);
        }
        Ok(Some(result))
    }

    /// Propose replacement of a typed workspace reference, including an unresolved
    /// value. No native read/write or independent parsing occurs. Invalid syntax or
    /// ambiguous declarations never reuse an earlier analysis. Revalidate applied edits.
    ///
    /// # Errors
    /// Refuses stale/unknown source, invalid UTF-8 positions, unavailable native
    /// analysis and a complete JSON projection exceeding `maximum_bytes`.
    pub fn complete_reference<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<ReferenceCompletion<'a>>, NavigationError> {
        if !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        let Some(navigation) = self.workspace_navigation_for(path, source)? else {
            return Ok(None);
        };
        let result = match navigation {
            WorkspaceNavigation::Buffer(navigation) => {
                let Some(reference) = navigation.reference_at(offset) else {
                    return Ok(None);
                };
                let preimage = reference
                    .location
                    .span
                    .slice(source)
                    .ok_or(NavigationError::InvalidSource)?;
                let mut candidates = Vec::new();
                let mut bytes = preimage.len();
                for symbol in reference.candidates() {
                    // Aliases are declared as keys; a valid string value must be
                    // formatted natively rather than inserting that bare key.
                    let replacement_bytes = match symbol {
                        WorkspaceSymbol::Declaration { .. } => {
                            symbol.location().span.end - symbol.location().span.start
                        }
                        WorkspaceSymbol::ResourceInput { .. } => symbol.name().len() + 2,
                    };
                    bytes = bytes
                        .checked_add(replacement_bytes)
                        .and_then(|n| n.checked_add(symbol.name().len()))
                        .filter(|n| *n <= maximum_bytes)
                        .ok_or(NavigationError::Limit)?;
                    let replacement = symbol
                        .replacement(source)
                        .map_err(|_| NavigationError::InvalidSource)?;
                    candidates.push(ReferenceCandidate {
                        symbol,
                        definition: SourceLocation {
                            path: &self.workspace,
                            source_sha256: self.inputs[&self.workspace],
                            location: symbol.location(),
                        },
                        replacement,
                    });
                }
                ReferenceCompletion {
                    path: &self.workspace,
                    source_sha256: self.inputs[&self.workspace],
                    location: reference.location,
                    preimage,
                    kind: reference.kind,
                    current: reference.symbol,
                    candidates,
                    identity_sha256: reference.identity_sha256,
                    unique_identity: reference.unique_identity,
                }
            }
            WorkspaceNavigation::Documents(navigation) => {
                let Some(reference) = navigation.reference_at(path, offset) else {
                    return Ok(None);
                };
                let preimage = reference
                    .location
                    .location
                    .span
                    .slice(source)
                    .ok_or(NavigationError::InvalidSource)?;
                let mut candidates = Vec::new();
                let mut bytes = preimage.len();
                for candidate in reference.candidates() {
                    let replacement = candidate.replacement();
                    let symbol = &candidate.symbol;
                    bytes = bytes
                        .checked_add(replacement.len())
                        .and_then(|n| n.checked_add(symbol.name().len()))
                        .filter(|n| *n <= maximum_bytes)
                        .ok_or(NavigationError::Limit)?;
                    candidates.push(ReferenceCandidate {
                        symbol,
                        definition: SourceLocation {
                            path: &candidate.document,
                            source_sha256: candidate.source_sha256,
                            location: symbol.location(),
                        },
                        replacement: Cow::Borrowed(replacement),
                    });
                }
                let (path, source_sha256) = self
                    .inputs
                    .get_key_value(path)
                    .ok_or(NavigationError::InvalidSource)?;
                ReferenceCompletion {
                    path,
                    source_sha256: *source_sha256,
                    location: reference.location.location,
                    preimage,
                    kind: reference.kind,
                    current: reference.symbol.map(|symbol| &symbol.symbol),
                    candidates,
                    identity_sha256: reference.identity_sha256,
                    unique_identity: reference.unique_identity,
                }
            }
        };
        let mut bounded = LimitedWriter::new(std::io::sink(), maximum_bytes);
        serde_json::to_writer(&mut bounded, &result).map_err(|_| NavigationError::Limit)?;
        if bounded.exceeded() {
            return Err(NavigationError::Limit);
        }
        Ok(Some(result))
    }
}
