//! Source-bound authored references over retained native declaration facts.

use super::{Analysis, Error, Limits, NavigationError, WorkspaceNavigation, charge};
use gazette_compiler::authored::AuthoredReferenceField;
use gazette_core::{
    publication::Key,
    render::LimitedWriter,
    source::{EntryPath, Location, Sha256Digest},
};

/// One current declaration and the complete native literal to propose.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct AuthoredReferenceCandidate<'a> {
    /// Typed identity supplied by the native declaration, never a name lookup.
    pub identity: &'a Key,
    /// Exact selected source defining this candidate.
    pub path: &'a EntryPath,
    /// Complete declaration preimage, not normalized TOML.
    pub source_sha256: Sha256Digest,
    /// Exact scalar, or native construct for a derived content identity.
    pub location: Location,
    /// Complete valid native replacement scalar.
    pub replacement: &'a str,
}
/// Complete authored-reference proposal; apply only against its exact preimage and reanalyze.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct AuthoredReferenceCompletion<'a> {
    /// Exact selected queried source.
    pub path: &'a EntryPath,
    /// Complete current queried preimage.
    pub source_sha256: Sha256Digest,
    /// Native full-file whole scalar range.
    pub location: Location,
    /// Exact original scalar bytes required before any edit.
    pub preimage: &'a str,
    /// Namespace selected by the native authored field.
    pub field: AuthoredReferenceField,
    /// Current resolved declaration identity, absent for a typo.
    pub current: Option<&'a Key>,
    /// Complete current namespace, never an arbitrary prefix or prior model.
    pub candidates: Vec<AuthoredReferenceCandidate<'a>>,
}
impl Analysis {
    /// Complete authored site, attribution principal and non-containing related references.
    /// Unrelated graph failures do not erase sound ordinary declaration evidence.
    /// A failed graph containing selected derived legal outputs cannot supply an
    /// incomplete related namespace; it remains unavailable, never guessed.
    ///
    /// # Errors
    /// Refuses changed sources, unavailable current native facts and complete-result bounds.
    pub fn complete_authored_reference<'a>(
        &'a self,
        path: &EntryPath,
        source: &'a str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<Option<AuthoredReferenceCompletion<'a>>, NavigationError> {
        // These inert repairs use current declaration evidence, not a successful
        // corpus graph. An unrelated syntax failure must not erase a sound
        // recovered scalar; each target namespace below owns its availability.
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
        let Some(slot) = authoring
            .reference_slot(source, offset)
            .map_err(|_| NavigationError::Unavailable)?
        else {
            return Ok(None);
        };
        let preimage = slot
            .location
            .span
            .slice(source)
            .ok_or(NavigationError::InvalidSource)?;
        let mut candidates = Vec::new();
        let mut bytes = preimage.len();
        let mut add = |candidate: AuthoredReferenceCandidate<'a>| {
            bytes = bytes
                .checked_add(std::mem::size_of::<AuthoredReferenceCandidate<'_>>())
                .and_then(|n| n.checked_add(candidate.identity.as_str().len()))
                .and_then(|n| n.checked_add(candidate.path.as_str().len()))
                .and_then(|n| n.checked_add(candidate.replacement.len()))
                .filter(|&n| n <= maximum_bytes)
                .ok_or(NavigationError::Limit)?;
            candidates.push(candidate);
            Ok::<_, NavigationError>(())
        };
        if matches!(
            slot.field,
            AuthoredReferenceField::RelatedContent | AuthoredReferenceField::BrowsingIndex
        ) {
            if let Some(targets) = &self.related_reference_targets {
                for target in targets {
                    if slot.field == AuthoredReferenceField::BrowsingIndex
                        && !target.is_browsing_index()
                    {
                        continue;
                    }
                    add(candidate(target))?;
                }
            } else {
                if slot.field == AuthoredReferenceField::BrowsingIndex {
                    // Recovered generic page facts do not prove index family/routing.
                    return Err(NavigationError::Unavailable);
                }
                let workspace = self
                    .workspace_compilation
                    .as_ref()
                    .ok_or(NavigationError::Unavailable)?;
                if workspace
                    .workspace
                    .legal_sources()
                    .any(|binding| self.inputs.keys().any(|path| binding.entry(path).is_some()))
                {
                    return Err(NavigationError::Unavailable);
                }
                let facts = self
                    .editorial_authoring
                    .as_ref()
                    .ok_or(NavigationError::Unavailable)?
                    .as_ref()
                    .map_err(|_| NavigationError::Unavailable)?;
                for declaration in facts.declarations() {
                    add(AuthoredReferenceCandidate {
                        identity: &declaration.identity,
                        path: &declaration.path,
                        source_sha256: declaration.source_sha256,
                        location: declaration.location,
                        replacement: declaration.replacement(),
                    })?;
                }
            }
        } else {
            for target in self
                .workspace_reference_targets
                .as_ref()
                .ok_or(NavigationError::Unavailable)?
                .iter()
                .filter(|target| slot.field.accepts_declaration(target.field))
            {
                add(candidate(target))?;
            }
        }
        let current = candidates
            .iter()
            .find(|candidate| *candidate.identity == slot.target)
            .map(|candidate| candidate.identity);
        let result = AuthoredReferenceCompletion {
            path,
            source_sha256: *digest,
            location: slot.location,
            preimage,
            field: slot.field,
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
pub(super) fn retain_workspace_targets(
    analysis: &Analysis,
    sources: &std::collections::BTreeMap<EntryPath, &[u8]>,
    limits: Limits,
    occurrences: &mut usize,
    bytes: &mut usize,
) -> Result<Option<Vec<gazette_compiler::authored::AuthoredReferenceDeclaration>>, Error> {
    let Some(navigation) = analysis.workspace_navigation.as_ref() else {
        return Ok(None);
    };
    let mut result = Vec::new();
    let mut add = |path: &EntryPath,
                   source: &str,
                   symbol: &gazette_compiler::workspace::WorkspaceSymbol| {
        if let Some(target) = gazette_compiler::authored::AuthoredReferenceDeclaration::workspace(
            symbol, path, source,
        )
        .map_err(|_| Error::InvalidSource)?
        {
            charge(
                occurrences,
                1,
                limits.navigation_occurrences,
                "navigation occurrences",
            )?;
            charge(
                bytes,
                target.fact_bytes(),
                limits.navigation_bytes,
                "navigation bytes",
            )?;
            result.push(target);
        }
        Ok::<_, Error>(())
    };
    for (path, bytes) in sources {
        let source = std::str::from_utf8(bytes).map_err(|_| Error::InvalidSource)?;
        match navigation {
            WorkspaceNavigation::Buffer(navigation) => {
                for symbol in navigation.symbols() {
                    add(path, source, symbol)?;
                }
            }
            WorkspaceNavigation::Documents(navigation) => {
                for symbol in navigation.symbols(path) {
                    add(path, source, symbol)?;
                }
            }
        }
    }
    Ok(Some(result))
}
fn candidate(
    declaration: &gazette_compiler::authored::AuthoredReferenceDeclaration,
) -> AuthoredReferenceCandidate<'_> {
    AuthoredReferenceCandidate {
        identity: &declaration.identity,
        path: &declaration.path,
        source_sha256: declaration.source_sha256,
        location: declaration.location,
        replacement: declaration.replacement(),
    }
}
