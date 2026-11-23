//! Documentation-repository assembly and explicitly selected reference generation through the Rust SDK.

mod generation;

use gazette_sdk::{build, source, toml, workspace};
use napi::{Error, Result, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Deserialize;

use crate::{
    Limits, PublicationFile, encode, integer, publication::file_payloads, query, transport,
};

/// Explicit local assembly permission and independent transport bounds.
#[napi(object)]
pub struct DocumentationOptions {
    /// Acknowledge private local assembly, not hosted compatibility or approval.
    pub local: bool,
    /// Permit exact configured source reads; never generator execution or writes.
    pub allow_read: bool,
    /// Permit fresh retained generator stages; omitted means denied.
    pub allow_stage: Option<bool>,
    /// Optional exact named native generation request JSON, bounded by `limits_bytes`.
    /// Its independent `allow_execute` flag defaults denied; commands are not sandboxed.
    pub generation_json: Option<String>,
    /// Optional native public-base to loopback-base JSON map for private manual previews.
    /// Parsed under the same exact JSON byte limit; never a maintained public setting.
    pub preview_link_bases_json: Option<String>,
    /// Complete workspace bytes before UTF-8 decoding; defaults to native TOML source bytes.
    pub input_bytes: Option<f64>,
    /// Complete native report JSON ceiling; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
    /// Each limits/generation/preview-map JSON ceiling before decoding; defaults to 16 KiB.
    pub limits_bytes: Option<f64>,
    /// Named native syntax, acquisition and output limits as exact JSON.
    pub limits_json: Option<String>,
}

/// Complete selected manual/reference assembly; the report retains its configured destination.
#[napi(object)]
pub struct DocumentationFiles {
    /// Exact native documentation report, including all modules and source evidence.
    pub report: String,
    /// Every regular file in native path order; directory entries remain in the report.
    pub files: Vec<PublicationFile>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeLimits {
    syntax: toml::Limits,
    acquisition: source::PreparationLimits,
    output: source::Limits,
}

impl Default for NativeLimits {
    fn default() -> Self {
        Self {
            syntax: toml::Limits::default(),
            acquisition: build::PublicationSetAcquisitionLimits::default().sources,
            output: source::Limits::default(),
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
        let value: serde_json::Value = query(json, &bounds)?;
        for path in [
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/output",
        ] {
            if value.pointer(path).is_some_and(|value| !value.is_object()) {
                return Err(transport("node_query"));
            }
        }
        serde_json::from_value(value).map_err(|_| transport("node_query"))
    }
}

/// Assemble one exact site's configured authored manual and references without staging the target.
/// Uses brand-owned destination and module selection, one retained capture and the
/// native target adapter. Returns owned bytes, not a persistent native handle.
///
/// # Errors
/// Refuses absent permissions, invalid inputs, native acquisition/target failures
/// and independent limits without a partial file result. Without a generation request,
/// configured generators retain `documentation_generation_required`. Requested generation
/// reserves complete custody delivery before effects; failures retain original jobs/stages.
#[napi]
pub fn assemble_documentation(
    bytes: Uint8Array,
    site: String,
    options: DocumentationOptions,
) -> Result<DocumentationFiles> {
    if !options.local || !options.allow_read {
        return Err(transport("documentation_permission"));
    }
    let maximum = integer(options.result_bytes, 1 << 20)?;
    let input = integer(options.input_bytes, toml::Limits::default().source_bytes)?;
    if bytes.len() > input {
        return Err(transport("node_input_limit"));
    }
    let json_maximum = integer(options.limits_bytes, 16 * 1024)?;
    let limits = NativeLimits::parse(options.limits_json.as_deref(), json_maximum)?;
    let preview_link_bases = options
        .preview_link_bases_json
        .as_deref()
        .map(|json| {
            let value: serde_json::Value = query(
                json,
                &Limits {
                    syntax: Default::default(),
                    query: json_maximum,
                    result: 0,
                },
            )?;
            if !value.is_object() {
                return Err(transport("node_query"));
            }
            serde_json::from_value::<gazette_sdk::publication::routing::PreviewLinkBases>(value)
                .map_err(|_| transport("node_query"))
        })
        .transpose()?;
    let generation = options
        .generation_json
        .as_deref()
        .map(|json| {
            generation::Request::parse(
                json,
                json_maximum,
                options.allow_stage == Some(true),
                maximum,
            )
        })
        .transpose()?;
    let site: workspace::Key = site.parse().map_err(|_| transport("documentation_site"))?;
    let source = std::str::from_utf8(&bytes).map_err(|_| transport("node_source_utf8"))?;
    let native_generation = generation
        .as_ref()
        .map(|request| request.selection(limits.output));
    build::documentation_output::Operation {
        preview_link_bases: preview_link_bases.as_ref(),
        input: &build::WorkspaceInput::Supplied {
            source: source.to_owned(),
        },
        site: &site,
        syntax: limits.syntax,
        acquisition: limits.acquisition,
        output: limits.output,
        generation: native_generation,
        retention: build::documentation_output::RetentionSelection::None,
        response_bytes: maximum,
        reserve_read_only_response: false,
    }
    .execute_with_delivery(|execution| match execution {
        build::documentation_output::Execution::Before(error) => Err(transport(error.code())),
        build::documentation_output::Execution::Complete { report, assembly } => {
            let report = encode(&report, maximum)?;
            let files = file_payloads(assembly.output())?;
            Ok(DocumentationFiles { report, files })
        }
        build::documentation_output::Execution::Contract { custody } => Err(Error::from_reason(
            serde_json::json!({"code":"documentation_delivery_contract", "custody":format!("{custody:?}"),
                "warning":"Original custody follows without a bounded-delivery guarantee; do not retry or infer rollback"}).to_string(),
        )),
        build::documentation_output::Execution::Delivered { response, assembly, .. } => {
            let report = String::from_utf8(response.bytes)
                .map_err(|_| transport("documentation_delivery_contract"))?;
            if response.is_error {
                return Err(Error::from_reason(report));
            }
            let files = file_payloads(assembly.ok_or_else(|| transport("node_output_contract"))?.output())?;
            Ok(DocumentationFiles { report, files })
        }
    })
}
