//! Effect-free documentation intent preflight over supplied workspace bytes.

use gazette_cli::{
    command::{Kernel, documentation as command},
    sdk::{build, toml, workspace::Key},
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Complete workspace TOML, not a native path to open.
    source: String,
    /// Exact site whose brand-owned documentation destination is inspected.
    site: Key,
    /// Native workspace syntax bounds.
    #[serde(default)]
    syntax: toml::Limits,
}

pub(super) fn descriptor() -> Tool {
    Tool::new(
        "check_documentation",
        "Use check_documentation to learn where a site's documentation will go and in what order its manual and generated reference will appear, before assembling anything. One call resolves one site's brand-owned documentation destination and ordered authored-manual/generated-reference intent through the same target preflight as assemble-docs --check. Parses only supplied workspace bytes: no source acquisition, generator, target assembly, filesystem, process, Git, network or publication effect. If it reports a problem, fix the workspace configuration; assembly is a separate step.",
        schemars::schema_for!(Input).ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<build::DocumentationCheck>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let value = serde_json::Value::Object(arguments);
    if value.get("syntax").is_some_and(|value| !value.is_object()) {
        return Err(invalid());
    }
    serde_json::from_value(value).map_err(|_| invalid())
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid documentation-check arguments; consult inputSchema; no operation started",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(command::Check {
            source: self.source,
            site: self.site,
            syntax: self.syntax,
        }) {
            Ok(report) => crate::tools::encode(&report, false, maximum),
            Err(error) => crate::tools::failure(error.code(), error.message(), maximum),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/documentation_check.rs"]
mod tests;
