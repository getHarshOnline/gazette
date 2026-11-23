//! Explicit working-byte acquisition with preserved, independently read Git baseline.

use gazette_core::source::{
    CommitId, EntryPath, Error, FileContents, Inventory, RevisionLimits, Subdirectory, TreeIdentity,
};
use gazette_ports::source::{RevisionReader, SourceCapture};
use serde::Serialize;

use super::{SourceSelection, revision};

/// Exact configured lineage, not a claim that working bytes match this tree.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DevelopmentBaseline {
    /// A repository remains checked out at the configured commit.
    Repository {
        /// Configured checked-out revision, never learned from dirty input.
        commit: CommitId,
        /// Literal selected subtree.
        subdirectory: Subdirectory,
        /// Independently read committed subtree identity.
        tree: TreeIdentity,
    },
    /// Parent and child remain at the exact configured parent/gitlink revisions.
    Submodule {
        /// Configured checked-out parent revision.
        parent_commit: CommitId,
        /// Literal gitlink location below the parent.
        submodule_path: Subdirectory,
        /// Child revision read from that exact gitlink.
        commit: CommitId,
        /// Literal selected subtree below the child.
        subdirectory: Subdirectory,
        /// Independently read committed child subtree identity.
        tree: TreeIdentity,
    },
}

/// Confined retained development bytes, deliberately not a verified source.
///
/// The inventory identifies the actual capture, including additions and deletions.
/// Even a clean capture remains development evidence, not release admission.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DevelopmentSource {
    baseline: DevelopmentBaseline,
    inventory: Inventory,
    #[serde(skip)]
    contents: FileContents,
}

impl DevelopmentSource {
    /// Preserved Git baseline, independently bounded and checked by the revision reader.
    pub fn baseline(&self) -> &DevelopmentBaseline {
        &self.baseline
    }

    /// Exact captured working-tree identity, not the baseline tree identity.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Borrow the frozen working bytes without reopening the native source.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.contents.get(path)
    }
}

/// Capture development bytes without relaxing source safety or configured Git pins.
///
/// Uses the same confined capture and revision readers as strict acquisition.
/// Working bytes need not match the baseline, but the declared checkout and
/// parent/gitlink selection must still hold. No source or Git metadata is written.
///
/// # Errors
/// Rejects unsafe/changing inputs, exhausted bounds and incorrect checkouts or gitlinks.
pub fn acquire_development(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    selection: &SourceSelection,
    limits: RevisionLimits,
) -> Result<DevelopmentSource, Error> {
    let (inventory, contents) =
        super::acquire::capture_selection(reader, selection, limits)?.into_parts();
    let baseline = match selection {
        SourceSelection::Filesystem { root, .. }
        | SourceSelection::FilteredFilesystem { root, .. }
        | SourceSelection::WorkingRepository { root, .. }
        | SourceSelection::IncludedFilesystem { root, .. } => {
            return Err(Error::InvalidInventory {
                path: root.clone(),
                reason: "filesystem sources do not have a Git development baseline",
            });
        }
        SourceSelection::Repository {
            root,
            subdirectory,
            commit,
        } => {
            let expected = revision::read_baseline(revisions, root, subdirectory, commit, limits)?;
            DevelopmentBaseline::Repository {
                commit: commit.clone(),
                subdirectory: subdirectory.clone(),
                tree: expected.identity(),
            }
        }
        SourceSelection::Submodule {
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
        } => {
            let (commit, expected) = revision::read_child_baseline(
                revisions,
                parent,
                parent_commit,
                submodule_path,
                subdirectory,
                limits,
            )?;
            DevelopmentBaseline::Submodule {
                parent_commit: parent_commit.clone(),
                submodule_path: submodule_path.clone(),
                commit,
                subdirectory: subdirectory.clone(),
                tree: expected.identity(),
            }
        }
    };
    Ok(DevelopmentSource {
        baseline,
        inventory,
        contents,
    })
}
