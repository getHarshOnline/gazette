//! Inert native scalar replacements over the complete current selected source set.

use super::{Analysis, NavigationError};
use gazette_compiler::authored::{EditorialDeclaration, EditorialField};
use gazette_core::{
    render::LimitedWriter,
    source::{EntryPath, Location, Sha256Digest},
};

/// One kind/site-correct current declaration, not permission to change residence.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct EditorialCandidate<'a> {
    /// Exact declaration and its own source preimage.
    pub declaration: &'a EditorialDeclaration,
    /// Whole original native TOML scalar to use as a replacement.
    pub replacement: &'a str,
}
/// Complete source-bound editorial replacements; apply and reanalyze before relying.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct EditorialReferenceCompletion<'a> {
    /// Exact selected logical source.
    pub path: &'a EntryPath,
    /// Complete current source preimage.
    pub source_sha256: Sha256Digest,
    /// Native whole scalar, never the searched/unquoted decoded identity.
    pub location: Location,
    /// Exact current bytes required before a reviewed edit.
    pub preimage: &'a str,
    /// Native field role makes this distinct from workspace reference completion.
    pub field: EditorialField,
    /// Current kind/site-correct declaration, absent for an unresolved scalar.
    pub current: Option<&'a EditorialDeclaration>,
    /// Complete eligible declarations in logical source order, without truncation.
    pub candidates: Vec<EditorialCandidate<'a>>,
}
impl Analysis {
    /// Propose editorial scalar replacements from native current metadata facts.
    ///
    /// Candidates match the field's native target class and explicit site, and do
    /// not name the owning container itself. Cycles, sole residence and all other
    /// constraints remain the one graph resolver's responsibility after the edit.
    /// Tolerant incomplete frontmatter assistance is independent of these facts.
    ///
    /// # Errors
    /// Refuses stale/unknown source, unavailable or ambiguous current metadata and
    /// exhausted complete-result bounds. Never borrows a prior resolved graph.
    pub fn complete_editorial_reference<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<EditorialReferenceCompletion<'a>>, NavigationError> {
        let (path, digest) = self
            .inputs
            .get_key_value(path)
            .ok_or(NavigationError::InvalidSource)?;
        if *digest != Sha256Digest::of(source.as_bytes()) || !source.is_char_boundary(offset) {
            return Err(NavigationError::InvalidSource);
        }
        let Some(authoring) = self.authored_authoring.get(path) else {
            return Ok(None);
        };
        let Some(authoring) = authoring
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?
        else {
            return Ok(None);
        };
        let Some(field) = authoring
            .editorial_field(source, offset)
            .map_err(|_| NavigationError::Unavailable)?
        else {
            return Ok(None);
        };
        let facts = self
            .editorial_authoring
            .as_ref()
            .ok_or(NavigationError::Unavailable)?
            .as_ref()
            .map_err(|_| NavigationError::Unavailable)?;
        let slot = facts
            .slot_at(path, offset)
            .ok_or(NavigationError::Unavailable)?;
        if slot.source_sha256 != *digest || slot.field != field {
            return Err(NavigationError::InvalidSource);
        }
        let preimage = slot
            .location
            .span
            .slice(source)
            .ok_or(NavigationError::InvalidSource)?;
        let mut candidates = Vec::new();
        let mut bytes = preimage.len();
        for declaration in facts.declarations().iter().filter(|candidate| {
            candidate.site == slot.site
                && field
                    .target_kind()
                    .is_none_or(|kind| candidate.kind == kind)
                && candidate.identity != slot.owner
        }) {
            bytes = bytes
                .checked_add(std::mem::size_of::<EditorialCandidate<'_>>())
                .and_then(|bytes| bytes.checked_add(declaration.replacement().len()))
                .and_then(|bytes| bytes.checked_add(declaration.identity.as_str().len()))
                .and_then(|bytes| bytes.checked_add(declaration.path.as_str().len()))
                .filter(|&bytes| bytes <= maximum_bytes)
                .ok_or(NavigationError::Limit)?;
            candidates.push(EditorialCandidate {
                declaration,
                replacement: declaration.replacement(),
            });
        }
        let current = candidates
            .iter()
            .find(|candidate| candidate.declaration.identity == slot.target)
            .map(|candidate| candidate.declaration);
        let result = EditorialReferenceCompletion {
            path,
            source_sha256: *digest,
            location: slot.location,
            preimage,
            field,
            current,
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
