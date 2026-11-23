//! Effect-aware protocol projection of the shared local preparation operation.

use gazette_cli::sdk::{build, release_commit, source, store};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Complete exact SDK recipe JSON. A string preserves duplicate keys for native rejection.
    recipe_json: String,
    /// Permit selected pinned local source acquisition; never remote discovery.
    #[serde(default)]
    allow_read: bool,
    /// Permit fresh named output stages; existing children are never adopted.
    #[serde(default)]
    create_stage: bool,
    /// Permit immutable phase records in the existing journal, not initialization or migration.
    #[serde(default)]
    record_preparation: bool,
    /// Permit local writer generation advancement independently of record insertion.
    #[serde(default)]
    advance_writer_generation: bool,
    /// Permit held local namespace reservations during native phases.
    #[serde(default)]
    reserve_local_coordination: bool,
    /// Permit writes to the exact selected coordinator anchor; never enrollment or repair.
    #[serde(default)]
    write_coordinator_anchor: bool,
    /// Permit independent Git metadata and detached commit creation; never push or signing.
    #[serde(default)]
    initialize_worktree: bool,
    /// Permit the exact configured policy executable. Gazette does not sandbox its effects.
    #[serde(default)]
    allow_policy_execution: bool,
    /// Independent journal coordination permission and bounded native connection policy.
    #[serde(default)]
    read: store::ReadOptions,
    /// Native per-site output bounds. Aggregate SDK bounds remain independently enforced.
    #[serde(default)]
    output: source::Limits,
    /// Recipe byte ceiling. The complete MCP frame ceiling can be stricter.
    #[serde(default = "default_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_input_bytes: usize,
    /// Result byte ceiling. The server ceiling is always an independent upper bound.
    #[serde(default = "default_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_output_bytes: usize,
}

fn default_bytes() -> usize {
    4 * 1024 * 1024
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input);
    let recipe = schema
        .ensure_object()
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut("recipe_json"))
        .expect("named native recipe field")
        .as_object_mut()
        .expect("named native recipe field");
    recipe.insert("contentMediaType".into(), "application/json".into());
    recipe.insert(
        "contentSchema".into(),
        serde_json::to_value(build::LocalPreparationRecipe::input_schema())
            .expect("native schema is JSON"),
    );
    Tool::new(
        "prepare_sites",
        "Use prepare_sites to prepare the complete set of sites for release locally: check them, render them and keep the result in Git worktrees. One call performs local-only complete-set preparation through Gazette's shared SDK: it acquires exact selected inputs once, runs an explicitly selected candidate check for every site, renders the same complete candidate, then retains stages, check and record native Git worktrees. This is the CLI prepare-sites --local operation, not certification, release, signing, push, journal initialization or migration. All effect permissions are independent and default denied, including read.allow_coordination_files and write_coordinator_anchor. The recipe's explicit coordinator selection guards each native writer lifetime; null keeps the ordinary local writer. No enrollment, repair or whole-set lock is implied. Optional configured policy execution is NOT sandboxed and may have external effects. Returns exact completed prefix and native phase evidence on failure. On response overflow, inspect the original journal/preparation and selected coordinator coordinates; a checker may have run even without a preparation record. Never blindly retry or infer rollback. Cancellation or carrier loss does not necessarily stop admitted blocking work; retain request coordinates before calling. MCP frame/result and worker ceilings apply in addition to native bounds; annotations grant no permission. Certifying, releasing, signing and pushing stay with the publisher.",
        schema.ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<build::LocalPreparationDelivery<'static>>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(false)
            .destructive(true)
            .idempotent(false)
            .open_world(true),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    for field in ["read", "output"] {
        if arguments.get(field).is_some_and(|v| !v.is_object()) {
            return Err(ErrorData::invalid_params(
                "Native options must be named objects; no preparation started",
                None,
            ));
        }
    }
    serde_json::from_value(Value::Object(arguments)).map_err(|_| {
        ErrorData::invalid_params(
            "Invalid local preparation arguments; consult inputSchema; no preparation started",
            None,
        )
    })
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let maximum = maximum.min(self.max_output_bytes);
        let options = build::LocalPreparationRunOptions {
            allow_read: self.allow_read,
            output: self.output,
            preparation: build::LocalPreparationOptions {
                allow_anchor_write: self.write_coordinator_anchor,
                stage: build::PreparationOptions {
                    allow_stage: self.create_stage,
                    output: self.output,
                    record: store::PreparationWriteOptions {
                        allow_record: self.record_preparation,
                    },
                    writer: store::WriterOptions {
                        allow_generation_advance: self.advance_writer_generation,
                        session: store::SessionOptions {
                            allow_local_reservation: self.reserve_local_coordination,
                            read: self.read,
                        },
                    },
                    ..Default::default()
                },
                policy: gazette_cli::sdk::process::Options {
                    allow_execute: self.allow_policy_execution,
                    ..Default::default()
                },
                worktree: release_commit::WorktreeOptions {
                    allow_initialize: self.initialize_worktree,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };
        let recipe = match build::LocalPreparationRecipe::parse_json(
            self.recipe_json.as_bytes(),
            self.max_input_bytes,
            options.acquisition.sites,
        ) {
            Ok(recipe) => recipe,
            Err(error) => {
                return crate::tools::failure("local_preparation_input", error, maximum);
            }
        };
        match recipe.execute_with_delivery(options, |report, is_error| {
            crate::tools::encode_complete(report, is_error, maximum)
        }) {
            Ok(encoded) => Ok(encoded),
            Err(_) => crate::tools::failure(
                "local_preparation_response_budget",
                "No preparation started: complete reconciliation coordinates do not fit the response ceiling",
                maximum,
            ),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/preparation.rs"]
mod tests;
