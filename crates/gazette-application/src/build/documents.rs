//! Pinned source bytes enter the pure corpus compiler without a second native read.

use std::fmt;

use gazette_compiler::corpus;
use gazette_core::source::{self, Content, EntryPath};
use serde::Serialize;

use super::{AcquiredSite, Inspection};

/// Source mapping or syntax failure, never a successfully admitted subset.
#[derive(Debug)]
pub enum DocumentError {
    /// A composed logical source path violated the shared literal-path contract.
    Path(source::Error),
    /// The retained inventory and its payload are inconsistent.
    MissingPayload(EntryPath),
    /// Whole-corpus syntax or projection failure with logical file evidence.
    Corpus(corpus::Error),
}

impl DocumentError {
    /// Stable diagnostic class, shared by SDK and interfaces.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Path(error) => error.code(),
            Self::MissingPayload(_) => "build_missing_payload",
            Self::Corpus(error) => error.code(),
        }
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Path(error) => error.fmt(f),
            Self::MissingPayload(path) => write!(
                f,
                "retained payload for {path:?} is missing; no parsed site produced"
            ),
            Self::Corpus(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for DocumentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Path(error) => Some(error),
            Self::Corpus(error) => Some(error),
            Self::MissingPayload(_) => None,
        }
    }
}

/// A complete selected-site documentation syntax phase, borrowing its exact acquired input.
///
/// Private construction keeps the workspace/source evidence and corpus together.
/// This is not the admitted publication model, a rendered site or certification.
#[derive(Debug)]
pub struct ParsedSite<'s> {
    acquired: &'s AcquiredSite,
    corpus: corpus::ParsedCorpus<'s>,
}

impl ParsedSite<'_> {
    /// Exact acquisition and complete workspace declaration used by this syntax phase.
    pub fn acquired(&self) -> &AcquiredSite {
        self.acquired
    }

    /// Complete retained file syntax/assets for the next semantic-resolution phase.
    pub fn corpus(&self) -> &corpus::ParsedCorpus<'_> {
        &self.corpus
    }

    /// Derive operator evidence from the same source bindings and retained syntax trees.
    ///
    /// # Errors
    /// Propagates corpus projection failure without a partial report.
    pub fn inspect(&self) -> Result<DocumentInspection<'_>, DocumentError> {
        Ok(DocumentInspection {
            acquisition: self.acquired.inspect(),
            corpus: self.corpus.inspect().map_err(DocumentError::Corpus)?,
        })
    }
}

/// Source-bound operator report; native roots and unadmitted text prohibit public reuse.
#[derive(Serialize, schemars::JsonSchema)]
pub struct DocumentInspection<'a> {
    /// Complete configuration, selected site and exact pin evidence.
    pub acquisition: Inspection<'a>,
    /// All selected files, including opaque assets; not a publication search index.
    pub corpus: corpus::Inspection,
}

impl AcquiredSite {
    /// Parse every retained documentation file and account for all accompanying assets.
    ///
    /// Logical paths are placement target plus literal source-relative path, not
    /// URLs or content identity. Empty directories remain in acquisition evidence.
    /// File classification and syntax belong to the compiler, not this mapping.
    /// No source is reopened, no writes occur and no metadata is synthesized.
    ///
    /// # Errors
    /// Rejects an inconsistent payload, invalid logical path or any corpus failure.
    pub fn parse_documents(
        &self,
        limits: corpus::Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<ParsedSite<'_>, DocumentError> {
        let mut inputs = Vec::new();
        let mut profiles = std::collections::BTreeMap::new();
        let mut path_bytes = 0_usize;
        for placement in self.sources().placements() {
            let source = placement.source();
            for entry in source.inventory().entries() {
                if matches!(entry.content, Content::Directory) {
                    continue;
                }
                // Bound the mapping allocation as well as the compiler's retained set.
                if inputs.len() >= limits.files {
                    return Err(DocumentError::Corpus(corpus::Error::Limit("files")));
                }
                path_bytes = path_bytes
                    .checked_add(placement.request().target.as_str().len())
                    .and_then(|used| used.checked_add(1))
                    .and_then(|used| used.checked_add(entry.path.as_str().len()))
                    .filter(|&used| used <= limits.path_bytes)
                    .ok_or(DocumentError::Corpus(corpus::Error::Limit("path bytes")))?;
                let path: EntryPath = format!("{}/{}", placement.request().target, entry.path)
                    .parse()
                    .map_err(DocumentError::Path)?;
                let bytes = source
                    .file(&entry.path)
                    .ok_or_else(|| DocumentError::MissingPayload(entry.path.clone()))?;
                profiles.insert(
                    path.clone(),
                    self.compilation().workspace.markdown_profile(&path),
                );
                inputs.push(corpus::Input { path, bytes });
            }
        }
        let corpus = corpus::ParsedCorpus::parse_with_profiles(inputs, &profiles, limits, markdown)
            .map_err(DocumentError::Corpus)?;
        Ok(ParsedSite {
            acquired: self,
            corpus,
        })
    }
}
