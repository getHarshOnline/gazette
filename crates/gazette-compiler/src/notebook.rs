//! Retained notebook syntax and explicit static projection, without execution or public admission.

use std::{collections::BTreeSet, fmt, sync::OnceLock};

use gazette_core::{json, source::Sha256Digest};
use serde::Serialize;
use serde_json::Value;

mod attachments;
mod html;
mod math;
mod raster;
mod render;
mod scan;
pub use render::{NotebookCell, RenderError, RenderedNotebook};
pub use scan::{ScanClass, ScanFinding};

const SCHEMA: &str = include_str!("notebook/nbformat.v4.5.schema.json");

/// Per-notebook bounds. JSON allocation overhead is additional to exact source bytes.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Exact input bytes checked before decoding.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Total JSON values checked before schema validation.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Cells checked before schema validation and identity indexing.
    #[schemars(range(max = usize::MAX))]
    pub cells: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1024 * 1024,
            nodes: 100_000,
            cells: 4096,
        }
    }
}

/// Structural failure. Diagnostics never echo notebook content or metadata values.
#[derive(Debug)]
pub enum Error {
    /// A bounded cell/output Markdown parse or exact typesetting binding failed.
    Markdown(crate::markdown::Error),
    /// Input or retained structure exceeds its explicit ceiling.
    Limit(&'static str),
    /// Syntax failure; only parser-local coordinates are reported, never instance values.
    Json {
        /// One-based line within the failing decoder slice.
        line: usize,
        /// Source column supplied by the JSON parser.
        column: usize,
    },
    /// This parser supports exactly notebook format 4.5, with no version conversion.
    Version,
    /// The bundled schema could not be compiled; no input is accepted.
    Schema,
    /// The instance does not conform to the bundled notebook schema.
    Structure,
    /// A cell repeats a previously declared ID; no replacement ID is generated.
    DuplicateId(usize),
    /// A cell repeats an explicitly supplied metadata name.
    DuplicateName(usize),
}

impl Error {
    /// Stable diagnostic class for SDK and interface consumers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Markdown(error) => error.code(),
            Self::Limit(_) => "notebook_limit",
            Self::Json { .. } => "notebook_json",
            Self::Version => "notebook_version",
            Self::Schema => "notebook_schema",
            Self::Structure => "notebook_structure",
            Self::DuplicateId(_) => "notebook_duplicate_id",
            Self::DuplicateName(_) => "notebook_duplicate_name",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Markdown(error) => write!(f, "notebook Markdown: {error}"),
            Self::Limit(resource) => write!(f, "notebook {resource} ceiling exceeded; narrow the input or explicitly raise the ceiling"),
            Self::Json { line, column } => write!(f, "invalid or ambiguous notebook JSON at decoder-local {line}:{column}; correct the source"),
            Self::Version => f.write_str("unsupported notebook version; this parser requires exactly nbformat 4.5 and performs no conversion"),
            Self::Schema => f.write_str("bundled notebook schema is unavailable; no notebook accepted"),
            Self::Structure => f.write_str("notebook does not conform to the bundled nbformat 4.5 schema; correct its structure"),
            Self::DuplicateId(cell) => write!(f, "notebook cell {cell} repeats an ID; correct the source without automatic repair"),
            Self::DuplicateName(cell) => write!(f, "notebook cell {cell} repeats a metadata name; correct the source"),
        }
    }
}
impl std::error::Error for Error {}

/// Exact source and its validated JSON tree. Notebook metadata grants no publication role.
pub struct ParsedNotebook<'s> {
    source: &'s [u8],
    value: Value,
    nodes: usize,
    profile: gazette_core::markdown::Profile,
    markdown_limits: crate::markdown::Limits,
    markdown_nodes: usize,
    fragments: std::collections::BTreeMap<crate::corpus::MathLocation, math::Fragment>,
}

impl fmt::Debug for ParsedNotebook<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inspect().fmt(f)
    }
}

/// Bounded structural evidence, not a public page or a renderer-availability claim.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact original byte count; source arrays and strings are not normalized.
    pub bytes: usize,
    /// Identity of the original source, not a reserialized JSON tree.
    pub sha256: Sha256Digest,
    /// Identity of the exact bundled schema used by this parser.
    pub schema_sha256: Sha256Digest,
    /// Number of validated ordered cells.
    pub cells: usize,
    /// Total retained JSON values, including metadata, outputs and attachments.
    pub nodes: usize,
}

impl<'s> ParsedNotebook<'s> {
    /// Parse exact nbformat 4.5 using its upstream schema and explicit cell uniqueness.
    ///
    /// Preserves cell order, metadata, outputs and attachments in one JSON tree.
    /// Object members are sorted once for presentation; arrays retain their order
    /// and original source bytes remain untouched. No kernels, downloads or native I/O occur.
    /// The shared decoder bounds nesting; input bytes are bounded before allocation,
    /// then total values and cells before the fixed schema is evaluated.
    ///
    /// # Errors
    /// Malformed, ambiguous, unsupported, oversized or nonconforming inputs fail
    /// completely. No IDs are generated and no older/future format is converted.
    pub fn parse(source: &'s [u8], limits: Limits) -> Result<Self, Error> {
        if source.len() > limits.source_bytes {
            return Err(Error::Limit("source bytes"));
        }
        let mut value = json::parse(source).map_err(|e| Error::Json {
            line: e.line(),
            column: e.column(),
        })?;
        let mut remaining = limits.nodes;
        count_nodes(&value, &mut remaining)?;
        value.sort_all_objects();
        if value.get("nbformat").and_then(Value::as_u64) != Some(4)
            || value.get("nbformat_minor").and_then(Value::as_u64) != Some(5)
        {
            return Err(Error::Version);
        }
        let cells = value
            .get("cells")
            .and_then(Value::as_array)
            .ok_or(Error::Structure)?;
        if cells.len() > limits.cells {
            return Err(Error::Limit("cells"));
        }
        if !validator()?.is_valid(&value) {
            return Err(Error::Structure);
        }
        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        for (index, cell) in cells.iter().enumerate() {
            let id = cell
                .get("id")
                .and_then(Value::as_str)
                .ok_or(Error::Structure)?;
            if !ids.insert(id) {
                return Err(Error::DuplicateId(index));
            }
            if let Some(name) = cell
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(Value::as_str)
                && !names.insert(name)
            {
                return Err(Error::DuplicateName(index));
            }
        }
        Ok(Self {
            source,
            value,
            nodes: limits.nodes - remaining,
            profile: Default::default(),
            markdown_limits: Default::default(),
            markdown_nodes: 0,
            fragments: Default::default(),
        })
    }

    /// Original bytes, including every stored output and attachment; not a download grant.
    pub fn source(&self) -> &'s [u8] {
        self.source
    }

    /// Immutable validated JSON for a later explicitly selected renderer/inspection.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Total retained values for shared corpus accounting.
    pub fn node_count(&self) -> usize {
        self.nodes
    }

    /// Project identities/counts without copying notebook contents into the report.
    pub fn inspect(&self) -> Inspection {
        Inspection {
            bytes: self.source.len(),
            sha256: Sha256Digest::of(self.source),
            schema_sha256: Sha256Digest::of(SCHEMA.as_bytes()),
            cells: self
                .value
                .get("cells")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
            nodes: self.nodes,
        }
    }
}

fn count_nodes(value: &Value, remaining: &mut usize) -> Result<(), Error> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or(Error::Limit("JSON values"))?;
    match value {
        Value::Array(values) => {
            for child in values {
                count_nodes(child, remaining)?;
            }
        }
        Value::Object(values) => {
            for child in values.values() {
                count_nodes(child, remaining)?;
            }
        }
        _ => (),
    }
    Ok(())
}

fn validator() -> Result<&'static jsonschema::Validator, Error> {
    static VALIDATOR: OnceLock<Result<jsonschema::Validator, ()>> = OnceLock::new();
    VALIDATOR
        .get_or_init(|| {
            let schema: Value = serde_json::from_str(SCHEMA).map_err(|_| ())?;
            jsonschema::draft4::options()
                .with_retriever(crate::schema::DenyExternal)
                .build(&schema)
                .map_err(|_| ())
        })
        .as_ref()
        .map_err(|_| Error::Schema)
}
