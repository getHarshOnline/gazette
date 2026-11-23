//! Complete local rendering through the same owned request and SDK as CLI/preview.

use gazette_cli::command::{Kernel, publication as command};
use gazette_cli::sdk::{build, source, toml};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Complete native local publication-set JSON. The string retains duplicate
    /// keys for shared native rejection before any source acquisition.
    #[serde(default)]
    #[schemars(with = "String")]
    recipe_json: Option<String>,
    /// Native workspace input instead of `recipe_json`; exactly one is required.
    #[serde(default)]
    #[schemars(with = "build::WorkspaceInput")]
    workspace: Option<build::WorkspaceInput>,
    /// Override workspace math selection without granting native execution.
    #[serde(default)]
    #[schemars(with = "bool")]
    math: Option<bool>,
    /// Permit this call's configured math, sharing-image and documentation processes; defaults denied.
    #[serde(default)]
    allow_execute: bool,
    /// Acknowledge private local rendering, not publication admission or certification.
    local: bool,
    /// Permit exact configured local source reads. Defaults denied; never network discovery.
    #[serde(default)]
    allow_read: bool,
    /// Complete native recipe byte ceiling, independent of the MCP frame ceiling.
    #[serde(default = "default_input_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_input_bytes: usize,
    /// Complete supplied workspace TOML syntax ceilings.
    #[serde(default)]
    pub(super) syntax: toml::Limits,
    /// Exact site-count and union-of-sources acquisition ceilings.
    #[serde(default)]
    pub(super) acquisition: build::PublicationSetAcquisitionLimits,
    /// Authored corpus parsing ceilings, including opaque retained assets.
    /// These are independent from acquisition, output and MCP response bounds.
    #[serde(default)]
    corpus: build::CorpusLimits,
    /// Per-site generated-output ceilings; these do not enlarge the response budget.
    #[serde(default)]
    pub(super) output: source::Limits,
    /// Maximum bytes per declared resource input; aggregate and decoder bounds stay separate.
    #[serde(default = "default_resource_file_bytes")]
    #[schemars(range(max = u64::MAX))]
    max_resource_file_bytes: u64,
    /// Aggregate rendered bytes across the complete selected site set.
    #[serde(default = "default_total_bytes")]
    #[schemars(range(max = u64::MAX))]
    max_total_output_bytes: u64,
    /// Aggregate file/directory entries across the complete selected site set.
    #[serde(default = "default_total_entries")]
    #[schemars(range(max = usize::MAX))]
    max_total_output_entries: usize,
}

fn default_input_bytes() -> usize {
    toml::Limits::default().source_bytes
}

fn default_resource_file_bytes() -> u64 {
    build::LocalRenderLimits::default()
        .publication
        .output
        .resources
        .inputs
        .file_bytes
}

fn default_total_bytes() -> u64 {
    build::LocalRenderLimits::default().bytes
}

fn default_total_entries() -> usize {
    build::LocalRenderLimits::default().entries
}

pub(super) fn input_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Input>();
    let recipe = schema
        .ensure_object()
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|p| p.get_mut("recipe_json"))
        .and_then(Value::as_object_mut)
        .expect("native recipe field");
    recipe.insert("contentMediaType".into(), "application/json".into());
    recipe.insert(
        "contentSchema".into(),
        serde_json::to_value(build::LocalPublicationRecipe::input_schema())
            .expect("native schema is JSON"),
    );
    schema.ensure_object().insert(
        "oneOf".into(),
        serde_json::json!([
            {"required":["recipe_json"],"not":{"required":["workspace"]}},
            {"required":["workspace"],"not":{"required":["recipe_json"]}}
        ]),
    );
    schema
}

// Discovery describes both retained output evidence and original native failure custody.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum Output {
    Rendered {
        #[serde(flatten)]
        inspection: build::PublicationSetInspection<'static>,
        #[serde(skip_serializing_if = "Option::is_none")]
        documentation: Option<Vec<build::DocumentationReceipt>>,
    },
    Documentation(DocumentationReport),
}
#[derive(JsonSchema)]
#[allow(dead_code)]
struct DocumentationReport {
    status: DocumentationStatus,
    code: String,
    documentation: Vec<build::DocumentationReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    site: Option<gazette_cli::sdk::workspace::Key>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    custody: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_error: Option<bool>,
}
#[derive(JsonSchema)]
#[allow(dead_code)]
enum DocumentationStatus {
    #[serde(rename = "documentation-publication-failed")]
    Failed,
    #[serde(rename = "documentation-publication-output-limit")]
    OutputLimit,
}

// The MCP result contains structured JSON and an escaped textual copy. Reserve the
// actual result envelope plus six bytes per input byte for worst-case JSON escaping.
// JSON-RPC request IDs belong to the surrounding server transport, not this result budget.
fn native_capacity(maximum: usize) -> usize {
    let empty = crate::tools::encoded(b"{}".to_vec(), true)
        .unwrap_or_else(|_| unreachable!("empty native result"));
    let envelope = serde_json::to_vec(&empty)
        .expect("serializable MCP result")
        .len();
    maximum.saturating_sub(envelope) / 7
}
fn encode_framed(
    value: &Value,
    failed: bool,
    maximum: usize,
) -> Result<CallToolResult, crate::tools::EncodingError> {
    let result = crate::tools::encode_complete(value, failed, maximum)?;
    let mut writer = gazette_cli::sdk::render::LimitedWriter::new(std::io::sink(), maximum);
    serde_json::to_writer(&mut writer, &result).map_err(|_| crate::tools::EncodingError::Limit)?;
    Ok(result)
}
fn deliver_report(
    report: &Value,
    failed: bool,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    if let Ok(result) = encode_framed(report, failed, maximum) {
        return Ok(result);
    }
    if report.get("documentation").is_none() {
        return crate::tools::failure(
            "mcp_output_limit",
            "Complete publication inspection exceeds response capacity",
            maximum,
        );
    }
    let retained = command::documentation_reconciliation(report);
    encode_framed(&retained, true, maximum).map_err(|_| ErrorData::internal_error(
        "Native documentation delivery exceeded its reserved contract; original custody retained; inspect before retrying",
        Some(retained),
    ))
}

pub(super) fn descriptor() -> Tool {
    let mut schema = input_schema(&mut schemars::SchemaGenerator::default());
    Tool::new(
        "render_sites",
        "Use render_sites to see what a publication's sites look like when built, locally, without publishing them. One call renders one or several explicitly configured sites through the same SDK request, pinned source acquisition, shared semantic model, resource compilers and Theme as CLI render-sites --local. Requires local=true and allow_read=true. Workspace math selection and corpus bytes are defaults; invocation overrides win. Math execution requires this call's allow_execute or its explicit recipe permission; diagram_generation retains its separate permission. Math executes the exact retained provider tree. Graphviz accepts only Gazette's file-free DOT profile and fixed stdout invocation from a verified empty disposable directory; its executable can still load ambient native libraries, plugins and fonts. Native execution may have effects and must not be automatically retried. Returns complete private source and per-site output evidence, including every failed site; a failed sibling makes the whole tool result an error without removing successful siblings. Selection is an optional complete non-draft candidate, not public admission. An explicit recipe documentation_generation parent permits retained private generator stages; its processes also require this call’s allow_execute=true. Receipts survive later render or response-limit failure; inspect them before retrying. Without that selection there is no staging. Gazette performs no store access, policy subprocess, listeners, source writes, signing or publication. Publication outputs remain in memory; selected documentation generation retains private input/output stages described by its receipts. Native, MCP frame/result and worker bounds all apply; cancellation does not establish rollback or stop admitted execution. Publication stays with the publisher.",
        schema.ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<Output>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let arguments = Value::Object(arguments);
    validate_shape(&arguments)?;
    serde_json::from_value(arguments).map_err(|_| invalid())
}

pub(super) fn validate_shape(arguments: &Value) -> Result<(), ErrorData> {
    if !arguments.is_object() {
        return Err(invalid());
    }
    for field in ["syntax", "acquisition", "corpus", "output"] {
        if arguments.get(field).is_some_and(|v| !v.is_object()) {
            return Err(invalid());
        }
    }
    for path in [
        "/corpus/document",
        "/corpus/document/metadata",
        "/corpus/document/markdown",
        "/corpus/notebook",
    ] {
        if arguments
            .pointer(path)
            .is_some_and(|value| !value.is_object())
        {
            return Err(invalid());
        }
    }
    if arguments.get("recipe_json").is_some() == arguments.get("workspace").is_some()
        || arguments.get("recipe_json").is_some_and(|v| !v.is_string())
        || arguments.get("workspace").is_some_and(|v| !v.is_object())
        || arguments.get("math").is_some_and(|v| !v.is_boolean())
        || arguments
            .pointer("/corpus/source_bytes")
            .is_some_and(|v| !v.is_u64())
    {
        return Err(invalid());
    }
    if arguments
        .get("acquisition")
        .and_then(|v| v.get("sources"))
        .is_some_and(|v| {
            !v.is_object()
                || v.get("per_source").is_some_and(|v| {
                    !v.is_object() || v.get("content").is_some_and(|v| !v.is_object())
                })
        })
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid local render arguments; consult inputSchema; no source acquisition started",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let native_capacity = native_capacity(maximum);
        match Kernel::new().execute(command::BoundedRender {
            command: self.into_command()?,
            response_bytes: native_capacity,
        }) {
            Ok(outcome) => deliver_report(outcome.report(), !outcome.is_complete(), maximum),
            Err(error) => crate::tools::failure(error.code(), error.message(), maximum),
        }
    }

    pub(super) fn into_command(self) -> Result<command::Render, ErrorData> {
        let year = gazette_cli::calendar::resolve(None)
            .map_err(|reason| ErrorData::internal_error(reason, None))?;
        Ok(command::Render {
            default_render_year: Some(year),
            recipe_json: match (self.recipe_json, self.workspace) {
                (Some(recipe), None) => recipe.into_bytes(),
                (None, Some(workspace)) => serde_json::to_vec(&serde_json::json!({
                    "format": "gazette-local-publication-set/1", "workspace": workspace
                }))
                .map_err(|_| invalid())?,
                _ => return Err(invalid()),
            },
            math: self.math,
            allow_math_execution: self.allow_execute,
            local: self.local,
            allow_read: self.allow_read,
            max_input_bytes: self.max_input_bytes,
            syntax: self.syntax,
            acquisition: self.acquisition,
            output: self.output,
            limits: build::PublicationSetRenderLimits {
                publication: build::PublicationRenderLimits {
                    corpus: self.corpus,
                    output: build::PublicationOutputLimits {
                        resources: gazette_cli::sdk::resource::Limits {
                            inputs: source::Limits {
                                file_bytes: self.max_resource_file_bytes,
                                ..gazette_cli::sdk::resource::Limits::default().inputs
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                bytes: self.max_total_output_bytes,
                entries: self.max_total_output_entries,
            },
        })
    }
}

#[cfg(test)]
#[path = "../tests/internal/publication.rs"]
mod tests;
