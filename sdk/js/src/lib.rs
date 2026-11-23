//! Gives Node.js programs Gazette's behavior: the native addon of the `@rtepl/gazette`
//! package, over the Gazette Rust SDK.
//!
//! This crate builds on `gazette-sdk`; no other Rust crate builds on it. It exposes
//! synchronous, supplied-source Node-API projections. Results and native failures cross
//! as exact JSON strings, never lossy JavaScript numbers or a second workspace model.
//! Native source reads require explicit permission on local publication and explicitly
//! pinned tree-inspection operations.

use gazette_sdk::{json, render::LimitedWriter, toml, workspace};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::{Serialize, de::DeserializeOwned};

mod authoring;
mod delivery;
mod documentation;
mod history;
mod inspection;
mod preparation;
mod publication;
mod qualification;
mod review;
pub use authoring::{
    PagePackageOptions, PagePackageRequest, PagePackageSource, inspect_page_package,
};
pub use delivery::{DeliveryOptions, observe_delivery};
pub use documentation::{DocumentationFiles, DocumentationOptions, assemble_documentation};
pub use history::{HistoryOptions, inspect_release_history};
pub use inspection::{
    DocumentOptions, MarkdownLimits, MarkdownOptions, MetadataLimits, TreeOptions,
    inspect_document, inspect_document_tree, inspect_markdown, inspect_markdown_tree,
};
pub use preparation::{PrepareOptions, prepare_sites};
pub use publication::{
    PublicationFile, PublicationFiles, PublicationSiteFiles, RenderOptions, render_site_files,
    render_sites,
};
pub use qualification::{QualificationOptions, qualify_sites};
pub use review::{ReviewOptions, review_publication};

/// Host transport ceilings; omitted parser fields retain the Rust SDK defaults.
#[napi(object)]
pub struct ProjectionLimits {
    /// Maximum supplied UTF-8 source bytes before copying or parsing.
    pub source_bytes: Option<f64>,
    /// Maximum native TOML syntax nodes.
    pub nodes: Option<f64>,
    /// Maximum native TOML nesting below its root.
    pub depth: Option<f64>,
    /// Maximum complete query JSON bytes before decoding; default 16384.
    pub query_bytes: Option<f64>,
    /// Maximum serialized success or native-failure JSON bytes; default 1048576.
    pub result_bytes: Option<f64>,
}

struct Limits {
    syntax: toml::Limits,
    query: usize,
    result: usize,
}

impl Limits {
    fn new(input: Option<ProjectionLimits>) -> Result<Self> {
        let defaults = toml::Limits::default();
        let Some(input) = input else {
            return Ok(Self {
                syntax: defaults,
                query: 16 * 1024,
                result: 1 << 20,
            });
        };
        Ok(Self {
            syntax: toml::Limits {
                source_bytes: integer(input.source_bytes, defaults.source_bytes)?,
                nodes: integer(input.nodes, defaults.nodes)?,
                depth: integer(input.depth, defaults.depth)?,
            },
            query: integer(input.query_bytes, 16 * 1024)?,
            result: integer(input.result_bytes, 1 << 20)?,
        })
    }
}

fn integer(value: Option<f64>, default: usize) -> Result<usize> {
    match value {
        None => Ok(default),
        Some(value)
            if value.is_finite()
                && value >= 0.0
                && value.fract() == 0.0
                && value <= 9_007_199_254_740_991.0
                && value <= usize::MAX as f64 =>
        {
            Ok(value as usize)
        }
        Some(_) => Err(transport("node_limits")),
    }
}

fn transport(code: &'static str) -> Error {
    Error::new(Status::InvalidArg, format!("{{\"code\":\"{code}\"}}"))
}

fn encode(value: &impl Serialize, maximum: usize) -> Result<String> {
    let mut writer = LimitedWriter::new(Vec::new(), maximum);
    let result = serde_json::to_writer(&mut writer, value);
    if writer.exceeded() {
        return Err(transport("node_result_limit"));
    }
    result.map_err(|_| transport("node_encoding"))?;
    String::from_utf8(writer.into_inner()).map_err(|_| transport("node_encoding"))
}

fn failure(value: workspace::Failure<'_>, maximum: usize) -> Error {
    match encode(&value, maximum) {
        Ok(json) => Error::new(Status::InvalidArg, json),
        Err(error) => error,
    }
}

fn source(source: &Uint8Array, limits: &Limits) -> Result<String> {
    if source.len() > limits.syntax.source_bytes {
        return Err(failure(
            workspace::Failure::from(&workspace::Error::Toml(toml::Error::Limit("source bytes"))),
            limits.result,
        ));
    }
    utf8(source)
}

fn utf8(source: &Uint8Array) -> Result<String> {
    // Own the supplied bytes for this call; no parser lifetime crosses into Node.
    String::from_utf8(source.to_vec()).map_err(|_| transport("node_source_utf8"))
}

fn query<T: DeserializeOwned>(json: &str, limits: &Limits) -> Result<T> {
    if json.len() > limits.query {
        return Err(transport("node_query_limit"));
    }
    // The SDK's shared decoder refuses duplicate decoded keys at every level.
    // Decode the original bytes into its typed query, never another field schema.
    let value = json::parse(json.as_bytes()).map_err(|_| transport("node_query"))?;
    if !value.is_object() {
        return Err(transport("node_query"));
    }
    serde_json::from_str(json).map_err(|_| transport("node_query"))
}

/// Compile complete supplied workspace bytes and return the SDK's exact JSON projection.
///
/// # Errors
/// Rejects host bounds/UTF-8 or native syntax/schema/semantic failures. Native
/// failures retain the shared SDK failure object as the thrown Error's JSON message.
#[napi]
pub fn check_workspace(bytes: Uint8Array, options: Option<ProjectionLimits>) -> Result<String> {
    let limits = Limits::new(options)?;
    let source = source(&bytes, &limits)?;
    let parsed = workspace::ParsedWorkspace::parse(&source, limits.syntax)
        .map_err(|error| failure(workspace::Failure::from(&error), limits.result))?;
    encode(parsed.compilation(), limits.result)
}

/// Locate an explicit field using the SDK's `FieldQuery` JSON and UTF-8 byte spans.
///
/// # Errors
/// Rejects malformed/over-limit query or source, unresolved workspaces, unknown
/// identities and fields without explicit native source coordinates.
#[napi]
pub fn field_origin(
    bytes: Uint8Array,
    query_json: String,
    options: Option<ProjectionLimits>,
) -> Result<String> {
    let limits = Limits::new(options)?;
    let source = source(&bytes, &limits)?;
    let query: workspace::FieldQuery = query(&query_json, &limits)?;
    let parsed = workspace::ParsedWorkspace::parse(&source, limits.syntax)
        .map_err(|error| failure(workspace::Failure::from(&error), limits.result))?;
    let value = parsed
        .origin(&query)
        .map_err(|error| failure(workspace::Failure::from(error), limits.result))?;
    encode(&value, limits.result)
}

/// Explain an effective Theme, capability or resource selection using the shared resolver and
/// origin index.
///
/// # Errors
/// Rejects malformed/over-limit input, unresolved workspaces and absent explicit
/// winning fields; inherited values do not receive fabricated local coordinates.
#[napi]
pub fn composition_origin(
    bytes: Uint8Array,
    query_json: String,
    options: Option<ProjectionLimits>,
) -> Result<String> {
    let limits = Limits::new(options)?;
    let source = source(&bytes, &limits)?;
    let query: workspace::CompositionOriginQuery = query(&query_json, &limits)?;
    let parsed = workspace::ParsedWorkspace::parse(&source, limits.syntax)
        .map_err(|error| failure(workspace::Failure::from(&error), limits.result))?;
    let value = parsed
        .composition_origin(&query)
        .map_err(|error| failure(workspace::Failure::from(error), limits.result))?;
    encode(&value, limits.result)
}
