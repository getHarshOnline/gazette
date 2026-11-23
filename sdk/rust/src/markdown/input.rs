//! Shared complete-tree acquisition and selected-document evidence, separate from syntax parsing.

use crate::source::{self, CapturedSource, EntryPath, Limits, TreeIdentity};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Exact independent complete-tree pin; never inferred from the selected document's bytes.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tree {
    /// Explicit local root; transports may impose absolute-path requirements.
    pub root: PathBuf,
    /// Independently retained identity of every entry in the complete tree.
    pub identity: TreeIdentity,
    /// Literal confined regular-file selection; no extension inference.
    pub document: EntryPath,
    /// Bounds for the entire capture, including unrelated and hidden assets.
    #[serde(default)]
    pub limits: Limits,
}

impl Tree {
    /// Capture once through the native SDK and retain bytes for subsequent inspection.
    ///
    /// # Errors
    /// Refuses missing read permission, native source failures and complete-tree mismatch.
    pub fn load(self, allow_native_reads: bool) -> Result<Loaded, Failure> {
        if !allow_native_reads {
            return Err(Failure {
                code: "markdown_read_permission",
                message: "explicit native-read permission is required; no source opened".into(),
            });
        }
        let captured =
            source::capture_pinned(&self.root, self.identity, self.limits).map_err(|error| {
                Failure {
                    code: error.code(),
                    message: error.to_string(),
                }
            })?;
        Ok(Loaded::Tree {
            captured,
            document: self.document,
        })
    }
}

/// Structural input evidence only; matching bytes grant no publication admission.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Evidence {
    /// Exact identity already verified during capture.
    pub identity: TreeIdentity,
    /// Literal selected regular file within that same capture.
    pub document: EntryPath,
}

/// Native captured bytes or supplied transport bytes; neither variant reopens a source.
pub enum Loaded {
    /// Bytes supplied by the caller or transport.
    Supplied(Vec<u8>),
    /// Complete verified capture and its selected document.
    Tree {
        /// Native immutable in-memory capture.
        captured: CapturedSource,
        /// Exact selection into the capture.
        document: EntryPath,
    },
}

/// Input acquisition or decoding failure, with the original native diagnostic retained.
#[derive(Debug)]
pub struct Failure {
    /// Stable failure classification.
    pub code: &'static str,
    /// Original native diagnostic; transports control its disclosure.
    pub message: String,
}

impl Loaded {
    /// Borrow selected UTF-8 bytes without a live path lookup or a supplied-text fallback.
    ///
    /// # Errors
    /// Refuses a missing/non-file document or invalid UTF-8; parser ceilings apply afterward.
    pub fn text(&self) -> Result<&str, Failure> {
        let bytes = match self {
            Self::Supplied(bytes) => bytes.as_slice(),
            Self::Tree { captured, document } => {
                captured.file(document).ok_or_else(|| Failure {
                    code: "markdown_document",
                    message: format!(
                        "selected path {document:?} is not a regular file in the captured tree"
                    ),
                })?
            }
        };
        std::str::from_utf8(bytes).map_err(|error| Failure {
            code: "markdown_input",
            message: error.to_string(),
        })
    }

    /// Return capture evidence only for a pinned tree, never for supplied text.
    pub fn evidence(&self) -> Option<Evidence> {
        match self {
            Self::Supplied(_) => None,
            Self::Tree { captured, document } => Some(Evidence {
                identity: captured.inventory().identity(),
                document: document.clone(),
            }),
        }
    }
}

/// The same input evidence envelope surrounds native Markdown and framed-document projections.
#[derive(Serialize, schemars::JsonSchema)]
pub struct Report<T> {
    /// Present only after exact complete-tree capture.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<Evidence>,
    /// Complete native syntax result; errors never acquire a partial projection.
    #[serde(flatten)]
    pub projection: T,
}
