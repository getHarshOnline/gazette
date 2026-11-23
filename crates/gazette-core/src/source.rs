//! Source observations and selection values; never publication permission.

use serde::Serialize;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

mod budget;
mod capture;
mod digest;
mod entry_path;
mod inventory;
mod listing;
mod path;
mod revision;
mod span;
mod target;
mod tree_identity;
pub use budget::{BudgetExceeded, MetadataBudget};
pub use capture::{CapturedSource, FileContents, TreeInput};
pub use digest::{InvalidSha256Digest, Sha256Digest};
pub use entry_path::EntryPath;
pub use inventory::{Inventory, InventoryBuilder};
pub use listing::{ListedEntry, Listing, ListingBuilder, ListingLimits};
pub use path::{InvalidSubdirectory, Subdirectory};
pub use revision::{CommitId, InvalidCommitId, RevisionLimits, valid_branch_name};
pub use span::{Location, Precision, Span};
pub use target::{InvalidTargetPath, TargetPath};
pub use tree_identity::TreeIdentity;

/// Resource ceilings for one observation, including hidden entries.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum files and directories, excluding the selected root.
    #[schemars(range(max = usize::MAX))]
    pub entries: usize,
    /// Maximum descendant depth; immediate children have depth one.
    #[schemars(range(max = usize::MAX))]
    pub depth: usize,
    /// Maximum admitted bytes in any file; growth detection may read one more.
    #[schemars(range(max = u64::MAX))]
    pub file_bytes: u64,
    /// Maximum combined admitted file bytes; growth detection may read one more.
    #[schemars(range(max = u64::MAX))]
    pub total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        // Bound descriptors, retained metadata and hashing work for interactive
        // inspection. Larger corpora require explicit ceilings.
        Self {
            entries: 10_000,
            depth: 64,
            file_bytes: 64 << 20,
            total_bytes: 512 << 20,
        }
    }
}

/// An observed descendant with a root-relative UTF-8 path.
#[derive(Debug, Serialize, PartialEq, Eq, schemars::JsonSchema)]
pub struct Entry {
    /// Forward-slash-separated path, never an absolute path.
    pub path: EntryPath,
    /// Directory presence or observed regular-file bytes.
    #[serde(flatten)]
    pub content: Content,
}

/// Filesystem content; links and special files have no accepted variant.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    /// A directory, including one with no children.
    Directory,
    /// A file read from its handle, not identified by timestamp or size alone.
    File {
        /// Bytes actually read.
        bytes: u64,
        /// Lowercase SHA-256 of those bytes.
        sha256: Sha256Digest,
    },
}

/// Why an observation could not be completed.
#[derive(Debug)]
pub enum Error {
    /// Native failure; no entry is silently omitted.
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// Selected root or root-relative entry involved.
        path: PathBuf,
        /// Native error retained for diagnostics.
        source: io::Error,
    },
    /// Link, device, socket or another unsupported entry.
    Unsupported(PathBuf),
    /// A name is not an unambiguous UTF-8 output path.
    InvalidName(PathBuf),
    /// A native reader supplied a duplicate or structurally inconsistent entry.
    InvalidInventory {
        /// Entry that could not be admitted.
        path: PathBuf,
        /// Violated structural invariant.
        reason: &'static str,
    },
    /// A declared ceiling would be exceeded.
    Limit {
        /// Entry being inspected.
        path: PathBuf,
        /// Resource that stopped inspection.
        resource: &'static str,
    },
    /// An entry changed during its observation.
    Changed(PathBuf),
    /// The worktree is detached or on a different branch than requested.
    BranchMismatch {
        /// Selected repository root.
        path: PathBuf,
        /// Explicit caller-selected local branch name.
        expected: String,
        /// Observed branch, or absence for detached HEAD.
        actual: Option<String>,
    },
    /// The selected repository's checked-out commit differs from the exact pin.
    RevisionMismatch {
        /// Selected repository root.
        path: PathBuf,
        /// Explicit caller-selected commit.
        expected: String,
        /// Observed checked-out commit.
        actual: String,
    },
    /// An observed path or its bytes differs from the selected commit tree.
    ContentMismatch(PathBuf),
    /// The complete captured tree differs from independently retained evidence.
    TreeMismatch {
        /// Selected native root.
        path: PathBuf,
        /// Explicit expected structural identity.
        expected: TreeIdentity,
        /// Identity of the complete confined capture.
        actual: TreeIdentity,
    },
}

impl Error {
    /// Machine-readable failure class, independent from diagnostic wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "source_io",
            Self::Unsupported(_) => "source_unsupported",
            Self::InvalidName(_) => "source_name",
            Self::InvalidInventory { .. } => "source_inventory",
            Self::Limit { .. } => "source_limit",
            Self::Changed(_) => "source_changed",
            Self::BranchMismatch { .. } => "source_branch_mismatch",
            Self::RevisionMismatch { .. } => "source_revision_mismatch",
            Self::ContentMismatch(_) => "source_content_mismatch",
            Self::TreeMismatch { .. } => "source_tree_mismatch",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(
                f,
                "cannot {operation} {path:?}: {source}; check the source and access"
            ),
            Self::Unsupported(path) => write!(
                f,
                "unsupported entry {path:?}; select only directories and regular files"
            ),
            Self::InvalidName(path) => write!(
                f,
                "invalid name beneath {path:?}; use UTF-8 names without controls or backslashes"
            ),
            Self::InvalidInventory { path, reason } => write!(
                f,
                "invalid source inventory at {path:?}: {reason}; inspect the reader and source structure"
            ),
            Self::Limit { path, resource } => write!(
                f,
                "{resource} ceiling exceeded at {path:?}; narrow the source or explicitly raise that ceiling"
            ),
            Self::Changed(path) => write!(
                f,
                "source changed at {path:?}; stop concurrent edits and inspect again"
            ),
            Self::BranchMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "repository {path:?} has branch {actual:?}, expected {expected:?}; inspect the checkout before retrying"
            ),
            Self::RevisionMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "repository {path:?} is at {actual}, not selected commit {expected}; inspect the checkout and explicitly select the intended pin"
            ),
            Self::ContentMismatch(path) => write!(
                f,
                "source at {path:?} differs from the selected commit tree; inspect local additions, removals and edits before retrying"
            ),
            Self::TreeMismatch {
                path,
                expected,
                actual,
            } => write!(
                f,
                "tree at {path:?} has {} digest {}, not expected {}; inspect additions, removals and edits against independent evidence before consuming it",
                TreeIdentity::FORMAT,
                actual.sha256(),
                expected.sha256()
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Decode one native entry name without lossy conversion or path normalization.
///
/// This shared rule applies to filesystem and committed-tree observations.
///
/// # Errors
///
/// Rejects non-UTF-8, empty/dot names, separators and control characters.
pub fn entry_name<'a>(raw: &'a [u8], parent: &Path) -> Result<&'a str, Error> {
    let name = std::str::from_utf8(raw).map_err(|_| Error::InvalidName(parent.to_owned()))?;
    if matches!(name, "" | "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(Error::InvalidName(parent.to_owned()));
    }
    Ok(name)
}

#[cfg(test)]
#[path = "../tests/internal/source.rs"]
mod tests;
