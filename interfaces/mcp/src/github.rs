//! Exact public Pages observation, projected from the native SDK.

use gazette_cli::command::{Kernel, observation};
use gazette_cli::sdk::github;
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Exact JSON object containing curl, selection and options; its contentSchema
    /// is derived from the SDK types. A string preserves duplicate-field evidence.
    request_json: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request {
    curl: github::Curl,
    selection: github::PagesBuild,
    options: github::Options,
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input).ensure_object().clone();
    schema["properties"]["request_json"]["contentMediaType"] = Value::from("application/json");
    let request = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Request>();
    schema["properties"]["request_json"]["contentSchema"] =
        serde_json::to_value(request).expect("native request schema");
    Tool::new(
        "observe_github_pages_build",
        "Use observe_github_pages_build to learn what GitHub Pages reports about one exact public build. One call reads one exact public GitHub Pages build through the explicitly pinned trusted curl executable. Requires allow_read, allow_execute and allow_network independently. No credentials, latest-build inference, redirects, retry, compilation, Git push, build request, signing or release settlement. A complete response can report pending, mismatch, denial or unavailable evidence; a built status does not prove served bytes. Response/deadline, frame and complete result bounds apply; cancellation does not undo admitted native work. Use observe_delivery to learn what is actually served. The publisher decides what a pending or mismatched build means.",
        schema,
    ).with_raw_output_schema(crate::tools::output_schema::<github::PagesObservation>())
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    serde_json::from_value(Value::Object(arguments)).map_err(|_| ErrorData::invalid_params(
        "Invalid Pages observation arguments; consult inputSchema; no provider request performed", None))
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let invalid = || {
            crate::tools::failure(
                "github_request",
                "Invalid exact Pages request; no provider request performed",
                maximum,
            )
        };
        let value = match gazette_cli::sdk::json::parse(self.request_json.as_bytes()) {
            Ok(value)
                if value.is_object()
                    && value["curl"].is_object()
                    && value["curl"]["executable"].is_object()
                    && value["selection"].is_object()
                    && value["options"].is_object() =>
            {
                value
            }
            _ => return invalid(),
        };
        let request: Request = match serde_json::from_value(value) {
            Ok(request) => request,
            Err(_) => return invalid(),
        };
        match Kernel::new().execute(observation::ObserveGithubPages {
            curl: request.curl,
            selection: request.selection,
            options: request.options,
        }) {
            Ok(report) => crate::tools::encode(&report, false, maximum),
            Err(error) => crate::tools::failure(
                error.code(),
                "No complete Pages observation; no deployment or settlement",
                maximum,
            ),
        }
    }
}
