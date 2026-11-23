//! Private diagnostic probing of an independently pinned imported output tree.

use gazette_cli::command::{Kernel, rendered_output as command};
use gazette_cli::sdk::{
    build::{self, disclosure_input::ScanLimits},
    source, toml,
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Must be true; grants no permission beyond host-native read access.
    allow_native_reads: bool,
    /// Explicit absolute imported-output tree; never rendered or rewritten.
    #[schemars(regex(pattern = "^/"))]
    root: PathBuf,
    /// Independently retained complete tree identity; never observed as a fallback.
    expected_tree: source::TreeIdentity,
    /// Private exact gazette-disclosure-output-input/1 JSON; no literals are logged.
    rules_json: String,
    /// Complete private rules JSON byte ceiling before decoding or native reads.
    #[serde(default = "rules_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_rules_bytes: usize,
    /// Independent complete native capture ceilings; native defaults apply.
    #[serde(default)]
    source_limits: source::Limits,
    /// Shared scanner overrides as CLI; all other native bounds retain defaults.
    #[serde(default)]
    scan_limits: ScanLimits,
}

fn rules_bytes() -> usize {
    toml::Limits::default().source_bytes
}

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(Input);
    if let Some(property) = schema
        .ensure_object()
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut("rules_json"))
        .and_then(Value::as_object_mut)
    {
        property.insert(
            "contentMediaType".into(),
            Value::String("application/json".into()),
        );
        property.insert(
            "contentSchema".into(),
            build::disclosure_input::output_schema().into(),
        );
    }
    Tool::new("probe_output", "Use probe_output to look for private literals that must not appear in an imported output tree. One call reads an explicitly permitted, independently pinned imported output tree and runs the same private literal disclosure probe as CLI probe-output. No rendering, staging, writes, network, browser execution, policy approval or release. Imported output has no authenticated producer media map: existing suffix-selected coverage remains explicit. Reports always mean partial-evidence, even with no findings; unknown formats may remain raw-only. Rules and reports are private operator evidence: paths and rule identifiers can themselves be confidential. Invalid rules and capture failures are redacted; output contains no separate literal or payload snippet. Report findings to the publisher, who decides whether the output may be published. No findings is not clearance.", schema.ensure_object().clone())
        .with_raw_output_schema(crate::tools::output_schema::<build::DisclosureProbe<'static>>())
        .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = Value::Object(arguments);
    for pointer in ["/expected_tree", "/source_limits", "/scan_limits"] {
        if value
            .pointer(pointer)
            .is_some_and(|value| !value.is_object())
        {
            return Err(invalid());
        }
    }
    let input: Input = serde_json::from_value(value).map_err(|_| invalid())?;
    if !input.allow_native_reads || !input.root.is_absolute() {
        return Err(invalid());
    }
    Ok(input)
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid probe_output selection: require explicit native-read permission, absolute root, exact tree identity and named limit objects; consult inputSchema",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(command::ProbeOutput {
            root: self.root,
            expected_tree: self.expected_tree,
            source_limits: self.source_limits,
            rules_json: self.rules_json,
            max_rules_bytes: self.max_rules_bytes,
            scan_limits: self.scan_limits.native(),
        }) {
            Ok(report) => crate::tools::encode(&report, false, maximum),
            Err(error) => crate::tools::failure(error.code(), error.message(), maximum),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/probe_output.rs"]
mod tests;
