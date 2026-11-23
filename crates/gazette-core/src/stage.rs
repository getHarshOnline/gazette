//! Explicit failures of fresh staging, retention and cleanup, not release transactions.

use std::fmt;
use std::path::PathBuf;

/// Canonical parent and literal child selected before a preparation write.
/// Serialization preserves a locator, not a native identity lock or cleanup authority.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct Address {
    /// Verified canonical absolute parent.
    pub parent: PathBuf,
    /// One validated child component; never a generated fallback.
    pub name: String,
}

impl Address {
    /// Exact selected child, without filesystem access or normalization.
    pub fn path(&self) -> PathBuf {
        self.parent.join(&self.name)
    }
}

/// Native synchronization and read-back evidence, not an immutable artifact or approval.
/// Dropping this value has no cleanup effect. Reverify its inventory before later use.
#[derive(Debug)]
#[must_use = "retain the inventory independently; dropping this evidence does not remove files"]
pub struct RetainedStage {
    path: PathBuf,
    inventory: crate::source::Inventory,
}

impl RetainedStage {
    /// Record evidence supplied by the native retention owner; does not itself perform I/O.
    pub fn new(path: PathBuf, inventory: crate::source::Inventory) -> Self {
        Self { path, inventory }
    }
    /// Retained native locator, not part of content identity.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
    /// Complete inventory observed after synchronization, not perpetual freshness.
    pub fn inventory(&self) -> &crate::source::Inventory {
        &self.inventory
    }
}

/// Explicit staging locator selection, independent from content identity.
/// A named child allows a caller to durably record its intended location before
/// creation. No variant grants ownership of an existing path.
#[derive(Clone, Copy, Debug)]
pub enum Location<'a> {
    /// Choose an exclusive fresh child beneath this existing parent.
    Fresh(&'a std::path::Path),
    /// Fresh child whose complete canonical native path is bounded before creation.
    /// This permits callers to reserve lossless failure/receipt delivery before effects.
    FreshBounded {
        /// Existing private parent, subject to the same native identity checks.
        parent: &'a std::path::Path,
        /// Positive maximum encoded native path bytes, at most one MiB.
        directory_bytes: usize,
    },
    /// Create exactly this single child; a collision must fail without fallback.
    Named {
        /// Existing trusted parent, separate from every protected source.
        parent: &'a std::path::Path,
        /// One literal relative path component, checked before native effects.
        name: &'a str,
    },
    /// Create exactly one named child with a bounded complete canonical native path.
    /// The byte bound is checked before creation; a collision never selects a fallback.
    NamedBounded {
        /// Existing trusted parent, separate from every protected source.
        parent: &'a std::path::Path,
        /// One literal relative path component, checked before native effects.
        name: &'a str,
        /// Positive maximum encoded native path bytes, at most one MiB.
        directory_bytes: usize,
    },
}

impl<'a> Location<'a> {
    /// Parent whose native identity and separation the writer must verify.
    pub fn parent(self) -> &'a std::path::Path {
        match self {
            Self::Fresh(parent)
            | Self::FreshBounded { parent, .. }
            | Self::Named { parent, .. }
            | Self::NamedBounded { parent, .. } => parent,
        }
    }
}

/// A stage was not returned, or explicit retention/cleanup could not complete safely.
#[derive(Debug)]
pub enum Error {
    /// No writes started; input or source/destination admission failed.
    Preflight(crate::destination::Error),
    /// A native operation failed and no stage residue remains.
    Operation(crate::source::Error),
    /// A new private stage remains; it is not complete or approved for reuse.
    Residue {
        /// Original operation failure.
        cause: crate::source::Error,
        /// Exact residue locator, not permission to remove it in a later run.
        path: PathBuf,
        /// Additional cleanup failure, if cleanup was attempted.
        cleanup: Option<Box<crate::source::Error>>,
    },
}

impl From<crate::destination::Error> for Error {
    fn from(error: crate::destination::Error) -> Self {
        Self::Preflight(error)
    }
}

impl From<crate::source::Error> for Error {
    fn from(error: crate::source::Error) -> Self {
        Self::Preflight(error.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Preflight(error) => write!(f, "stage preflight failed: {error}"),
            Self::Operation(error) => write!(f, "staging failed: {error}; no stage remains"),
            Self::Residue {
                cause,
                path,
                cleanup,
            } => {
                write!(
                    f,
                    "staging failed: {cause}; untrusted residue remains at {path:?}; do not reuse or delete it without ownership reconciliation"
                )?;
                if let Some(error) = cleanup {
                    write!(f, "; cleanup: {error}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Preflight(error) => error,
            Self::Operation(error) | Self::Residue { cause: error, .. } => error,
        })
    }
}
