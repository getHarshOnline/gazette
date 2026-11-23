//! Proposed destination inventory, not permission to mutate the observed tree.

use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};
use std::path::{Path, PathBuf};

use caseless::Caseless;
use gazette_core::destination::Error;
use gazette_core::source::{Content, EntryPath, Inventory, InventoryBuilder, Limits, Sha256Digest};
use gazette_ports::destination::DestinationReader;
use unicode_normalization::UnicodeNormalization;

use super::{PreparedSourceSet, SourceSelection, StageContext, materialization};

/// Complete proposed inventory based on a write-free destination observation.
///
/// Existing bytes and acquired source bytes are represented together. This
/// grants no writer capability, durable tree ownership or publication approval;
/// an eventual writer must revalidate the exact inputs at its effect boundary.
#[derive(Debug)]
pub struct CheckedDestination {
    inventory: Inventory,
}

impl CheckedDestination {
    /// Expected complete relative tree after the selected placements are added.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
}

/// Check source separation and construct a complete, collision-free proposed tree.
///
/// The existing content root must already exist and remain independent of all
/// selected repository roots. No existing target is overwritten, even if empty
/// or marked as previously generated. Shared exact-spelling ancestor directories
/// are allowed. Canonical Unicode normalization plus full case folding detects
/// ambiguous names conservatively even on a case-sensitive filesystem; original
/// spelling is never rewritten. This is not a universal filesystem-equivalence
/// claim or an atomic snapshot. No destination path is created or changed.
///
/// Limits apply to both the existing observation and the complete proposed tree.
///
/// # Errors
///
/// Fails on unsafe/changing native roots, source overlap, existing targets,
/// file/directory or canonical-caseless collisions, or exhausted tree bounds.
pub fn check_destination(
    reader: &impl DestinationReader,
    prepared: &PreparedSourceSet,
    context: &StageContext,
    content_root: &Path,
    limits: Limits,
) -> Result<CheckedDestination, Error> {
    let generated = materialization::generate(prepared, context)?;
    let roots = protected_roots(prepared);
    let existing = reader.read_destination(content_root, &roots, limits)?;
    Ok(CheckedDestination {
        inventory: compose(&existing, prepared, &generated, limits)?,
    })
}

pub(super) fn protected_roots(prepared: &PreparedSourceSet) -> Vec<PathBuf> {
    prepared
        .placements()
        .iter()
        .map(|placement| match &placement.request().source {
            SourceSelection::Filesystem { root, .. }
            | SourceSelection::FilteredFilesystem { root, .. }
            | SourceSelection::WorkingRepository { root, .. }
            | SourceSelection::IncludedFilesystem { root, .. } => root.clone(),
            SourceSelection::Repository { root, .. } => root.clone(),
            SourceSelection::Submodule { parent, .. } => parent.clone(),
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn compose(
    existing: &Inventory,
    prepared: &PreparedSourceSet,
    generated: &BTreeMap<EntryPath, Vec<u8>>,
    limits: Limits,
) -> Result<Inventory, Error> {
    let mut layout = Layout {
        inventory: InventoryBuilder::new(limits),
        names: BTreeMap::new(),
    };
    for entry in existing.entries() {
        layout.insert(entry.path.as_str(), entry.content, false)?;
    }
    for placement in prepared.placements() {
        let target = placement.request().target.as_str();
        for end in target
            .match_indices('/')
            .map(|(index, _)| index)
            .chain(std::iter::once(target.len()))
        {
            layout.insert(&target[..end], Content::Directory, end != target.len())?;
        }
        for entry in placement.source().inventory().entries() {
            layout.insert(&format!("{target}/{}", entry.path), entry.content, false)?;
        }
    }
    for (path, bytes) in generated {
        layout.insert(
            path.as_str(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
            false,
        )?;
    }
    Ok(layout.inventory.finish())
}

struct Layout {
    inventory: InventoryBuilder,
    names: BTreeMap<String, (String, bool)>,
}

pub(crate) fn collision_key(path: &str) -> String {
    path.nfd().default_case_fold().nfd().collect()
}

impl Layout {
    fn insert(
        &mut self,
        path: &str,
        content: Content,
        shared_directory: bool,
    ) -> Result<(), Error> {
        let key = collision_key(path);
        let directory = matches!(content, Content::Directory);
        match self.names.entry(key) {
            Entry::Occupied(entry) => {
                let (first, is_directory) = entry.get();
                if shared_directory && directory && *is_directory && first == path {
                    return Ok(());
                }
                return Err(Error::Collision {
                    first: first.clone(),
                    second: path.to_owned(),
                });
            }
            Entry::Vacant(entry) => {
                entry.insert((path.to_owned(), directory));
            }
        }
        self.inventory.insert(path.parse()?, content)?;
        Ok(())
    }
}
