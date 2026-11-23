//! Failures in write-free destination inspection and proposed-tree construction.

use std::fmt;
use std::path::PathBuf;

mod output;
pub use output::{
    OutputAdministrationBinding, OutputBinding, OutputChange, OutputIdentity, OutputMutation,
    OutputObservation, OutputWriteError,
};

/// A destination cannot safely accommodate the complete selected source set.
#[derive(Debug)]
pub enum Error {
    /// Native observation, path, resource or proposed-inventory failure.
    Observation(crate::source::Error),
    /// The content root contains, equals, or lies inside a protected source root.
    SourceOverlap {
        /// Explicit source root that must not be modified.
        source: PathBuf,
        /// Explicit existing content root.
        destination: PathBuf,
    },
    /// Existing content, a conflicting ancestor, or canonical-caseless alias.
    Collision {
        /// Previously observed or planned relative path.
        first: String,
        /// Conflicting proposed or observed relative path.
        second: String,
    },
}

impl From<crate::source::Error> for Error {
    fn from(error: crate::source::Error) -> Self {
        Self::Observation(error)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Observation(error) => write!(f, "destination check failed: {error}"),
            Self::SourceOverlap {
                source,
                destination,
            } => write!(
                f,
                "destination {destination:?} overlaps protected source {source:?}; select an independent destination root"
            ),
            Self::Collision { first, second } => write!(
                f,
                "destination paths {first:?} and {second:?} collide; select disjoint, unambiguous placements without overwriting existing content"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Observation(error) => Some(error),
            _ => None,
        }
    }
}
