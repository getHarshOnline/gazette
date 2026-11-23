//! Retain and verify source bytes for consumers that must not reopen live input.

use std::path::Path;

use gazette_core::source::{
    CapturedSource, CommitId, EntryPath, Error, FileContents, RevisionLimits, Subdirectory,
};
use gazette_ports::source::{RevisionReader, SourceCapture};

use super::revision::{verify_child_observed, verify_observed};
use super::{SourceSelection, VerifiedSource, VerifiedSubmodule};

pub(super) fn capture_selection(
    reader: &impl SourceCapture,
    selection: &SourceSelection,
    limits: RevisionLimits,
) -> Result<CapturedSource, Error> {
    match selection {
        SourceSelection::Filesystem { root, subdirectory } => {
            reader.capture(root, subdirectory.as_ref(), limits.content)
        }
        SourceSelection::FilteredFilesystem { root, exclude } => {
            reader.capture_excluding(root, exclude, limits.content)
        }
        SourceSelection::IncludedFilesystem { root, include, .. } => {
            reader.capture_including(root, include, limits.content)
        }
        SourceSelection::WorkingRepository { root, .. } => {
            // This selection requires the Git filter supplied by Batch.
            Err(Error::Unsupported(root.clone()))
        }
        SourceSelection::Repository {
            root, subdirectory, ..
        } => reader.capture(root, Some(subdirectory), limits.content),
        SourceSelection::Submodule {
            parent,
            submodule_path,
            subdirectory,
            ..
        } => {
            let selected = submodule_path.join(subdirectory);
            reader.capture(parent, Some(&selected), limits.content)
        }
    }
}

/// Retained immutable source payloads paired with their typed revision evidence.
///
/// The application creates this only after capture and revision comparison.
/// Native input may change afterward; consumers use these owned bytes instead
/// of reopening it. This grants neither publication permission nor durable
/// generated-tree ownership, and dropping it discards the in-memory capture.
#[derive(Debug)]
pub struct AcquiredSource<P> {
    verification: P,
    contents: FileContents,
}

impl<P> AcquiredSource<P> {
    /// Revision/source evidence for this exact in-memory capture.
    pub fn verification(&self) -> &P {
        &self.verification
    }

    /// Read retained source bytes without accessing the original filesystem.
    pub fn file(&self, path: &EntryPath) -> Option<&[u8]> {
        self.contents.get(path)
    }
}

/// Capture and verify an exact repository subtree without any filesystem writes.
///
/// # Errors
///
/// Rejects unsafe/changing input, retained-byte mismatch, wrong revisions and
/// exhausted limits. No partial acquired source is returned.
pub fn acquire(
    source: &impl SourceCapture,
    revisions: &impl RevisionReader,
    root: &Path,
    subdirectory: &Subdirectory,
    commit: &CommitId,
    limits: RevisionLimits,
) -> Result<AcquiredSource<VerifiedSource>, Error> {
    let captured = capture_selection(
        source,
        &SourceSelection::Repository {
            root: root.to_owned(),
            subdirectory: subdirectory.clone(),
            commit: commit.clone(),
        },
        limits,
    )?;
    let (observed, contents) = captured.into_parts();
    let verification = verify_observed(revisions, root, subdirectory, commit, limits, observed)?;
    Ok(AcquiredSource {
        verification,
        contents,
    })
}

/// Capture child documentation using only the parent's exact gitlink selection.
///
/// # Errors
///
/// Propagates capture failures and the same parent/child pin, byte and metadata
/// budget failures as inventory-only submodule verification.
pub fn acquire_submodule(
    source: &impl SourceCapture,
    revisions: &impl RevisionReader,
    parent: &Path,
    parent_commit: &CommitId,
    submodule_path: &Subdirectory,
    subdirectory: &Subdirectory,
    limits: RevisionLimits,
) -> Result<AcquiredSource<VerifiedSubmodule>, Error> {
    let (observed, contents) = capture_selection(
        source,
        &SourceSelection::Submodule {
            parent: parent.to_owned(),
            parent_commit: parent_commit.clone(),
            submodule_path: submodule_path.clone(),
            subdirectory: subdirectory.clone(),
        },
        limits,
    )?
    .into_parts();
    let verification = verify_child_observed(
        revisions,
        parent,
        parent_commit,
        submodule_path,
        subdirectory,
        limits,
        observed,
    )?;
    Ok(AcquiredSource {
        verification,
        contents,
    })
}

/// Bind selected captures to the source's existing revision contract.
pub(crate) fn bind_selected(
    revisions: &impl RevisionReader,
    selection: &SourceSelection,
    filter: &dyn gazette_ports::source::SourceFilter,
    limits: RevisionLimits,
    captured: CapturedSource,
) -> Result<super::AcquiredInput, Error> {
    if !matches!(
        selection,
        SourceSelection::Repository { .. } | SourceSelection::Submodule { .. }
    ) {
        return Ok(super::AcquiredInput::Filesystem(captured));
    }
    let (observed, contents) = captured.into_parts();
    match super::revision::verify_selected(revisions, selection, filter, limits, observed)? {
        super::revision::SelectedVerification::Repository(verification) => {
            Ok(super::AcquiredInput::Repository(AcquiredSource {
                verification,
                contents,
            }))
        }
        super::revision::SelectedVerification::Submodule(verification) => {
            Ok(super::AcquiredInput::Submodule(AcquiredSource {
                verification,
                contents,
            }))
        }
    }
}
