//! Collection purpose follows the validated containment graph, not project registration.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{ContentKind, ContentNode, Diagnostic, Key, PageFamily};

/// A collection's reader purpose, independent of its subject and presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CollectionPurpose {
    /// Sustained editorial writing; the compatible default for an unclassified root.
    Editorial,
    /// Connected product understanding and use, including ordered tasks and examples.
    Documentation,
}

pub(super) fn resolve(
    nodes: &BTreeMap<Key, ContentNode>,
    parents: &BTreeMap<Key, Key>,
    errors: &mut Vec<Diagnostic>,
) -> BTreeSet<Key> {
    let mut purposes = BTreeMap::new();
    // Each node enters a path once. Parent chains have already been proved acyclic.
    for id in nodes.keys() {
        let mut path = Vec::new();
        let mut cursor = Some(id);
        while let Some(current) = cursor {
            if purposes.contains_key(current) {
                break;
            }
            path.push(current);
            cursor = parents.get(current);
        }
        let mut inherited = cursor
            .and_then(|parent| purposes.get(parent).copied())
            .unwrap_or(CollectionPurpose::Editorial);
        for current in path.into_iter().rev() {
            match nodes[current].structure {
                ContentKind::Collection {
                    purpose: Some(purpose),
                    ..
                } => {
                    if parents.contains_key(current) && purpose != inherited {
                        errors.push(Diagnostic {
                            code: "collection_purpose_conflict",
                            source: current.clone(),
                            target: parents.get(current).cloned(),
                        });
                    }
                    inherited = purpose;
                }
                ContentKind::Page {
                    family: PageFamily::Documentation,
                } => {
                    inherited = CollectionPurpose::Documentation;
                }
                _ => (),
            }
            purposes.insert(current, inherited);
        }
    }
    purposes
        .into_iter()
        .filter(|(_, purpose)| *purpose == CollectionPurpose::Documentation)
        .map(|(id, _)| id.clone())
        .collect()
}
