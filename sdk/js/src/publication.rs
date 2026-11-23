//! Explicit local acquisition and rendering; the SDK owns the complete pipeline.

use gazette_sdk::{build, source, toml};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Deserialize;

use crate::{Limits, encode, integer, query, transport};

mod files;
pub(crate) use files::file_payloads;
pub use files::{PublicationFile, PublicationFiles, PublicationSiteFiles, render_site_files};

/// Permissions and independent host bounds for a private local publication report.
#[napi(object)]
pub struct RenderOptions {
    /// Acknowledge private local rendering, not admission or certification.
    pub local: bool,
    /// Permit exact configured source reads; no discovery or network fallback.
    pub allow_read: bool,
    /// Complete recipe byte ceiling before parsing; defaults to native TOML source bytes.
    pub input_bytes: Option<f64>,
    /// Maximum serialized complete report bytes; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
    /// Maximum supplied limits JSON bytes before decoding; defaults to 16 KiB.
    pub limits_bytes: Option<f64>,
    /// Native syntax, acquisition, output and aggregate ceilings as exact JSON.
    /// Omitted fields retain native defaults; values are not coerced through JS numbers.
    pub limits_json: Option<String>,
}

// Compose existing native limit types, not a second source or publication schema.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeLimits {
    syntax: toml::Limits,
    acquisition: build::PublicationSetAcquisitionLimits,
    output: source::Limits,
    max_total_output_bytes: u64,
    max_total_output_entries: usize,
}

impl Default for NativeLimits {
    fn default() -> Self {
        let render = build::LocalRenderLimits::default();
        Self {
            syntax: toml::Limits::default(),
            acquisition: build::PublicationSetAcquisitionLimits::default(),
            output: source::Limits::default(),
            max_total_output_bytes: render.bytes,
            max_total_output_entries: render.entries,
        }
    }
}

impl NativeLimits {
    fn parse(json: Option<&str>, maximum: usize) -> Result<Self> {
        let Some(json) = json else {
            return Ok(Self::default());
        };
        let bounds = Limits {
            syntax: toml::Limits::default(),
            query: maximum,
            result: 0,
        };
        // Native Serde structs accept positional arrays too; this host exposes only
        // named objects. The shared decoder also refuses duplicate decoded keys.
        let value: serde_json::Value = query(json, &bounds)?;
        for path in [
            "/syntax",
            "/acquisition",
            "/output",
            "/acquisition/sources",
            "/acquisition/sources/per_source",
            "/acquisition/sources/per_source/content",
        ] {
            if value.pointer(path).is_some_and(|v| !v.is_object()) {
                return Err(transport("node_query"));
            }
        }
        serde_json::from_value(value).map_err(|_| transport("node_query"))
    }
}

/// Render the exact requested site set through retained sources and the shared SDK.
/// Returns private native inspection JSON, not generated files or certification.
/// No staging, policy subprocesses, listeners, stores, signing or publication.
/// Explicit recipe math generation can run the configured native command;
/// source-read permission alone does not permit it, and callers must not retry effects blindly.
///
/// # Errors
/// Refuses missing permissions, malformed input and exhausted independent bounds.
/// An incomplete set throws its entire native inspection, including successful
/// siblings. Earlier failures disclose their class only, never private source text.
#[napi]
pub fn render_sites(bytes: Uint8Array, options: RenderOptions) -> Result<String> {
    with_rendered(bytes, options, |_, report| Ok(report))
}

fn with_rendered<T>(
    bytes: Uint8Array,
    options: RenderOptions,
    consume: impl FnOnce(&build::RenderedPublicationSet<'_>, String) -> Result<T>,
) -> Result<T> {
    if !options.local || !options.allow_read {
        return Err(transport("publication_permission"));
    }
    let maximum = integer(options.result_bytes, 1 << 20)?;
    let input = integer(options.input_bytes, toml::Limits::default().source_bytes)?;
    if bytes.len() > input {
        return Err(transport("node_input_limit"));
    }
    let limits = NativeLimits::parse(
        options.limits_json.as_deref(),
        integer(options.limits_bytes, 16 * 1024)?,
    )?;
    let recipe = build::LocalPublicationRecipe::parse_json(&bytes, input, limits.acquisition.sites)
        .map_err(|_| transport("publication_set_input"))?;
    recipe
        .with_rendered(
            limits.syntax,
            limits.acquisition,
            limits.output,
            build::PublicationSetRenderLimits {
                bytes: limits.max_total_output_bytes,
                entries: limits.max_total_output_entries,
                ..Default::default()
            },
            |rendered| {
                let report = encode(&rendered.inspect(), maximum)?;
                if rendered.is_complete() {
                    consume(rendered, report)
                } else {
                    Err(Error::new(Status::GenericFailure, report))
                }
            },
        )
        .map_err(|error| transport(error.code()))?
}
