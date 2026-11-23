//! Exact revision observations; not source acquisition or publication admission.

use gazette_core::source::{
    CommitId, Error, Inventory, MetadataBudget, RevisionLimits, Subdirectory,
};
use gazette_ports::source::{RevisionReader, SourceReader};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Observed source bytes match the selected commit.
/// `is_selected_only` distinguishes selected inputs from the complete subtree.
///
/// This is neither an atomic snapshot nor a permission-bearing receipt. A later
/// consumer must acquire/revalidate the exact bytes at its own effect boundary.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct VerifiedSource {
    commit: CommitId,
    subdirectory: Subdirectory,
    inventory: Inventory,
    #[serde(skip_serializing_if = "is_false")]
    selected_only: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl VerifiedSource {
    /// Whether verification covers only the retained selected inputs, not the whole subtree.
    pub fn is_selected_only(&self) -> bool {
        self.selected_only
    }

    /// The commit used for the comparison.
    pub fn commit(&self) -> &CommitId {
        &self.commit
    }

    /// The literal selected subtree, relative to the repository root.
    pub fn subdirectory(&self) -> &Subdirectory {
        &self.subdirectory
    }

    /// Matching directory paths and regular-file byte identities.
    pub fn inventory(&self) -> &Inventory {
        &self.inventory
    }
}

/// Compare confined source observations to their explicitly selected revision.
///
/// Changes outside the selected subtree are not publication inputs here.
/// Untracked and ignored entries inside it are included and must also match.
///
/// # Errors
///
/// Propagates either reader's failure or reports the first mismatched path.
/// No partial or mismatched inventory is returned as verified.
pub fn verify(
    source: &impl SourceReader,
    revisions: &impl RevisionReader,
    root: &Path,
    subdirectory: &Subdirectory,
    commit: &CommitId,
    limits: RevisionLimits,
) -> Result<VerifiedSource, Error> {
    let observed = super::inspect(source, root, Some(subdirectory), limits.content)?;
    verify_observed(revisions, root, subdirectory, commit, limits, observed)
}

pub(super) fn verify_observed(
    revisions: &impl RevisionReader,
    root: &Path,
    subdirectory: &Subdirectory,
    commit: &CommitId,
    limits: RevisionLimits,
    observed: Inventory,
) -> Result<VerifiedSource, Error> {
    let expected = read_baseline(revisions, root, subdirectory, commit, limits)?;
    compare(observed, expected, subdirectory, commit)
}

pub(super) fn read_baseline(
    revisions: &impl RevisionReader,
    root: &Path,
    subdirectory: &Subdirectory,
    commit: &CommitId,
    limits: RevisionLimits,
) -> Result<Inventory, Error> {
    let mut remaining_metadata = MetadataBudget::new(limits.metadata_bytes);
    revisions.read_revision(
        root,
        subdirectory,
        commit,
        limits.content,
        &mut remaining_metadata,
    )
}

fn compare(
    observed: Inventory,
    expected: Inventory,
    subdirectory: &Subdirectory,
    commit: &CommitId,
) -> Result<VerifiedSource, Error> {
    let shared = observed.entries().len().min(expected.entries().len());
    let mismatch = observed
        .entries()
        .iter()
        .zip(expected.entries())
        .find(|(a, b)| a != b)
        .map(|(a, b)| std::cmp::min(&a.path, &b.path).as_str())
        .or_else(|| {
            observed
                .entries()
                .get(shared)
                .or_else(|| expected.entries().get(shared))
                .map(|e| e.path.as_str())
        });
    // Canonical inventories derive their totals from these exact entries.
    if let Some(path) = mismatch {
        return Err(Error::ContentMismatch(PathBuf::from(path)));
    }
    Ok(VerifiedSource {
        commit: commit.clone(),
        subdirectory: subdirectory.clone(),
        inventory: observed,
        selected_only: false,
    })
}

/// A source observation bound to its parent commit and exact gitlink path.
///
/// This transports provenance, not publication authority or atomic acquisition.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct VerifiedSubmodule {
    parent_commit: CommitId,
    submodule_path: Subdirectory,
    source: VerifiedSource,
}

impl VerifiedSubmodule {
    /// Exact parent revision from which the child pin was read.
    pub fn parent_commit(&self) -> &CommitId {
        &self.parent_commit
    }

    /// Literal gitlink location beneath the parent root.
    pub fn submodule_path(&self) -> &Subdirectory {
        &self.submodule_path
    }

    /// Child commit and matching source observation.
    pub fn source(&self) -> &VerifiedSource {
        &self.source
    }
}

/// Verify child documentation using only the parent's exact gitlink selection.
///
/// No separate sibling path or child revision is accepted. Filesystem reads are
/// confined from the parent handle through both the child and documentation
/// paths. A single metadata budget covers both parent checks and the child read.
///
/// # Errors
///
/// Fails on unsafe paths, missing children, parent/child pin mismatch, changed
/// bytes or any Git-reader failure. No child is fetched or initialized.
pub fn verify_submodule(
    source: &impl SourceReader,
    revisions: &impl RevisionReader,
    parent: &Path,
    parent_commit: &CommitId,
    submodule_path: &Subdirectory,
    subdirectory: &Subdirectory,
    limits: RevisionLimits,
) -> Result<VerifiedSubmodule, Error> {
    let selected = submodule_path.join(subdirectory);
    let observed = super::inspect(source, parent, Some(&selected), limits.content)?;
    verify_child_observed(
        revisions,
        parent,
        parent_commit,
        submodule_path,
        subdirectory,
        limits,
        observed,
    )
}

pub(super) fn verify_child_observed(
    revisions: &impl RevisionReader,
    parent: &Path,
    parent_commit: &CommitId,
    submodule_path: &Subdirectory,
    subdirectory: &Subdirectory,
    limits: RevisionLimits,
    observed: Inventory,
) -> Result<VerifiedSubmodule, Error> {
    let (child_commit, expected) = read_child_baseline(
        revisions,
        parent,
        parent_commit,
        submodule_path,
        subdirectory,
        limits,
    )?;
    let source = compare(observed, expected, subdirectory, &child_commit)?;
    Ok(VerifiedSubmodule {
        parent_commit: parent_commit.clone(),
        submodule_path: submodule_path.clone(),
        source,
    })
}

pub(super) fn read_child_baseline(
    revisions: &impl RevisionReader,
    parent: &Path,
    parent_commit: &CommitId,
    submodule_path: &Subdirectory,
    subdirectory: &Subdirectory,
    limits: RevisionLimits,
) -> Result<(CommitId, Inventory), Error> {
    let mut remaining_metadata = MetadataBudget::new(limits.metadata_bytes);
    let child_commit = revisions.read_gitlink(
        parent,
        parent_commit,
        submodule_path,
        &mut remaining_metadata,
    )?;
    let child_root = parent.join(submodule_path.to_string());
    let expected = revisions.read_revision(
        &child_root,
        subdirectory,
        &child_commit,
        limits.content,
        &mut remaining_metadata,
    )?;
    let after = revisions.read_gitlink(
        parent,
        parent_commit,
        submodule_path,
        &mut remaining_metadata,
    )?;
    if after != child_commit {
        return Err(Error::Changed(PathBuf::from(submodule_path.to_string())));
    }
    Ok((child_commit, expected))
}

/// Compare only selected manual inputs while preserving exact commit and gitlink checks.
pub(super) fn verify_selected(
    revisions: &impl RevisionReader,
    selection: &super::SourceSelection,
    filter: &dyn gazette_ports::source::SourceFilter,
    limits: RevisionLimits,
    observed: Inventory,
) -> Result<SelectedVerification, Error> {
    let mut metadata = MetadataBudget::new(limits.metadata_bytes);
    match selection {
        super::SourceSelection::Repository {
            root,
            subdirectory,
            commit,
        } => {
            let expected = revisions.read_revision_filtered(
                root,
                subdirectory,
                commit,
                filter,
                limits.content,
                &mut metadata,
            )?;
            let mut verified = compare(observed, expected, subdirectory, commit)?;
            verified.selected_only = true;
            Ok(SelectedVerification::Repository(verified))
        }
        super::SourceSelection::Submodule {
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
        } => {
            let commit =
                revisions.read_gitlink(parent, parent_commit, submodule_path, &mut metadata)?;
            let root = parent.join(submodule_path.to_string());
            let expected = revisions.read_revision_filtered(
                &root,
                subdirectory,
                &commit,
                filter,
                limits.content,
                &mut metadata,
            )?;
            let after =
                revisions.read_gitlink(parent, parent_commit, submodule_path, &mut metadata)?;
            if after != commit {
                return Err(Error::Changed(PathBuf::from(submodule_path.to_string())));
            }
            let mut source = compare(observed, expected, subdirectory, &commit)?;
            source.selected_only = true;
            Ok(SelectedVerification::Submodule(VerifiedSubmodule {
                parent_commit: parent_commit.clone(),
                submodule_path: submodule_path.clone(),
                source,
            }))
        }
        _ => unreachable!("selected verification requires a pinned source"),
    }
}

pub(super) enum SelectedVerification {
    Repository(VerifiedSource),
    Submodule(VerifiedSubmodule),
}
