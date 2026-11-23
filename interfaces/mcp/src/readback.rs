//! Explicit native ref observation through the same SDK used by the CLI.

use std::time::Duration;

use gazette_cli::command::{Kernel, observation};
use gazette_cli::sdk::release;
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Exact ordered target-array JSON; a string preserves duplicate keys and
    /// numeric spellings for Gazette's native complete-selection validator.
    targets_json: String,
    /// Fixed native Git executable, isolation and optional denied-by-default HTTPS helper.
    git: release::GitSelection,
    /// Explicit acknowledgement of native provider execution and destination reads.
    allow_read: bool,
    /// Separately allow supported network destinations; local reads remain available.
    #[serde(default)]
    allow_network: bool,
    /// Per-destination native child deadline, not a whole-batch deadline.
    #[serde(default = "default_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    timeout_ms: u64,
    /// Native response ceiling per destination; zero permits no response bytes.
    #[serde(default = "default_response_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_response_bytes: usize,
    /// Shared strict target parsing and canonicalization limits. The MCP frame,
    /// worker and complete result ceilings also apply independently.
    #[serde(default)]
    limits: release::Limits,
}

fn default_timeout_ms() -> u64 {
    u64::try_from(release::RemoteReadOptions::default().timeout.as_millis())
        .expect("native bounded timeout fits milliseconds")
}

fn default_response_bytes() -> usize {
    release::RemoteReadOptions::default().max_response_bytes
}

pub(super) fn descriptor() -> Tool {
    Tool::new(
        "observe_release_targets",
        "Use observe_release_targets to learn what each release destination's Git refs currently advertise. One call validates a complete exact target-array JSON string, then reads every selected ref through the explicitly selected trusted native Git executable and existing empty private isolation directory. Requires allow_read; supported network reads additionally require allow_network. HTTPS authentication can use only the explicitly selected absolute credential helper when its separate allow_credentials flag is true; Gazette accepts no secret and never falls back to ambient Git configuration, tokens or askpass. Returns every ordinal, including unavailable or indeterminate observations; missing advertisements never prove unborn state. This is sequential readback, not an atomic snapshot, signing, store access, push, consumption or settlement. Native deadlines apply per destination; caller cancellation does not undo or necessarily stop admitted blocking work. Input/output and concurrent-worker ceilings remain enforced. Whether an unavailable or indeterminate destination blocks a release stays with the publisher.",
        schemars::schema_for!(Input).ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<release::ReadbackReport>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(true),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    if arguments
        .get("limits")
        .is_some_and(|value| !value.is_object())
    {
        return Err(ErrorData::invalid_params("limits must be an object", None));
    }
    let input: Input = serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
    if !input.git.is_valid() {
        return Err(invalid());
    }
    Ok(input)
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid ref-observation arguments; consult inputSchema; no provider reads performed",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let options = release::RemoteReadOptions {
            allow_read: self.allow_read,
            allow_network: self.allow_network,
            timeout: Duration::from_millis(self.timeout_ms),
            max_response_bytes: self.max_response_bytes,
        };
        match Kernel::new().execute(observation::ObserveTargets {
            targets_json: self.targets_json.into_bytes(),
            git: self.git,
            limits: self.limits,
            options,
        }) {
            Ok(report) => crate::tools::encode(&report, false, maximum),
            Err(error) => crate::tools::failure(
                error.code(),
                "No complete ref-observation report; no push or consumption",
                maximum,
            ),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/readback.rs"]
mod tests;
