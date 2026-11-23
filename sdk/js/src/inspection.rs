//! Distinct supplied-text and pinned-tree inputs; the Rust SDK owns acquisition and inspection.

use gazette_sdk::{document, markdown, toml};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;

use crate::{Limits, encode, integer, query, transport, utf8};

/// Optional native Markdown ceilings; every omitted field uses the SDK default.
#[napi(object)]
#[derive(Default)]
pub struct MarkdownLimits {
    /// Maximum source UTF-8 bytes before copying or parsing.
    pub source_bytes: Option<f64>,
    /// Maximum syntax nodes, including the root.
    pub nodes: Option<f64>,
    /// Maximum syntax nesting below the root.
    pub depth: Option<f64>,
    /// Maximum retained analysis string bytes.
    pub fact_bytes: Option<f64>,
    /// Independent inspection, plan and HTML projection byte ceiling.
    pub output_bytes: Option<f64>,
}

impl MarkdownLimits {
    fn native(self) -> Result<markdown::Limits> {
        let defaults = markdown::Limits::default();
        Ok(markdown::Limits {
            source_bytes: integer(self.source_bytes, defaults.source_bytes)?,
            nodes: integer(self.nodes, defaults.nodes)?,
            depth: integer(self.depth, defaults.depth)?,
            fact_bytes: integer(self.fact_bytes, defaults.fact_bytes)?,
            output_bytes: integer(self.output_bytes, defaults.output_bytes)?,
        })
    }
}

/// Optional native TOML metadata ceilings, independent of the Markdown body.
#[napi(object)]
#[derive(Default)]
pub struct MetadataLimits {
    /// Maximum metadata bytes, excluding framing delimiters.
    pub source_bytes: Option<f64>,
    /// Maximum metadata syntax nodes.
    pub nodes: Option<f64>,
    /// Maximum metadata nesting below its root.
    pub depth: Option<f64>,
}

impl MetadataLimits {
    fn native(self) -> Result<toml::Limits> {
        let defaults = toml::Limits::default();
        Ok(toml::Limits {
            source_bytes: integer(self.source_bytes, defaults.source_bytes)?,
            nodes: integer(self.nodes, defaults.nodes)?,
            depth: integer(self.depth, defaults.depth)?,
        })
    }
}

/// Body-only Markdown inspection; framing is never inferred from input text.
#[napi(object)]
#[derive(Default)]
pub struct MarkdownOptions {
    /// Include the SDK's bounded HTML fragment; defaults to false.
    pub html: Option<bool>,
    /// Independent native syntax and projection ceilings.
    pub limits: Option<MarkdownLimits>,
    /// Maximum encoded result or classified failure bytes; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
}

impl MarkdownOptions {
    fn native(self) -> Result<(markdown::Limits, bool, usize)> {
        Ok((
            self.limits.unwrap_or_default().native()?,
            self.html.unwrap_or(false),
            integer(self.result_bytes, 1 << 20)?,
        ))
    }
}

/// Explicitly framed TOML document inspection; missing delimiters are errors.
#[napi(object)]
#[derive(Default)]
pub struct DocumentOptions {
    /// Include bounded body HTML; any failure prevents the entire report.
    pub html: Option<bool>,
    /// Maximum complete document UTF-8 bytes, including its delimiters.
    pub source_bytes: Option<f64>,
    /// Native metadata ceilings.
    pub metadata: Option<MetadataLimits>,
    /// Native body ceilings, independent of the complete document ceiling.
    pub markdown: Option<MarkdownLimits>,
    /// Maximum encoded result or classified failure bytes; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
}

impl DocumentOptions {
    fn native(self) -> Result<(document::Limits, bool, usize)> {
        Ok((
            document::Limits {
                source_bytes: integer(self.source_bytes, document::Limits::default().source_bytes)?,
                metadata: self.metadata.unwrap_or_default().native()?,
                markdown: self.markdown.unwrap_or_default().native()?,
            },
            self.html.unwrap_or(false),
            integer(self.result_bytes, 1 << 20)?,
        ))
    }
}

/// Independent native-read permission and bounded exact tree-selection JSON.
#[napi(object)]
pub struct TreeOptions {
    /// Must be true before native tree acquisition; never inferred from a valid pin.
    pub allow_read: bool,
    /// Maximum selection JSON bytes before native decoding; defaults to 16 KiB.
    pub query_bytes: Option<f64>,
}

fn load_tree(json: &str, options: TreeOptions, maximum: usize) -> Result<markdown::input::Loaded> {
    if !options.allow_read {
        return Err(classified("markdown_read_permission", maximum));
    }
    let bounds = Limits {
        syntax: toml::Limits::default(),
        query: integer(options.query_bytes, 16 * 1024)?,
        result: maximum,
    };
    let value: serde_json::Value = query(json, &bounds)?;
    // Native types own fields and limits. This host admits named JSON maps only,
    // not Serde's positional struct representation at nested native boundaries.
    for path in ["/identity", "/limits"] {
        if value.pointer(path).is_some_and(|value| !value.is_object()) {
            return Err(transport("node_query"));
        }
    }
    let tree: markdown::input::Tree =
        serde_json::from_value(value).map_err(|_| transport("node_query"))?;
    if !tree.root.is_absolute() {
        return Err(transport("node_query"));
    }
    tree.load(options.allow_read)
        .map_err(|error| classified(error.code, maximum))
}

fn classified(code: &'static str, maximum: usize) -> Error {
    match encode(&serde_json::json!({"code":code}), maximum) {
        Ok(value) => Error::new(Status::InvalidArg, value),
        Err(error) => error,
    }
}

/// Inspect exact supplied UTF-8 Markdown using the shared SDK; no native source reads.
///
/// # Errors
/// Rejects host limits/encoding and native parse/inspection/HTML failures without
/// returning a partial report. Native failures expose their class, not source text.
#[napi]
pub fn inspect_markdown(bytes: Uint8Array, options: Option<MarkdownOptions>) -> Result<String> {
    let (limits, html, maximum) = options.unwrap_or_default().native()?;
    if bytes.len() > limits.source_bytes {
        return Err(classified(
            markdown::Error::Limit("source bytes").code(),
            maximum,
        ));
    }
    let source = utf8(&bytes)?;
    let report = markdown::inspect(&source, limits, html)
        .map_err(|error| classified(error.code(), maximum))?;
    encode(&report, maximum)
}

/// Inspect an explicitly TOML-framed UTF-8 document through the shared SDK.
///
/// # Errors
/// Missing/malformed framing, metadata, body or requested HTML fails the complete
/// operation. No extension guessing, source lookup or partial fallback occurs.
#[napi]
pub fn inspect_document(bytes: Uint8Array, options: Option<DocumentOptions>) -> Result<String> {
    let (limits, html, maximum) = options.unwrap_or_default().native()?;
    if bytes.len() > limits.source_bytes {
        return Err(classified(document::Error::SourceLimit.code(), maximum));
    }
    let source = utf8(&bytes)?;
    let report = document::inspect(&source, limits, html)
        .map_err(|error| classified(error.code(), maximum))?;
    encode(&report, maximum)
}

/// Inspect a document in an independently pinned complete tree; include its original input evidence.
/// The selection is the Rust SDK's exact `markdown::input::Tree` JSON, not supplied text.
///
/// # Errors
/// Refuses missing read permission, invalid selection, pin drift, capture/parse/output limits,
/// non-file or invalid UTF-8 content. No supplied fallback, second read or partial result.
#[napi]
pub fn inspect_markdown_tree(
    tree_json: String,
    tree_options: TreeOptions,
    options: Option<MarkdownOptions>,
) -> Result<String> {
    let (limits, html, maximum) = options.unwrap_or_default().native()?;
    let loaded = load_tree(&tree_json, tree_options, maximum)?;
    let text = loaded
        .text()
        .map_err(|error| classified(error.code, maximum))?;
    let projection =
        markdown::inspect(text, limits, html).map_err(|error| classified(error.code(), maximum))?;
    encode(
        &markdown::input::Report {
            input: loaded.evidence(),
            projection,
        },
        maximum,
    )
}

/// Inspect explicit TOML-framed content in an independently pinned complete tree.
/// All capture evidence and source ranges come from the same retained native bytes.
///
/// # Errors
/// Refuses missing permission, invalid selection, capture/pin/UTF-8/framing failures or
/// exhausted independent bounds without returning a partial document or reopening a path.
#[napi]
pub fn inspect_document_tree(
    tree_json: String,
    tree_options: TreeOptions,
    options: Option<DocumentOptions>,
) -> Result<String> {
    let (limits, html, maximum) = options.unwrap_or_default().native()?;
    let loaded = load_tree(&tree_json, tree_options, maximum)?;
    let text = loaded
        .text()
        .map_err(|error| classified(error.code, maximum))?;
    let projection =
        document::inspect(text, limits, html).map_err(|error| classified(error.code(), maximum))?;
    encode(
        &markdown::input::Report {
            input: loaded.evidence(),
            projection,
        },
        maximum,
    )
}
