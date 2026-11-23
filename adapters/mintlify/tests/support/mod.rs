//! Real retained input trees shared by the target boundary fixtures.

use gazette_core::{
    documentation::{Artifact, Format, Selection},
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use std::collections::BTreeMap;

pub(super) struct Input {
    pub(super) tree: CapturedSource,
    pub(super) selection: Selection,
}

impl Input {
    pub(super) fn new(bytes: &[u8]) -> Self {
        let entry: EntryPath = "reference.json".parse().expect("literal input path");
        let mut inventory = InventoryBuilder::new(Limits::default());
        inventory
            .insert(
                entry.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("fixture inventory");
        let tree = CapturedSource::from_parts(
            inventory.finish(),
            BTreeMap::from([(entry.clone(), bytes.to_vec())]),
        )
        .expect("exact retained input");
        Self {
            tree,
            selection: Selection::File { path: entry },
        }
    }

    pub(super) fn artifact(&self, format: Format) -> Artifact<'_> {
        Artifact {
            format,
            tree: self.tree.as_input(),
            selection: &self.selection,
        }
    }
}
