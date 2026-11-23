//! Shared bounded acquisition after a consumer's complete source-set preflight.

use gazette_core::source::{Error, RevisionLimits};
use gazette_ports::source::{RevisionReader, SourceCapture};

use super::{
    AcquiredInput, PreparationLimits, SourceSelection, acquire, acquire::capture_selection,
    acquire_development, acquire_submodule,
};

#[derive(Clone, Copy)]
pub(crate) enum AcquisitionMode {
    Strict,
    Development,
}

/// One acquisition budget; consumers own source-count, identity and target preflight.
pub(crate) struct Batch {
    per_source: RevisionLimits,
    remaining_entries: usize,
    remaining_bytes: u64,
    mode: AcquisitionMode,
}

/// Preserve native failures separately from complete-set accounting failures.
#[derive(Debug)]
pub(crate) enum BatchError {
    Native(Error),
    Aggregate(&'static str),
}

impl Batch {
    pub(crate) fn new(limits: PreparationLimits) -> Self {
        Self::with_mode(limits, AcquisitionMode::Strict)
    }

    pub(crate) fn with_mode(limits: PreparationLimits, mode: AcquisitionMode) -> Self {
        Self {
            per_source: limits.per_source,
            remaining_entries: limits.entries,
            remaining_bytes: limits.bytes,
            mode,
        }
    }

    /// Tighten native limits before reading, then account for the retained inventory.
    pub(crate) fn acquire(
        &mut self,
        reader: &impl SourceCapture,
        revisions: &impl RevisionReader,
        selection: &SourceSelection,
    ) -> Result<AcquiredInput, BatchError> {
        self.acquire_selected(reader, revisions, selection, false)
    }

    pub(crate) fn acquire_theme(
        &mut self,
        reader: &impl SourceCapture,
        revisions: &impl RevisionReader,
        selection: &SourceSelection,
    ) -> Result<AcquiredInput, BatchError> {
        self.acquire_selected(reader, revisions, selection, true)
    }

    fn acquire_selected(
        &mut self,
        reader: &impl SourceCapture,
        revisions: &impl RevisionReader,
        selection: &SourceSelection,
        theme: bool,
    ) -> Result<AcquiredInput, BatchError> {
        let selected = self.remaining_limits();
        let branch = match selection {
            SourceSelection::IncludedFilesystem {
                root,
                expected_branch: Some(expected),
                ..
            }
            | SourceSelection::WorkingRepository {
                root,
                expected_branch: Some(expected),
                ..
            } => Some((root, expected)),
            _ => None,
        };
        if let Some((root, expected)) = branch {
            revisions
                .verify_branch(root, expected)
                .map_err(BatchError::Native)?;
        }
        let implicit_tree = match selection {
            SourceSelection::Filesystem { root, subdirectory } if theme => {
                Some((root, subdirectory.as_ref(), &[][..]))
            }
            SourceSelection::FilteredFilesystem { root, exclude } if theme => {
                Some((root, None, exclude.as_slice()))
            }
            _ => None,
        };
        let filter = implicit_tree
            .map(|(root, subtree, _)| revisions.working_source_filter(root, subtree, selected))
            .transpose()
            .map_err(BatchError::Native)?
            .flatten();
        let source = if let Some(((root, subtree, exclude), filter)) =
            implicit_tree.zip(filter.as_deref())
        {
            reader
                .capture_filtered(root, subtree, exclude, filter, selected.content)
                .map(AcquiredInput::Filesystem)
        } else {
            match (selection, self.mode) {
                (
                    SourceSelection::WorkingRepository {
                        root, subdirectory, ..
                    },
                    _,
                ) => {
                    let filter = revisions
                        .working_source_filter(root, subdirectory.as_ref(), selected)
                        .map_err(BatchError::Native)?
                        .ok_or_else(|| BatchError::Native(Error::Unsupported(root.clone())))?;
                    reader
                        .capture_filtered(
                            root,
                            subdirectory.as_ref(),
                            &[],
                            filter.as_ref(),
                            selected.content,
                        )
                        .map(AcquiredInput::Filesystem)
                }
                (
                    SourceSelection::Filesystem { .. }
                    | SourceSelection::FilteredFilesystem { .. }
                    | SourceSelection::IncludedFilesystem { .. },
                    _,
                ) => capture_selection(reader, selection, selected).map(AcquiredInput::Filesystem),
                (
                    SourceSelection::Repository { .. } | SourceSelection::Submodule { .. },
                    AcquisitionMode::Development,
                ) => acquire_development(reader, revisions, selection, selected)
                    .map(AcquiredInput::Development),
                (
                    SourceSelection::Repository {
                        root,
                        subdirectory,
                        commit,
                    },
                    AcquisitionMode::Strict,
                ) => acquire(reader, revisions, root, subdirectory, commit, selected)
                    .map(AcquiredInput::Repository),
                (
                    SourceSelection::Submodule {
                        parent,
                        parent_commit,
                        submodule_path,
                        subdirectory,
                    },
                    AcquisitionMode::Strict,
                ) => acquire_submodule(
                    reader,
                    revisions,
                    parent,
                    parent_commit,
                    submodule_path,
                    subdirectory,
                    selected,
                )
                .map(AcquiredInput::Submodule),
            }
        }
        .map_err(BatchError::Native)?;
        if let Some((root, expected)) = branch {
            revisions
                .verify_branch(root, expected)
                .map_err(BatchError::Native)?;
        }
        self.retain(source)
    }

    pub(crate) fn remaining_limits(&self) -> RevisionLimits {
        let mut selected = self.per_source;
        selected.content.entries = selected.content.entries.min(self.remaining_entries);
        selected.content.total_bytes = selected.content.total_bytes.min(self.remaining_bytes);
        selected
    }

    pub(crate) fn retain(&mut self, source: AcquiredInput) -> Result<AcquiredInput, BatchError> {
        let remaining_entries = self
            .remaining_entries
            .checked_sub(source.inventory().entries().len())
            .ok_or(BatchError::Aggregate("entry count"))?;
        let remaining_bytes = self
            .remaining_bytes
            .checked_sub(source.inventory().total_bytes())
            .ok_or(BatchError::Aggregate("retained bytes"))?;
        self.remaining_entries = remaining_entries;
        self.remaining_bytes = remaining_bytes;
        Ok(source)
    }
}
