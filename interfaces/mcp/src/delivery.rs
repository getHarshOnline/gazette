//! Served-byte evidence through the same complete SDK request used by the CLI.

use gazette_cli::command::{Kernel, observation};
use gazette_cli::sdk::delivery;
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Complete native request JSON; raw text preserves duplicate-field evidence.
    #[schemars(length(max = 1048576))]
    request_json: String,
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input).ensure_object().clone();
    schema["properties"]["request_json"]["contentMediaType"] = Value::from("application/json");
    schema["properties"]["request_json"]["contentSchema"] =
        serde_json::to_value(delivery::request_schema()).expect("shared delivery request schema");
    Tool::new(
        "observe_delivery",
        "Use observe_delivery to learn whether the routes a site serves match the output you retained. One call compares every explicitly selected served route with an independently pinned retained local output tree. Uses the explicitly pinned trusted curl executable, with bounded complete responses and no redirects, credentials, automatic retries, rebuilding, filesystem writes, signing, publication or release settlement. Returns the complete selected-route report even for mismatch, HTTP failure or unavailable evidence; isError is true unless every selected route matches. This observation proves neither unselected routes nor provider readiness. Native input is limited to 1 MiB of UTF-8 bytes; frame/result ceilings also apply. Report a mismatch or unavailable evidence to the publisher, who decides what follows.",
        schema,
    ).with_raw_output_schema(crate::tools::output_schema::<delivery::Report>())
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    serde_json::from_value(Value::Object(arguments)).map_err(|_| {
        ErrorData::invalid_params(
            "Invalid delivery arguments; consult inputSchema; no native observation performed",
            None,
        )
    })
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(observation::ObserveDelivery {
            request_json: self.request_json.into_bytes(),
            max_input_bytes: 1 << 20,
        }) {
            Ok(outcome) => crate::tools::encode(
                outcome.report(),
                !outcome.matches_selected_routes(),
                maximum,
            ),
            Err(error) => crate::tools::failure(
                error.code(),
                "No complete served-output report; no publication or settlement",
                maximum,
            ),
        }
    }
}
