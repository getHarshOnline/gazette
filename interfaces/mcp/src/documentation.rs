//! Configured reference generation and assembly project the existing SDK phase owners.

use gazette_cli::command::{Kernel, documentation as command};
use gazette_cli::sdk::{
    build::{self, documentation_output as output},
    source, toml,
    workspace::Key,
};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::{path::PathBuf, time::Duration};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Optional public URL base to literal-loopback base map for local manual links.
    /// Preserves path/query/fragment; refuses durable preparation and maintained output.
    #[serde(default)]
    preview_link_bases: Option<gazette_cli::sdk::publication::routing::PreviewLinkBases>,
    /// Exact shared supplied/local workspace selection; relative configured paths
    /// require a loaded native owner, never the MCP process directory.
    workspace: build::WorkspaceInput,
    /// Exact configured publication site, not an inferred current project.
    site: Key,
    /// Acknowledge local private work, not public admission or hosted compatibility.
    local: bool,
    /// Independent configured-source read permission; omitted means denied.
    #[serde(default)]
    allow_read: bool,
    /// Independent permission for fresh retained job/target stages; omitted means denied.
    #[serde(default)]
    allow_stage: bool,
    /// Canonical native path-byte ceiling for every retained stage; also reserves lossless delivery.
    #[schemars(range(min = 1, max = 1_048_576))]
    directory_bytes: usize,
    /// Optional existing absolute parent for a fresh assembled target; never the configured destination.
    #[serde(default)]
    #[schemars(regex(pattern = "^/"))]
    output_parent: Option<PathBuf>,
    /// Optional durable local preparation; mutually exclusive with `output_parent`.
    /// Its own stage, history and writer permissions are independent and default denied.
    #[serde(default)]
    preparation: Option<output::PreparationSelection>,
    /// Explicit native generator selection. Absence never falls back for generated references.
    #[serde(default)]
    generation: Option<Generation>,
    /// Native workspace syntax bounds.
    #[serde(default)]
    syntax: toml::Limits,
    /// Complete selected-source acquisition and per-source bounds.
    #[serde(default = "acquisition")]
    acquisition: source::PreparationLimits,
    /// Complete generated-set and assembled-tree byte/entry ceilings, plus per-file bounds.
    #[serde(default)]
    output: source::Limits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Generation {
    /// Existing absolute private parent for fresh input/output stages; no existing tree is reused.
    #[schemars(regex(pattern = "^/"))]
    parent: PathBuf,
    /// Complete configured job ceiling, independent of the MCP response capacity.
    #[schemars(range(max = 128))]
    max_jobs: usize,
    /// Selected relative artifact entry-byte ceiling; used by the native owner and response reservation.
    #[schemars(range(min = 1, max = 1_048_576))]
    entry_bytes: usize,
    /// Explicit native command permission, independent of source reads and stage creation.
    #[serde(default)]
    allow_execute: bool,
    /// Native shared post-spawn deadline in milliseconds, matching the direct CLI.
    #[serde(default = "timeout_ms")]
    #[schemars(range(min = 1, max = 300_000))]
    timeout_ms: u64,
    /// Complete version/generation stdout ceiling; incomplete output is not accepted.
    #[serde(default = "capture_bytes")]
    #[schemars(range(max = 67_108_864))]
    max_capture_bytes: usize,
    /// Per-file and complete repeated-input stage bounds.
    #[serde(default)]
    input: source::Limits,
}

fn acquisition() -> source::PreparationLimits {
    build::PublicationSetAcquisitionLimits::default().sources
}
fn timeout_ms() -> u64 {
    30_000
}
fn capture_bytes() -> usize {
    1 << 20
}

pub(super) fn descriptor() -> Tool {
    Tool::new(
        "assemble_docs",
        "Use assemble_docs to build one site's documentation repository from its authored manual and generated reference. One call assembles one site's configured documentation repository through the same SDK acquisition, optional native reference generation, authored-manual/reference Mintlify assembly and optional fresh retention as CLI assemble-docs. Authored manual pages remain distinct from generated artifacts and from Gazette Theme content while sharing explicit project-module navigation. Requires local=true and allow_read=true; generation, output_parent or preparation additionally requires allow_stage=true, and generation.allow_execute independently permits configured native commands. Optional preparation corresponds to CLI --preparation-json and is mutually exclusive with output_parent: it durably records exact documentation BUILD INPUT bytes in an existing journal with separate stage/history/writer permissions, never initializes a store or certifies hosted HTML. Explicit generation bounds and directory_bytes must fit reserved result capacity before effects; no partial job set is inferred. Native commands are not sandboxed. Returns exact manual/artifact/tree, generator and journal/stage custody evidence. A compact fallback is an error preserving original custody; never automatically retry, clean up or infer rollback after cancellation/lost delivery. No GitHub, signing, disclosure, certification or publication effect occurs. Run check_documentation first. Publishing the documentation stays with its publisher.",
        schemars::schema_for!(Input).ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::native_output_schema(output::schema()))
    .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = Value::Object(arguments);
    for path in [
        "/workspace",
        "/workspace/limits",
        "/workspace/limits/source",
        "/workspace/limits/syntax",
        "/workspace/limits/syntax/document",
        "/syntax",
        "/acquisition",
        "/acquisition/per_source",
        "/acquisition/per_source/content",
        "/output",
        "/generation/input",
        "/preparation/writer",
        "/preparation/writer/session",
        "/preparation/writer/session/read",
    ] {
        if value.pointer(path).is_some_and(|value| !value.is_object()) {
            return Err(invalid());
        }
    }
    if value
        .get("generation")
        .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(invalid());
    }
    if value
        .get("preparation")
        .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(invalid());
    }
    let input: Input = serde_json::from_value(value).map_err(|_| invalid())?;
    if !(1..=1 << 20).contains(&input.directory_bytes)
        || (input.preparation.is_some() && input.output_parent.is_some())
        || input
            .output_parent
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        || input.generation.as_ref().is_some_and(|generation| {
            !generation.parent.is_absolute()
                || generation.max_jobs > 128
                || !(1..=1 << 20).contains(&generation.entry_bytes)
                || !(1..=300_000).contains(&generation.timeout_ms)
                || generation.max_capture_bytes > 64 << 20
        })
    {
        return Err(invalid());
    }
    Ok(input)
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Use exact absolute parents, closed named native option objects and finite bounds from inputSchema; no documentation operation started",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(self.into_command(maximum)) {
            Ok(outcome) => deliver(outcome),
            Err(error) => match error.code() {
                "documentation_response_limit" | "documentation_response_limits" => {
                    Err(ErrorData::invalid_params(
                        "Documentation response capacity cannot preserve native custody; no operation started",
                        None,
                    ))
                }
                "documentation_response_allocation" => Err(ErrorData::internal_error(
                    "Documentation response custody could not be reserved; no operation started",
                    None,
                )),
                _ => crate::tools::failure(error.code(), error.message(), maximum),
            },
        }
    }

    pub(super) fn into_command(self, maximum: usize) -> command::Assemble {
        let generation = self.generation.map(|generation| command::Generation {
            parent: generation.parent,
            input: generation.input,
            output: self.output,
            allow_execute: generation.allow_execute,
            timeout: Duration::from_millis(generation.timeout_ms),
            max_capture_bytes: generation.max_capture_bytes,
            max_jobs: generation.max_jobs,
            directory_bytes: self.directory_bytes,
            entry_bytes: generation.entry_bytes,
        });
        let retention = if let Some(selected) = self.preparation {
            command::Retention::Preparation {
                selection: selected,
                directory_bytes: self.directory_bytes,
            }
        } else if let Some(parent) = self.output_parent {
            command::Retention::Stage {
                parent,
                directory_bytes: self.directory_bytes,
            }
        } else {
            command::Retention::None
        };
        command::Assemble {
            preview_link_bases: self.preview_link_bases,
            input: self.workspace,
            site: self.site,
            local: self.local,
            allow_read: self.allow_read,
            allow_stage: self.allow_stage,
            syntax: self.syntax,
            acquisition: self.acquisition,
            output: self.output,
            generation,
            retention,
            response_bytes: maximum,
        }
    }
}

fn deliver(outcome: command::Outcome) -> Result<CallToolResult, ErrorData> {
    let failed = outcome.is_failure();
    let bytes = outcome.into_bytes();
    let structured: Value = serde_json::from_slice(&bytes).map_err(|_| {
        ErrorData::internal_error(
            "Native encoded documentation custody was invalid; original bytes follow",
            Some(Value::Array(
                bytes.iter().copied().map(Value::from).collect(),
            )),
        )
    })?;
    let text = String::from_utf8(bytes).expect("SDK JSON encoding is UTF-8");
    let mut result = if failed {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    result.structured_content = Some(structured);
    Ok(result)
}

#[cfg(test)]
#[path = "../tests/internal/documentation.rs"]
mod tests;
