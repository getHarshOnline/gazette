//! Exact editorial source associations over the existing admitted metadata model.

use gazette_core::{
    publication::{Key, content::ContentKind},
    source::{EntryPath, Location, Precision},
};

use super::{EditorialField, Metadata, ProjectionError, ResolvedAuthored};
use crate::toml::PathSegment;

/// One typed editorial reference bound to two exact retained source files.
/// Native TOML scalar coordinates include their original delimiters and escapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorialReference<'a> {
    /// Exact source containing the declared relationship.
    pub path: &'a EntryPath,
    /// Full-file native scalar occurrence, not a searched decoded identity.
    pub location: Location,
    /// Exact selected source supplying the declared target.
    pub target: &'a EntryPath,
    /// Explicit identity scalar, or a native derived target's source construct.
    pub definition: Location,
}

impl ResolvedAuthored<'_, '_> {
    /// Project collection residence, journey containment and documentation destinations.
    ///
    /// The existing metadata type and resolved graph own meaning and target kinds.
    /// The retained TOML tree owns source ranges; no reparsing, key-prefix lookup,
    /// route guessing or independently deserialized relationship schema occurs.
    ///
    /// # Errors
    /// Refuses invalid retained source/graph bindings or exhausted complete-result
    /// bounds. Derived identities never fabricate an authored declaration scalar.
    pub fn editorial_references(
        &self,
        occurrence_limit: usize,
        byte_limit: usize,
    ) -> Result<Vec<EditorialReference<'_>>, ProjectionError> {
        let mut references = Vec::new();
        let mut bytes = 0usize;
        let identity = [key("content"), key("id")];
        for node in self.content.graph().nodes() {
            // Reduced legal/native sources have no authored relationship declaration.
            if self.origin(&node.id, &identity).is_none() {
                continue;
            }
            let binding = |reason| ProjectionError::Binding {
                id: node.id.clone(),
                reason,
            };
            let path = self
                .source_path(&node.id)
                .ok_or_else(|| binding("editorial source has no retained source association"))?;
            let input = self
                .file(&node.id)
                .and_then(super::input::Input::from_file)
                .ok_or_else(|| binding("editorial source has no retained authored metadata"))?;
            let metadata = input.read().map_err(|_| {
                binding("resolved editorial metadata no longer matches its admitted type")
            })?;
            if metadata.content.id != node.id {
                return Err(binding(
                    "editorial metadata identity differs from its graph owner",
                ));
            }
            let mut add = |target: &Key, field: &[PathSegment], purpose: EditorialField| {
                let target_node =
                    self.content.graph().node(target).ok_or_else(|| {
                        binding("editorial target is absent from the resolved graph")
                    })?;
                let correct_kind = match purpose.target_kind() {
                    Some(super::EditorialKind::Collection) => {
                        matches!(target_node.structure, ContentKind::Collection { .. })
                    }
                    Some(super::EditorialKind::Page) => {
                        matches!(target_node.structure, ContentKind::Page { .. })
                    }
                    Some(super::EditorialKind::Journey) => {
                        matches!(target_node.structure, ContentKind::Journey { .. })
                    }
                    None => true,
                };
                if !correct_kind {
                    return Err(binding(
                        "editorial target kind differs from its native field",
                    ));
                }
                let (location, decoded) = input.string(field).ok_or_else(|| {
                    binding("editorial reference has no exact native string occurrence")
                })?;
                if decoded != target.as_str() {
                    return Err(binding("editorial scalar differs from its typed target"));
                }
                let span = location.span;
                let target_path = self.source_path(target).ok_or_else(|| {
                    binding("editorial target has no retained source association")
                })?;
                let definition = match self
                    .origin(target, &identity)
                    .and_then(|origin| origin.span)
                {
                    Some(span) => Location {
                        span,
                        precision: self
                            .file(target)
                            .and_then(super::input::Input::from_file)
                            .and_then(|input| input.field(&identity))
                            .map_or(Precision::Construct, |location| location.precision),
                    },
                    None => {
                        self.reference_definition(target, "", span)
                            .map_err(|_| {
                                binding("derived editorial target has no source construct")
                            })?
                            .1
                    }
                };
                if references.len() >= occurrence_limit {
                    return Err(ProjectionError::Limit("editorial reference occurrences"));
                }
                bytes = bytes
                    .checked_add(std::mem::size_of::<EditorialReference<'_>>())
                    .and_then(|n| n.checked_add(path.as_str().len()))
                    .and_then(|n| n.checked_add(target_path.as_str().len()))
                    .filter(|&n| n <= byte_limit)
                    .ok_or(ProjectionError::Limit("editorial reference bytes"))?;
                references.push(EditorialReference {
                    path,
                    location,
                    target: target_path,
                    definition,
                });
                Ok(())
            };
            visit(&metadata, &mut add)?;
        }
        references.sort_by(|left, right| {
            left.path
                .cmp(right.path)
                .then(left.location.span.cmp(&right.location.span))
        });
        Ok(references)
    }
}

/// The sole field-to-meaning walk shared by resolved navigation and repair proposals.
pub(super) fn visit<S>(
    metadata: &Metadata<S>,
    mut add: impl FnMut(&Key, &[PathSegment], EditorialField) -> Result<(), ProjectionError>,
) -> Result<(), ProjectionError> {
    for (index, target) in metadata.collections.iter().enumerate() {
        add(
            target,
            &[key("collections"), PathSegment::Index(index)],
            EditorialField::CollectionResidence,
        )?;
    }
    match &metadata.content.structure {
        ContentKind::Collection {
            parent: Some(parent),
            ..
        } => {
            add(
                parent,
                &[key("content"), key("structure"), key("parent")],
                EditorialField::CollectionParent,
            )?;
        }
        ContentKind::Journey {
            collection,
            members,
        } => {
            if let Some(collection) = collection {
                add(
                    collection,
                    &[key("content"), key("structure"), key("collection")],
                    EditorialField::JourneyCollection,
                )?;
            }
            for (index, member) in members.iter().enumerate() {
                add(
                    &member.content,
                    &[
                        key("content"),
                        key("structure"),
                        key("members"),
                        PathSegment::Index(index),
                        key("content"),
                    ],
                    EditorialField::JourneyMember,
                )?;
            }
        }
        _ => {}
    }
    if let ContentKind::Collection { member_order, .. } = &metadata.content.structure {
        for (index, member) in member_order.iter().enumerate() {
            add(
                member,
                &[
                    key("content"),
                    key("structure"),
                    key("member_order"),
                    PathSegment::Index(index),
                ],
                EditorialField::CollectionOrder,
            )?;
        }
    }
    if let ContentKind::Collection {
        navigation: Some(items),
        ..
    } = &metadata.content.structure
    {
        use gazette_core::publication::content::NavigationItem;
        let prefix = vec![key("content"), key("structure"), key("navigation")];
        let mut stack: Vec<_> = items
            .iter()
            .enumerate()
            .rev()
            .map(|(index, item)| {
                let mut path = prefix.clone();
                path.push(PathSegment::Index(index));
                (item, path)
            })
            .collect();
        while let Some((item, mut path)) = stack.pop() {
            match item {
                NavigationItem::Content { content } => {
                    path.push(key("content"));
                    add(content, &path, EditorialField::DocumentationNavigation)?;
                }
                NavigationItem::Group { pages, .. } => {
                    path.push(key("pages"));
                    stack.extend(pages.iter().enumerate().rev().map(|(index, item)| {
                        let mut path = path.clone();
                        path.push(PathSegment::Index(index));
                        (item, path)
                    }));
                }
            }
        }
    }
    Ok(())
}

fn key(value: &str) -> PathSegment {
    PathSegment::Key(value.to_owned())
}
