//! Complete logical source files: retained document syntax and opaque asset bytes.
//!
//! This phase does not admit publication, infer roles, rewrite source, or render a
//! site. A native Markdown file may be plain or explicitly begin with a TOML fence.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

use gazette_core::source::{EntryPath, Sha256Digest};
use serde::Serialize;

use crate::{document, landing, markdown, notebook, toml};

mod navigation;
pub use navigation::DocumentReference;
mod kind;
pub use kind::SourceKind;

/// Corpus ceilings, including opaque assets; parser allocation overhead is additional.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum source files, including assets.
    #[schemars(range(max = usize::MAX))]
    pub files: usize,
    /// Maximum aggregate exact input bytes before parsing.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Maximum aggregate logical-path bytes before retaining names.
    #[schemars(range(max = usize::MAX))]
    pub path_bytes: usize,
    /// Maximum retained Markdown syntax nodes across the complete corpus.
    #[schemars(range(max = usize::MAX))]
    pub markdown_nodes: usize,
    /// Maximum retained TOML nodes across all frontmatter trees.
    #[schemars(range(max = usize::MAX))]
    pub metadata_nodes: usize,
    /// Maximum retained notebook JSON values across the complete corpus.
    #[schemars(range(max = usize::MAX))]
    pub notebook_nodes: usize,
    /// Additional per-document bounds, independent of corpus ceilings.
    pub document: document::Limits,
    /// Per-notebook bounds, independent of aggregate source and value ceilings.
    pub notebook: notebook::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            files: 4096,
            source_bytes: 16 * 1024 * 1024,
            path_bytes: 1024 * 1024,
            markdown_nodes: 250_000,
            metadata_nodes: 100_000,
            notebook_nodes: 250_000,
            document: document::Limits::default(),
            notebook: notebook::Limits::default(),
        }
    }
}

/// One explicitly supplied logical file, borrowing immutable source bytes.
pub struct Input<'s> {
    /// Source path, not a generated URL or semantic object identity.
    pub path: EntryPath,
    /// Exact file bytes, including frontmatter delimiters when present.
    pub bytes: &'s [u8],
}

/// Whole-corpus failure; no successful subset is returned.
#[derive(Debug)]
pub enum Error {
    /// Duplicate logical input, even when bytes are equal.
    Duplicate(EntryPath),
    /// A syntax selection or retained math result names an absent/inapplicable input.
    Binding(EntryPath),
    /// Aggregate resources exceeded before a successful corpus could be returned.
    Limit(&'static str),
    /// The first rejected aggregate source size, including opaque assets.
    SourceBytesLimit {
        /// Configured maximum before parsing.
        maximum: usize,
        /// Attempted total, absent if adding the next input overflowed.
        attempted: Option<usize>,
    },
    /// First failing file in logical-path order, retaining source-local diagnostics.
    File {
        /// Exact logical input name.
        path: EntryPath,
        /// Syntax or projection failure; opaque assets are not decoded as text.
        error: FileError,
    },
}

/// Document-local failure, without host paths or publication claims.
#[derive(Debug)]
pub enum FileError {
    /// Markdown input was not UTF-8.
    Utf8(std::str::Utf8Error),
    /// Explicit frontmatter document failure, preserving full-document offsets.
    Document(document::Error),
    /// Explicit structured YAML landing failure.
    Landing(landing::Error),
    /// Plain Markdown failure, with coordinates relative to the whole file.
    Markdown(markdown::Error),
    /// Notebook syntax/schema failure; no normalization or rendering occurred.
    Notebook(notebook::Error),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Utf8(error) => error.fmt(f),
            Self::Document(error) => error.fmt(f),
            Self::Landing(error) => error.fmt(f),
            Self::Markdown(error) => error.fmt(f),
            Self::Notebook(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Utf8(error) => Some(error),
            Self::Document(error) => Some(error),
            Self::Landing(error) => Some(error),
            Self::Markdown(error) => Some(error),
            Self::Notebook(error) => Some(error),
        }
    }
}

impl Error {
    /// Stable diagnostic class shared by all callers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Duplicate(_) => "corpus_duplicate",
            Self::Binding(_) => "corpus_binding",
            Self::Limit(_) | Self::SourceBytesLimit { .. } => "corpus_limit",
            Self::File { error, .. } => match error {
                FileError::Utf8(_) => "corpus_utf8",
                FileError::Document(error) => error.code(),
                FileError::Landing(error) => error.code(),
                FileError::Markdown(error) => error.code(),
                FileError::Notebook(error) => error.code(),
            },
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate(path) => write!(f, "duplicate corpus file {path:?}; select it once"),
            Self::Binding(path) => write!(
                f,
                "corpus binding {path:?} has no matching input; resolve the exact current source set"
            ),
            Self::Limit(resource) => write!(
                f,
                "corpus {resource} ceiling exceeded; narrow the input or explicitly raise the ceiling"
            ),
            Self::SourceBytesLimit { maximum, attempted } => {
                write!(
                    f,
                    "aggregate corpus.source_bytes (--max-corpus-bytes) ceiling exceeded: maximum {maximum} bytes"
                )?;
                if let Some(attempted) = attempted {
                    write!(f, ", attempted {attempted} bytes")?;
                }
                f.write_str("; no complete corpus produced")
            }
            Self::File { path, error } => write!(
                f,
                "corpus file {path:?}: {error}; no complete corpus produced"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::File { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Retained syntax or opaque source; none of these variants grants output permission.
/// Parsed payloads are boxed so each opaque asset does not carry their cache footprint.
#[derive(Debug)]
pub enum File<'s> {
    /// Exact first-line `+++` selected the existing strict TOML framing parser.
    Document(Box<document::ParsedDocument<'s>>),
    /// Explicit `.landing.yaml` structured content.
    Landing(Box<landing::ParsedLanding<'s>>),
    /// Plain Markdown without synthesized frontmatter.
    Markdown(Box<markdown::ParsedMarkdown<'s>>),
    /// Structurally validated notebook, not a rendered page or public download.
    Notebook(Box<notebook::ParsedNotebook<'s>>),
    /// Files without a supported syntax profile remain borrowed unchanged.
    Asset(&'s [u8]),
}

impl<'s> File<'s> {
    /// Exact complete file identity, independent of a chosen presentation projection.
    pub fn source_sha256(&self) -> Sha256Digest {
        match self {
            Self::Document(document) => document.source_sha256(),
            Self::Landing(landing) => landing.source_sha256(),
            Self::Markdown(body) => body.source_sha256(),
            Self::Notebook(notebook) => Sha256Digest::of(notebook.source()),
            Self::Asset(bytes) => Sha256Digest::of(bytes),
        }
    }

    /// Start of Markdown body coordinates in complete file bytes; assets have no body.
    pub fn body_offset(&self) -> Option<usize> {
        match self {
            Self::Document(document) => Some(document.body_span().start),
            Self::Markdown(_) => Some(0),
            Self::Landing(_) | Self::Notebook(_) | Self::Asset(_) => None,
        }
    }

    /// Common retained Markdown AST; assets have no Markdown representation.
    pub fn body(&self) -> Option<&markdown::ParsedMarkdown<'s>> {
        match self {
            Self::Document(document) => Some(document.body()),
            Self::Landing(landing) => Some(landing.body()),
            Self::Markdown(body) => Some(body),
            Self::Notebook(_) | Self::Asset(_) => None,
        }
    }

    /// Exact independently parsed fragments; notebook coordinates never become body offsets.
    pub fn math_sources(
        &self,
    ) -> impl Iterator<
        Item = (
            MathLocation,
            gazette_core::markdown::Identity,
            &[gazette_core::markdown::Math],
        ),
    > {
        self.body()
            .into_iter()
            .map(|body| (MathLocation::Body, body.identity(), body.math()))
            .chain(
                match self {
                    Self::Notebook(notebook) => Some(notebook.math_sources()),
                    _ => None,
                }
                .into_iter()
                .flatten(),
            )
    }

    /// Explicit metadata only; plain documentation does not acquire invented fields.
    pub fn metadata(&self) -> Option<&toml::ParsedToml<'s>> {
        match self {
            Self::Document(document) => Some(document.metadata()),
            _ => None,
        }
    }

    /// Original text source. Normalized landing text is available only through `body`.
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Document(document) => Some(document.source()),
            Self::Landing(landing) => Some(landing.source()),
            Self::Markdown(body) => Some(body.source()),
            _ => None,
        }
    }

    /// Map a normalized body construct into the exact retained source.
    pub fn body_location(
        &self,
        location: gazette_core::source::Location,
    ) -> Option<gazette_core::source::Location> {
        if let Self::Landing(landing) = self {
            return landing.original_location(location);
        }
        let offset = self.body_offset()?;
        Some(gazette_core::source::Location {
            span: gazette_core::source::Span::checked(
                self.source()?,
                offset.checked_add(location.span.start)?..offset.checked_add(location.span.end)?,
            )?,
            precision: location.precision,
        })
    }

    fn inspect(&self) -> Result<FileInspection, FileError> {
        match self {
            Self::Document(document) => document
                .inspect()
                .map(FileInspection::Document)
                .map_err(FileError::Document),
            Self::Markdown(body) => body
                .inspect()
                .map(FileInspection::Markdown)
                .map_err(FileError::Markdown),
            Self::Landing(landing) => landing
                .inspect()
                .map(FileInspection::Landing)
                .map_err(FileError::Landing),
            Self::Asset(bytes) => Ok(FileInspection::Asset {
                bytes: bytes.len(),
                sha256: Sha256Digest::of(bytes),
            }),
            Self::Notebook(notebook) => Ok(FileInspection::Notebook(notebook.inspect())),
        }
    }
}

/// Syntax report or asset identity; not a Theme envelope or public search index.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", content = "syntax", rename_all = "kebab-case")]
pub enum FileInspection {
    /// Full-document identity and explicit metadata/body coordinate systems.
    Document(document::Inspection),
    /// Original YAML identity with explicitly normalized body evidence.
    Landing(landing::Inspection),
    /// Body-only evidence for unframed Markdown.
    Markdown(markdown::Inspection),
    /// Exact notebook identity and structural counts without public-use claims.
    Notebook(notebook::Inspection),
    /// Opaque bytes remain visible in the complete source accounting.
    Asset {
        /// Exact byte length.
        bytes: usize,
        /// Exact source digest; no native path or timestamp participates.
        sha256: Sha256Digest,
    },
}

/// Complete path-ordered source evidence; JSON overhead is additional to syntax limits.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Every file, including assets, without inferred publication routes.
    pub files: BTreeMap<EntryPath, FileInspection>,
    /// Aggregate bytes of the supplied files.
    pub source_bytes: usize,
}

/// All supplied regular files, parsed once and retained until their source owner is dropped.
#[derive(Debug)]
pub struct ParsedCorpus<'s> {
    files: BTreeMap<EntryPath, File<'s>>,
    source_bytes: usize,
}

/// Retained typesetting candidates for one exact Markdown body, not publication approval.
#[derive(Clone, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MathResult {
    /// Body bytes and syntax profile from which occurrences were acquired.
    pub identity: gazette_core::markdown::Identity,
    /// Complete ordered emitted occurrences and their generated fragments.
    pub bindings: Vec<gazette_core::markdown::MathBinding>,
}

/// A Markdown fragment's location in its original file, independent from local spans.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum MathLocation {
    /// The document's Markdown body, excluding frontmatter.
    Body,
    /// A notebook Markdown cell, indexed from zero.
    NotebookCell {
        /// Original ordered cell index.
        cell: usize,
    },
    /// A stored text/markdown representation, indexed in its code cell's outputs.
    NotebookOutput {
        /// Original ordered cell index.
        cell: usize,
        /// Original ordered output index.
        output: usize,
    },
}

/// Retained typesetting for one exact independently parsed fragment.
#[derive(Clone, Debug, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocatedMathResult {
    /// Exact source location, not a presentation namespace.
    pub location: MathLocation,
    /// Complete source-bound typesetting candidates.
    pub result: MathResult,
}

/// Supplied typesetting candidates, keyed by exact captured logical source.
/// Decoding grants no source validity or generation claim: `with_math` must bind
/// every occurrence to the current corpus before any formatter consumes it.
pub type RetainedMath = BTreeMap<EntryPath, Vec<LocatedMathResult>>;

/// Decode the generated retained-math data contract without accepting Serde's
/// positional struct form. This validates transport shape only; current source
/// identity and formula admission remain exclusively with `with_math`.
///
/// # Errors
/// Refuses malformed or unknown fields without returning a partial result map.
///
/// # Panics
/// Panics if the schema generated from `RetainedMath` is internally invalid.
pub fn decode_retained_math(value: serde_json::Value) -> Result<RetainedMath, &'static str> {
    static SCHEMA: std::sync::OnceLock<jsonschema::Validator> = std::sync::OnceLock::new();
    let validator = SCHEMA.get_or_init(|| {
        jsonschema::validator_for(schemars::schema_for!(RetainedMath).as_value())
            .expect("native retained-math schema")
    });
    if !validator.is_valid(&value) {
        return Err("retained math must match the complete named native result schema");
    }
    serde_json::from_value(value).map_err(|_| "retained math contains an invalid native value")
}

impl<'s> ParsedCorpus<'s> {
    /// Parse the complete set in path order after checking names and input ceilings.
    ///
    /// `.md`, `.markdown` and `.ipynb` suffixes are ASCII case-insensitive. Notebooks
    /// use the strict notebook profile; other files remain opaque assets. No failed
    /// supported syntax falls back to opaque bytes. An exact first line `+++` selects TOML
    /// frontmatter; a malformed explicitly selected document never falls back to
    /// plain Markdown. Structured YAML requires the explicit `.landing.yaml` or `.landing.yml` suffix; other YAML remains an opaque asset. No source bytes,
    /// paths, roles, lifecycle values or semantic relations are rewritten.
    ///
    /// # Errors
    /// Duplicate names, resource exhaustion or the first path-ordered document
    /// failure reject the whole corpus. No native I/O or publication admission occurs.
    pub fn parse(
        inputs: impl IntoIterator<Item = Input<'s>>,
        limits: Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<Self, Error> {
        Self::parse_with_profiles(inputs, &BTreeMap::new(), limits, markdown)
    }

    /// Parse once with explicit per-file syntax choices. Unselected Markdown uses
    /// the ordinary publication profile; assets and notebooks retain their native parsers.
    ///
    /// # Errors
    /// Rejects unknown selection paths and the same whole-corpus failures as `parse`.
    pub fn parse_with_profiles(
        inputs: impl IntoIterator<Item = Input<'s>>,
        profiles: &BTreeMap<EntryPath, gazette_core::markdown::Profile>,
        limits: Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<Self, Error> {
        let mut selected = BTreeMap::new();
        let mut source_bytes = 0_usize;
        let mut path_bytes = 0_usize;
        for Input { path, bytes } in inputs {
            if selected.contains_key(&path) {
                return Err(Error::Duplicate(path));
            }
            if selected.len() >= limits.files {
                return Err(Error::Limit("files"));
            }
            let attempted = source_bytes.checked_add(bytes.len());
            source_bytes = attempted
                .filter(|&total| total <= limits.source_bytes)
                .ok_or(Error::SourceBytesLimit {
                    maximum: limits.source_bytes,
                    attempted,
                })?;
            charge(
                &mut path_bytes,
                path.as_str().len(),
                limits.path_bytes,
                "path bytes",
            )?;
            selected.insert(path, bytes);
        }
        if let Some(path) = profiles.keys().find(|path| !selected.contains_key(*path)) {
            return Err(Error::Binding(path.clone()));
        }
        let mut files = BTreeMap::new();
        let mut markdown_nodes = 0;
        let mut nodes = 0;
        let mut notebook_nodes = 0;
        for (path, bytes) in selected {
            let mut document_limits = limits.document;
            document_limits.markdown.nodes = document_limits
                .markdown
                .nodes
                .min(limits.markdown_nodes - markdown_nodes);
            document_limits.metadata.nodes = document_limits
                .metadata
                .nodes
                .min(limits.metadata_nodes - nodes);
            let mut notebook_limits = limits.notebook;
            notebook_limits.nodes = notebook_limits
                .nodes
                .min(limits.notebook_nodes - notebook_nodes);
            let profile = profiles.get(&path).copied().unwrap_or_default();
            let file = parse_file(
                &path,
                bytes,
                profile,
                document_limits,
                notebook_limits,
                markdown,
            )
            .map_err(|error| Error::File {
                path: path.clone(),
                error,
            })?;
            markdown_nodes += match &file {
                File::Notebook(notebook) => notebook.markdown_node_count(),
                _ => file.body().map_or(0, markdown::ParsedMarkdown::node_count),
            };
            nodes += match &file {
                File::Landing(landing) => landing.node_count(),
                _ => file.metadata().map_or(0, toml::ParsedToml::node_count),
            };
            if let File::Notebook(notebook) = &file {
                notebook_nodes += notebook.node_count();
            }
            files.insert(path, file);
        }
        Ok(Self {
            files,
            source_bytes,
        })
    }

    /// Every retained file in logical-path order, for subsequent typed resolution.
    pub fn files(&self) -> &BTreeMap<EntryPath, File<'s>> {
        &self.files
    }

    /// Attach one complete retained math result set before semantic resolution.
    /// Ownership is consumed: a late mismatch returns no partially rebound corpus.
    /// Fragments still undergo native structural validation before formatting.
    ///
    /// # Errors
    /// Rejects missing/extra results, assets, stale source/profile, changed occurrences
    /// or exceeded per-body ceilings. A source without emitted math needs no result.
    pub fn with_math(
        mut self,
        mut results: BTreeMap<EntryPath, Vec<LocatedMathResult>>,
    ) -> Result<Self, Error> {
        for (path, file) in &self.files {
            let needed = file
                .math_sources()
                .any(|(_, _, facts)| facts.iter().any(|fact| fact.emitted));
            if needed != results.contains_key(path) {
                return Err(Error::Binding(path.clone()));
            }
            if let Some(results) = results.get(path) {
                let expected: BTreeSet<_> = file
                    .math_sources()
                    .filter(|(_, _, facts)| facts.iter().any(|fact| fact.emitted))
                    .map(|(location, _, _)| location)
                    .collect();
                let actual: BTreeSet<_> = results.iter().map(|result| result.location).collect();
                if actual != expected || actual.len() != results.len() {
                    return Err(Error::Binding(path.clone()));
                }
            }
        }
        if let Some(path) = results.keys().find(|path| !self.files.contains_key(*path)) {
            return Err(Error::Binding(path.clone()));
        }
        self.files = self
            .files
            .into_iter()
            .map(|(path, file)| {
                let Some(mut results) = results.remove(&path) else {
                    return Ok((path, file));
                };
                let rebound = match file {
                    File::Document(document) => {
                        let result = results.remove(0).result;
                        (*document)
                            .with_math(result.identity, result.bindings)
                            .map(|document| File::Document(Box::new(document)))
                            .map_err(FileError::Document)
                    }
                    File::Landing(landing) => {
                        let result = results.remove(0).result;
                        (*landing)
                            .with_math(result.identity, result.bindings)
                            .map(|landing| File::Landing(Box::new(landing)))
                            .map_err(FileError::Landing)
                    }
                    File::Markdown(body) => {
                        let result = results.remove(0).result;
                        (*body)
                            .with_math(result.identity, result.bindings)
                            .map(|body| File::Markdown(Box::new(body)))
                            .map_err(FileError::Markdown)
                    }
                    File::Notebook(notebook) => (*notebook)
                        .with_math(results)
                        .map(|notebook| File::Notebook(Box::new(notebook)))
                        .map_err(FileError::Notebook),
                    File::Asset(_) => return Err(Error::Binding(path)),
                }
                .map_err(|error| Error::File {
                    path: path.clone(),
                    error,
                })?;
                Ok((path, rebound))
            })
            .collect::<Result<_, _>>()?;
        Ok(self)
    }

    /// Project syntax evidence from the retained trees, with no second parse.
    ///
    /// # Errors
    /// Projection exhaustion returns no successful partial report.
    pub fn inspect(&self) -> Result<Inspection, Error> {
        let files = self
            .files
            .iter()
            .map(|(path, file)| {
                file.inspect()
                    .map(|syntax| (path.clone(), syntax))
                    .map_err(|error| Error::File {
                        path: path.clone(),
                        error,
                    })
            })
            .collect::<Result<_, _>>()?;
        Ok(Inspection {
            files,
            source_bytes: self.source_bytes,
        })
    }
}

fn parse_file<'s>(
    path: &EntryPath,
    bytes: &'s [u8],
    profile: gazette_core::markdown::Profile,
    limits: document::Limits,
    notebook_limits: notebook::Limits,
    markdown: &dyn gazette_ports::markdown::Markdown,
) -> Result<File<'s>, FileError> {
    match SourceKind::for_path(path) {
        SourceKind::Landing => {
            let source = std::str::from_utf8(bytes).map_err(FileError::Utf8)?;
            return landing::ParsedLanding::parse_with_profile(source, profile, limits, markdown)
                .map(|landing| File::Landing(Box::new(landing)))
                .map_err(FileError::Landing);
        }
        SourceKind::Notebook => {
            return notebook::ParsedNotebook::parse_with_profile(
                bytes,
                profile,
                notebook_limits,
                limits.markdown,
                markdown,
            )
            .map(|notebook| File::Notebook(Box::new(notebook)))
            .map_err(FileError::Notebook);
        }
        SourceKind::Asset => return Ok(File::Asset(bytes)),
        SourceKind::Markdown => {}
    }
    if bytes.len() > limits.source_bytes {
        return Err(FileError::Document(document::Error::SourceLimit));
    }
    let source = std::str::from_utf8(bytes).map_err(FileError::Utf8)?;
    if source == "+++" || source.starts_with("+++\n") || source.starts_with("+++\r\n") {
        document::ParsedDocument::parse_with_profile(source, profile, limits, markdown)
            .map(|document| File::Document(Box::new(document)))
            .map_err(FileError::Document)
    } else {
        markdown::ParsedMarkdown::parse_with_profile(source, profile, limits.markdown, markdown)
            .map(|body| File::Markdown(Box::new(body)))
            .map_err(FileError::Markdown)
    }
}

fn charge(
    used: &mut usize,
    amount: usize,
    ceiling: usize,
    resource: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&total| total <= ceiling)
        .ok_or(Error::Limit(resource))?;
    Ok(())
}
