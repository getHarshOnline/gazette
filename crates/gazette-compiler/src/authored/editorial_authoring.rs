//! Strict typed source facts for editorial repair, never a second resolved graph.

use gazette_core::{
    publication::{Key, content::ContentKind},
    source::{EntryPath, Location, Precision, Sha256Digest},
};
use serde::Serialize;

use super::{Metadata, Origin, ProjectionError, editorial_references::visit};
use crate::{
    corpus::{File, ParsedCorpus},
    document,
    toml::PathSegment,
    workspace::Compilation,
};

/// Native declaration class, independent of an identity's spelling or source path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorialKind {
    /// A leaf, including all explicitly declared page families.
    Page,
    /// A structural collection.
    Collection,
    /// An ordered containing journey.
    Journey,
}
/// Native field role selects the completion namespace without a transport parser.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EditorialField {
    /// A leaf's direct collection residence.
    CollectionResidence,
    /// Another collection's structural parent.
    CollectionParent,
    /// A journey's containing collection.
    JourneyCollection,
    /// One ordered leaf in a journey.
    JourneyMember,
    /// A documentation outline destination, which may itself be a container.
    DocumentationNavigation,
    /// A collection's ordered direct child, without creating a residence.
    CollectionOrder,
}
impl EditorialField {
    /// One target class, or none for orderings that accept pages and containers.
    /// Proposed edits still require whole-graph reanalysis.
    pub fn target_kind(self) -> Option<EditorialKind> {
        match self {
            Self::JourneyMember => Some(EditorialKind::Page),
            Self::DocumentationNavigation | Self::CollectionOrder => None,
            _ => Some(EditorialKind::Collection),
        }
    }
}
/// One current typed declaration and its exact original identity scalar.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct EditorialDeclaration {
    /// Logical selected source, not a native URI or public route.
    pub path: EntryPath,
    /// Complete current source preimage.
    pub source_sha256: Sha256Digest,
    /// Native decoded identity, not a title or prefix heuristic.
    pub identity: Key,
    /// Declared or source-bound home, retained while relationship resolution fails.
    pub site: Key,
    /// Native structural class.
    pub kind: EditorialKind,
    /// Exact full-file identity scalar.
    pub location: Location,
    /// Native TOML literal bytes, including original quoting and escapes.
    replacement: String,
}
impl EditorialDeclaration {
    /// Reuse the original valid native literal without generating another spelling.
    pub fn replacement(&self) -> &str {
        &self.replacement
    }
}
/// One native current scalar slot, including an unresolved but typed key.
#[derive(Debug)]
pub struct EditorialSlot {
    /// Logical selected source.
    pub path: EntryPath,
    /// Complete current source preimage.
    pub source_sha256: Sha256Digest,
    /// Exact native scalar.
    pub location: Location,
    /// Decoded current target, which need not resolve.
    pub target: Key,
    /// Exact owning content declaration.
    pub owner: Key,
    /// Declared or source-bound owning site.
    pub site: Key,
    /// Native relationship role.
    pub field: EditorialField,
}
/// Complete bounded current metadata facts; this is not publication admission.
#[derive(Debug)]
pub struct EditorialAuthoring {
    declarations: Vec<EditorialDeclaration>,
    slots: Vec<EditorialSlot>,
    bytes: usize,
}
impl EditorialAuthoring {
    /// Declarations in deterministic logical source order.
    pub fn declarations(&self) -> &[EditorialDeclaration] {
        &self.declarations
    }
    /// Exact current slot at a native UTF-8 byte offset.
    pub fn slot_at(&self, path: &EntryPath, offset: usize) -> Option<&EditorialSlot> {
        self.slots.iter().find(|slot| {
            &slot.path == path
                && slot.location.span.start <= offset
                && offset < slot.location.span.end
        })
    }
    /// Aggregate declarations plus native reference slots charged to shared budgets.
    pub fn occurrence_count(&self) -> usize {
        self.declarations.len() + self.slots.len()
    }
    /// Complete owned fact bytes, including original literals and logical identities.
    pub fn fact_bytes(&self) -> usize {
        self.bytes
    }
}

/// Retain native declaration/slot facts independently of relationship resolution.
///
/// Unknown references may be repaired from current typed declarations. Missing or
/// malformed typed metadata never becomes a successful candidate subset. Existing
/// tolerant frontmatter/schema assistance remains separate and is not restricted.
///
/// # Errors
/// Refuses ambiguous identities, typed metadata/source invariants and complete
/// projection ceilings. Legal reduced sources are excluded by their native role.
pub fn editorial_authoring(
    compilation: &Compilation,
    corpus: &ParsedCorpus<'_>,
    occurrence_limit: usize,
    byte_limit: usize,
) -> Result<EditorialAuthoring, ProjectionError> {
    let mut facts = EditorialAuthoring {
        declarations: Vec::new(),
        slots: Vec::new(),
        bytes: 0,
    };
    let mut identities = std::collections::BTreeSet::new();
    let identity_path = [key("content"), key("id")];
    for (path, file) in corpus.files() {
        if compilation.workspace.legal_bindings(path).next().is_some() {
            continue;
        }
        let File::Document(document) = file else {
            continue;
        };
        let metadata = document
            .metadata()
            .deserialize::<Metadata<Option<Key>>>()
            .map_err(|error| ProjectionError::Document {
                origin: Origin {
                    path: path.clone(),
                    source_sha256: file.source_sha256(),
                    span: None,
                },
                error: Box::new(document::Error::Metadata {
                    error,
                    offset: document.metadata_span().start,
                }),
            })?;
        let id = &metadata.content.id;
        let binding = |reason| ProjectionError::Binding {
            id: id.clone(),
            reason,
        };
        let site = metadata
            .home_site(compilation, path)
            .ok_or_else(|| binding("omitted content site requires a configured source binding"))?;
        if !identities.insert(id.clone()) {
            return Err(binding(
                "editorial declarations have ambiguous current identities",
            ));
        }
        let scalar = |field: &[PathSegment], target: &Key| -> Result<Location, ProjectionError> {
            let (span, decoded) = document
                .metadata()
                .string_at(field)
                .ok_or_else(|| binding("editorial fact has no exact native scalar"))?;
            if decoded != target.as_str() {
                return Err(binding("editorial fact differs from its typed scalar"));
            }
            Ok(Location {
                span: span
                    .in_parent(document.source(), document.metadata_span())
                    .ok_or_else(|| binding("editorial fact is outside retained source"))?,
                precision: Precision::Exact,
            })
        };
        let location = scalar(&identity_path, id)?;
        let replacement = location
            .span
            .slice(document.source())
            .ok_or_else(|| binding("editorial identity has no original literal"))?;
        let kind = match metadata.content.structure {
            ContentKind::Page { .. } => EditorialKind::Page,
            ContentKind::Collection { .. } => EditorialKind::Collection,
            ContentKind::Journey { .. } => EditorialKind::Journey,
        };
        charge(
            &mut facts,
            1,
            std::mem::size_of::<EditorialDeclaration>()
                + path.as_str().len()
                + id.as_str().len()
                + site.as_str().len()
                + replacement.len(),
            occurrence_limit,
            byte_limit,
        )?;
        facts.declarations.push(EditorialDeclaration {
            path: path.clone(),
            source_sha256: file.source_sha256(),
            identity: id.clone(),
            site: site.clone(),
            kind,
            location,
            replacement: replacement.to_owned(),
        });
        visit(&metadata, |target, field, purpose| {
            let location = scalar(field, target)?;
            charge(
                &mut facts,
                1,
                std::mem::size_of::<EditorialSlot>()
                    + path.as_str().len()
                    + id.as_str().len()
                    + site.as_str().len()
                    + target.as_str().len(),
                occurrence_limit,
                byte_limit,
            )?;
            facts.slots.push(EditorialSlot {
                path: path.clone(),
                source_sha256: file.source_sha256(),
                location,
                target: target.clone(),
                owner: id.clone(),
                site: site.clone(),
                field: purpose,
            });
            Ok(())
        })?;
    }
    Ok(facts)
}
fn charge(
    facts: &mut EditorialAuthoring,
    count: usize,
    bytes: usize,
    occurrence_limit: usize,
    byte_limit: usize,
) -> Result<(), ProjectionError> {
    if facts
        .occurrence_count()
        .checked_add(count)
        .is_none_or(|count| count > occurrence_limit)
    {
        return Err(ProjectionError::Limit("editorial authoring occurrences"));
    }
    facts.bytes = facts
        .bytes
        .checked_add(bytes)
        .filter(|&bytes| bytes <= byte_limit)
        .ok_or(ProjectionError::Limit("editorial authoring bytes"))?;
    Ok(())
}
fn key(value: &str) -> PathSegment {
    PathSegment::Key(value.to_owned())
}
