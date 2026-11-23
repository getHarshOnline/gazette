//! Complete-set technical qualification through the same SDK operation as the CLI.

use gazette_cli::command::{Kernel, publication as command};
use gazette_cli::sdk::{build, source};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Exact native recipe JSON; retain duplicate-field evidence for SDK rejection.
    recipe_json: String,
    /// Permit configured native reads; never implies checker execution permission.
    #[serde(default)]
    allow_read: bool,
    /// Permit the configured candidate checker, which Gazette does not sandbox.
    #[serde(default)]
    allow_policy_execution: bool,
    /// Native per-site output bounds; independent aggregate SDK limits still apply.
    #[serde(default)]
    output: source::Limits,
    /// Complete recipe byte ceiling; the MCP frame ceiling remains independent.
    #[serde(default = "default_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_input_bytes: usize,
    /// Complete result byte ceiling, additionally bounded by the server ceiling.
    #[serde(default = "default_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_output_bytes: usize,
}

fn default_bytes() -> usize {
    4 * 1024 * 1024
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input).ensure_object().clone();
    schema["properties"]["recipe_json"]["contentMediaType"] = "application/json".into();
    schema["properties"]["recipe_json"]["contentSchema"] =
        serde_json::to_value(build::QualificationRecipe::input_schema())
            .expect("native recipe schema");
    Tool::new(
        "qualify_sites",
        "Use qualify_sites to learn whether the complete set of sites passes its candidate-policy checks and renders, before anyone asks for approval. One call qualifies the explicit complete site set through the same SDK recipe as CLI qualify-sites: it acquires selected sources, runs each site's required candidate-policy check, then renders and inspects the same candidate. Returns the full technical qualification report; success requires every selected site, not a silently reduced set. This is not publisher approval, release authorization or deployment. No output stages, journal/store writes, Git preparation, signing or push. Native reads and unsandboxed configured checker execution have independent opt-ins. Explicit generation requests retain their own permissions and process effects. Preserve the original recipe before calling: response loss or cancellation cannot prove that checkers/generators did not run. Never infer rollback or blindly retry. Input, result, frame and native limits are independent; annotations grant no permissions. Approval, release and deployment stay with the publisher.",
        schema,
    )
    .with_raw_output_schema(crate::tools::output_schema::<build::QualificationReport>())
    .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    if arguments
        .get("output")
        .is_some_and(|value| !value.is_object())
    {
        return Err(ErrorData::invalid_params(
            "Native output limits must be a named object; no qualification started",
            None,
        ));
    }
    serde_json::from_value(Value::Object(arguments)).map_err(|_| {
        ErrorData::invalid_params(
            "Invalid qualification arguments; consult inputSchema; no qualification started",
            None,
        )
    })
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let maximum = maximum.min(self.max_output_bytes);
        // Bound effect-aware fallback evidence before any source or checker work.
        let unavailable = crate::tools::failure_complete(
            "qualification_response_unavailable",
            "No complete qualification report delivered; configured checkers or generators may have run. Preserve the original recipe and inspect native outcomes before further work; do not infer rollback or retry. No release preparation performed.",
            maximum,
        ).map_err(|_| ErrorData::invalid_params("Response ceiling cannot hold qualification failure evidence; no qualification started", None))?;
        let report = match Kernel::new().execute(command::Qualify {
            recipe_json: self.recipe_json.into_bytes(),
            max_input_bytes: self.max_input_bytes,
            allow_read: self.allow_read,
            allow_policy_execution: self.allow_policy_execution,
            output: self.output,
        }) {
            Ok(report) => report,
            Err(error) => return crate::tools::failure(error.code(), error.message(), maximum),
        };
        Ok(
            crate::tools::encode_complete(&report, !report.is_complete(), maximum)
                .unwrap_or(unavailable),
        )
    }
}
