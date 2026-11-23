//! Complete, explicitly supplied TOML source sets without discovery or semantic merging.

use std::collections::BTreeMap;
use std::fmt;

use gazette_core::source::EntryPath;
use serde::Serialize;

use super::{Error, Inspection, Limits, ParsedToml};

/// Whole-set logical ceilings, additional to each document's parser limits.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SetLimits {
    /// Maximum distinct documents; duplicate paths always fail.
    #[schemars(range(max = usize::MAX))]
    pub documents: usize,
    /// Maximum aggregate source bytes before parsing any document.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Maximum aggregate path bytes before retaining cloned names.
    #[schemars(range(max = usize::MAX))]
    pub path_bytes: usize,
    /// Maximum aggregate syntax nodes, including every document root.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Independent per-document source, node and depth ceilings.
    pub document: Limits,
}

impl Default for SetLimits {
    fn default() -> Self {
        Self {
            documents: 512,
            source_bytes: 4 * 1024 * 1024,
            path_bytes: 64 * 1024,
            nodes: 100_000,
            document: Limits::default(),
        }
    }
}

/// Failure of the complete set; no successfully parsed subset is returned.
#[derive(Debug)]
pub enum SetError {
    /// The same logical path appeared more than once, even with identical bytes.
    Duplicate(EntryPath),
    /// An aggregate resource ceiling was exhausted.
    Limit(&'static str),
    /// Document failures in path order; resource exhaustion stops collection immediately.
    Documents(Vec<DocumentError>),
}

/// One source-local failure, preserving its logical name and parser coordinates.
#[derive(Debug)]
pub enum DocumentError {
    /// A selected document is not valid UTF-8.
    Utf8 {
        /// Exact logical source name, never an inferred host path.
        path: EntryPath,
        /// Length of the valid UTF-8 prefix, in bytes.
        valid_up_to: usize,
    },
    /// A selected document failed the shared TOML parser or its limits.
    Toml {
        /// Exact logical source name.
        path: EntryPath,
        /// Original TOML failure, with document-local ranges.
        error: Error,
    },
}

impl DocumentError {
    /// Stable diagnostic class, preserving the underlying TOML class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Utf8 { .. } => "toml_set_utf8",
            Self::Toml { error, .. } => error.code(),
        }
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8 { path, valid_up_to } => {
                write!(f, "TOML source {path:?} is not UTF-8 at byte {valid_up_to}")
            }
            Self::Toml { path, error } => write!(f, "TOML source {path:?}: {error}"),
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Toml { error, .. } => Some(error),
            _ => None,
        }
    }
}

impl SetError {
    /// Stable class; a single document retains its original failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Duplicate(_) => "toml_set_duplicate",
            Self::Limit(_) => "toml_set_limit",
            Self::Documents(errors) => match errors.as_slice() {
                [error] => error.code(),
                _ => "toml_set_documents",
            },
        }
    }
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(path) => write!(f, "duplicate TOML source {path:?}; select it once"),
            Self::Limit(resource) => write!(f, "TOML set {resource} ceiling exceeded"),
            Self::Documents(errors) => {
                if let [error] = errors.as_slice() {
                    return error.fmt(f);
                }
                write!(f, "{} TOML sources failed", errors.len())?;
                for error in errors {
                    write!(f, "\n  {}: {error}", error.code())?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Documents(errors) => match errors.as_slice() {
                [error] => Some(error),
                _ => None,
            },
            _ => None,
        }
    }
}

/// One explicitly supplied source's structural evidence.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct NamedInspection {
    /// Exact logical path, with no extension-based selection or normalization.
    pub path: EntryPath,
    /// Byte identity, root keys and node count, not semantic admission.
    pub syntax: Inspection,
}

/// The complete source set in path order; no host roots or wall clocks are included.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct SetInspection {
    /// Every supplied document, including hidden and empty documents.
    pub documents: Vec<NamedInspection>,
    /// Sum of the exact supplied document byte lengths.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Sum of all syntax nodes, including each root.
    #[schemars(range(max = usize::MAX))]
    pub node_count: usize,
}

/// Immutable complete syntax set, not a resolved workspace or admitted desired state.
///
/// Sources keep their separate namespaces. There is no import discovery, field
/// merging, extension filtering, reference resolution or fallback to an older set.
/// Unknown metadata remains available to each caller's explicit typed schema.
#[derive(Debug)]
pub struct ParsedSet<'s> {
    documents: BTreeMap<EntryPath, ParsedToml<'s>>,
    source_bytes: usize,
    node_count: usize,
}

impl<'s> ParsedSet<'s> {
    /// Validate all selected source sizes/names, then parse every document in path order.
    ///
    /// An empty set is valid syntax evidence, not proof of a valid workspace.
    /// Input bytes are borrowed unchanged; no native reads or writes occur.
    ///
    /// # Errors
    ///
    /// Rejects duplicate paths, invalid UTF-8, syntax errors and exhausted bounds.
    /// Independent UTF-8 and syntax failures are collected in path order, at most
    /// one per document. Duplicate names and resource exhaustion stop immediately;
    /// they do not promise complete diagnostics. A failure drops all retained
    /// parses; there is no partial successful set. Source/path/document ceilings
    /// also bound the diagnostic collection; parser message overhead is additional.
    pub fn parse<'p>(
        sources: impl IntoIterator<Item = (&'p EntryPath, &'s [u8])>,
        limits: SetLimits,
    ) -> Result<Self, SetError> {
        let mut selected = BTreeMap::new();
        let mut source_bytes = 0_usize;
        let mut path_bytes = 0_usize;
        for (path, bytes) in sources {
            if selected.contains_key(path) {
                return Err(SetError::Duplicate(path.clone()));
            }
            if selected.len() >= limits.documents {
                return Err(SetError::Limit("documents"));
            }
            source_bytes = charge(
                source_bytes,
                bytes.len(),
                limits.source_bytes,
                "source bytes",
            )?;
            path_bytes = charge(
                path_bytes,
                path.as_str().len(),
                limits.path_bytes,
                "path bytes",
            )?;
            // The per-document byte ceiling also precedes every parser invocation.
            if bytes.len() > limits.document.source_bytes {
                return Err(SetError::Documents(vec![DocumentError::Toml {
                    path: path.clone(),
                    error: Error::Limit("source bytes"),
                }]));
            }
            selected.insert(path.clone(), bytes);
        }
        let mut documents = BTreeMap::new();
        let mut node_count = 0;
        let mut errors = Vec::new();
        for (path, bytes) in selected {
            let document = std::str::from_utf8(bytes)
                .map_err(|error| DocumentError::Utf8 {
                    path: path.clone(),
                    valid_up_to: error.valid_up_to(),
                })
                .and_then(|source| {
                    ParsedToml::parse(source, limits.document).map_err(|error| {
                        DocumentError::Toml {
                            path: path.clone(),
                            error,
                        }
                    })
                });
            match document {
                Ok(document) => {
                    node_count = charge(node_count, document.node_count, limits.nodes, "nodes")?;
                    documents.insert(path, document);
                }
                Err(
                    error @ DocumentError::Toml {
                        error: Error::Limit(_),
                        ..
                    },
                ) => {
                    return Err(SetError::Documents(vec![error]));
                }
                Err(error) => errors.push(error),
            }
        }
        if !errors.is_empty() {
            return Err(SetError::Documents(errors));
        }
        Ok(Self {
            documents,
            source_bytes,
            node_count,
        })
    }

    /// Borrow every retained document for caller-owned typed consumption, in path order.
    pub fn documents(&self) -> impl ExactSizeIterator<Item = (&EntryPath, &ParsedToml<'s>)> {
        self.documents.iter()
    }

    /// Borrow one exact selected document; no path fallback or normalization is applied.
    pub(crate) fn document(&self, path: &EntryPath) -> Option<&ParsedToml<'s>> {
        self.documents.get(path)
    }

    /// Derive complete structural evidence without emitting metadata values.
    pub fn inspect(&self) -> SetInspection {
        SetInspection {
            documents: self
                .documents
                .iter()
                .map(|(path, document)| NamedInspection {
                    path: path.clone(),
                    syntax: document.inspect(),
                })
                .collect(),
            source_bytes: self.source_bytes,
            node_count: self.node_count,
        }
    }
}

fn charge(
    used: usize,
    amount: usize,
    ceiling: usize,
    resource: &'static str,
) -> Result<usize, SetError> {
    used.checked_add(amount)
        .filter(|&total| total <= ceiling)
        .ok_or(SetError::Limit(resource))
}
