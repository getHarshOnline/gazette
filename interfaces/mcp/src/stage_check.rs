//! Write-free staging preflight through the existing SDK source pipeline.

use gazette_cli::command::{Kernel, rendered_output as command};
use gazette_cli::sdk::source::{self, PreparationLimits, StageContext, input};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::path::PathBuf;

use crate::tools::{encode, failure};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request<Mode = Check> {
    /// Must be true; this does not override native host permissions.
    pub(super) allow_native_reads: bool,
    /// Explicit absolute authored input root; never modified.
    pub(super) authored: PathBuf,
    /// Explicit absolute existing staging parent; must be disjoint from inputs.
    pub(super) parent: PathBuf,
    /// Explicit workspace identity, not a path or approval.
    workspace: String,
    /// Explicit site identity, not inferred from paths.
    site: String,
    /// Complete placement set. An explicit empty array selects authored input only.
    #[serde(deserialize_with = "placements")]
    placements: Vec<input::Placement>,
    /// Acquisition ceilings: sources, entries, bytes and `per_source` are required.
    /// Nested per-source fields retain the native SDK defaults when omitted.
    pub(super) preparation: PreparationLimits,
    /// Independent authored-input and complete-composition ceilings.
    pub(super) limits: source::Limits,
    #[serde(flatten)]
    pub(super) mode: Mode,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Check {}

fn placements<'de, D: Deserializer<'de>>(decoder: D) -> Result<Vec<input::Placement>, D::Error> {
    #[derive(Deserialize)]
    struct Named(#[serde(deserialize_with = "input::object")] input::Placement);
    Vec::<Named>::deserialize(decoder)
        .map(|values| values.into_iter().map(|value| value.0).collect())
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Request);
    Tool::new(
        "check_stage",
        "Use check_stage to learn whether the pinned sources and authored input compose into a valid stage, without creating one. One call reads exactly selected pinned project sources and authored input, then checks their complete staged composition and existing output-parent separation through Gazette's SDK. No files, stages or receipts are created, no fetch/checkout/filter is executed, and no content is published. Success is an observation, not a write lease, policy admission or release approval. placements must be supplied explicitly, including [] for authored-only. Native-read permission is required. Results disclose logical paths and hashes.",
        schema.ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<Report>())
    .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(true))
}

pub(super) struct Input<Mode = Check> {
    pub(super) request: Request<Mode>,
    pub(super) context: StageContext,
    pub(super) placements: Vec<source::PlacementRequest>,
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    decode_mode(arguments)
}

pub(super) fn decode_mode<Mode: DeserializeOwned>(
    arguments: JsonObject,
) -> Result<Input<Mode>, ErrorData> {
    let value = Value::Object(arguments);
    for path in [
        "/preparation",
        "/preparation/per_source",
        "/preparation/per_source/content",
        "/limits",
    ] {
        if value.pointer(path).is_some_and(|value| !value.is_object()) {
            return Err(invalid());
        }
    }
    let mut request: Request<Mode> = serde_json::from_value(value).map_err(|_| invalid())?;
    if !request.allow_native_reads
        || !request.authored.is_absolute()
        || !request.parent.is_absolute()
        || request
            .placements
            .iter()
            .any(|placement| !placement.source.root().is_absolute())
        || request.placements.len() > request.preparation.sources
    {
        return Err(invalid());
    }
    let (context, placements) = input::bind(
        request.workspace.clone(),
        request.site.clone(),
        std::mem::take(&mut request.placements),
    )
    .map_err(|_| invalid())?;
    Ok(Input {
        request,
        context,
        placements,
    })
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid staging arguments: supply named objects, explicit native-read permission, absolute roots and a complete bounded placement set; consult inputSchema",
        None,
    )
}

#[derive(Serialize, JsonSchema)]
struct Report<'a> {
    status: Status,
    workspace: &'a str,
    site: &'a str,
    inventory: &'a source::Inventory,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Status {
    Checked,
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(command::CheckStage(command::StageInput {
            allow_read: self.request.allow_native_reads,
            context: self.context,
            placements: self.placements,
            preparation: self.request.preparation,
            authored: self.request.authored,
            parent: self.request.parent,
            limits: self.request.limits,
        })) {
            Ok(outcome) => encode(
                &Report {
                    status: Status::Checked,
                    workspace: &outcome.context().workspace,
                    site: &outcome.context().site,
                    inventory: outcome.inventory(),
                },
                false,
                maximum,
            ),
            Err(error) => failure(error.code(), error.message(), maximum),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/stage_check.rs"]
mod tests;
