//! Source-separated local object retention; no journal or release approval.

use gazette_core::{
    release::{CommitMetadata, CustodyError, CustodyOptions, CustodyReport},
    source::{CommitId, RevisionLimits, Sha256Digest, TreeIdentity},
};
use gazette_ports::{destination::RootSeparation, release::ReleaseCustody};
use std::path::{Path, PathBuf};

/// Exact existing object and source-separation inputs for local retention.
pub struct CustodyRequest<'a> {
    /// Explicit independent local Git storage.
    pub repository: &'a Path,
    /// Immutable local custody coordinate, not publisher authorization.
    pub id: Sha256Digest,
    /// Existing full commit coordinate, never a predicted object.
    pub commit: &'a CommitId,
    /// Independently retained complete output identity.
    pub output: TreeIdentity,
    /// Exact deterministic recipe to compare with native bytes.
    pub metadata: &'a CommitMetadata,
    /// Every configured source/output root; the repository must be disjoint.
    pub protected_roots: &'a [PathBuf],
}

/// Retain verified local objects behind an immutable native custody ref.
/// The caller must keep the selected repository under trusted exclusive control.
/// This performs no journal write, branch update, signing or remote operation.
///
/// # Errors
/// Refuses permission or root overlap before native mutation; preserves adapter
/// conflict, verification, synchronization and possible-ref-effect evidence.
pub fn retain_commit(
    git: &impl ReleaseCustody,
    separation: &impl RootSeparation,
    request: CustodyRequest<'_>,
    options: CustodyOptions,
) -> Result<CustodyReport, CustodyError> {
    if !options.allow_retain {
        return Err(CustodyError::refused(
            "explicit local object-retention permission is required",
        ));
    }
    separation
        .check_root_separation(request.repository, request.protected_roots)
        .map_err(|_| {
            CustodyError::refused(
                "custody storage overlaps or cannot be separated from protected inputs",
            )
        })?;
    git.retain_commit(
        request.repository,
        request.id,
        request.commit,
        request.output,
        request.metadata,
        options,
    )
}

/// Inspect exact local custody and objects, without synchronization or writes.
/// Does not establish that any preceding retention operation completed.
///
/// # Errors
/// Refuses unavailable, unsafe, mismatched or over-limit refs/objects.
pub fn inspect_custody(
    git: &impl ReleaseCustody,
    repository: &Path,
    id: Sha256Digest,
    commit: &CommitId,
    output: TreeIdentity,
    metadata: &CommitMetadata,
    limits: RevisionLimits,
) -> Result<CustodyReport, CustodyError> {
    git.inspect_custody(repository, id, commit, output, metadata, limits)
}
