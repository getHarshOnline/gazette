//! Pure supplied-byte verification; neither key admission nor release execution.

use gazette_cli::sdk::release;
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use gazette_cli::command::{self, Kernel};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    /// Complete exact UTF-8 authorization JSON. A string preserves duplicate keys,
    /// numeric spellings and transport identity for the native verifier to check.
    authorization_json: String,
    /// Complete independently supplied binding JSON, including admitted public
    /// keys, observed time and exact expectations. Never derive trust from the body.
    binding_json: String,
    /// Native per-input, canonical-output and verification-work limits. Omitted
    /// fields use the shared verifier defaults; MCP frame/result limits also apply.
    #[serde(default)]
    limits: release::Limits,
}

pub(super) fn descriptor() -> Tool {
    Tool::new(
        "verify_release_authorization",
        "Use verify_release_authorization to check a release authorization's signature against its binding, without touching any journal or key store. One call verifies exact supplied authorization JSON against independently supplied binding JSON using Gazette's strict signature verifier. Both inputs are strings to preserve original bytes and reject duplicate keys natively. Returns digest-only evidence, not key admission, freshness, native-state verification, authorization issuance, consumption or release. No files, clock, store, network or private keys are accessed. Issuing, consuming and acting on an authorization stay with the publisher.",
        schemars::schema_for!(Input).ensure_object().clone(),
    )
    .with_raw_output_schema(crate::tools::output_schema::<release::VerificationReport>())
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    if arguments
        .get("limits")
        .is_some_and(|value| !value.is_object())
    {
        return Err(ErrorData::invalid_params("limits must be an object", None));
    }
    serde_json::from_value(Value::Object(arguments)).map_err(|_| {
        ErrorData::invalid_params(
            "Invalid release verification arguments; consult inputSchema; no verification or release effects",
            None,
        )
    })
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match Kernel::new().execute(command::release::VerifyAuthorization {
            authorization_json: self.authorization_json.into_bytes(),
            binding_json: self.binding_json.into_bytes(),
            limits: self.limits,
        }) {
            Ok(report) => crate::tools::encode(&report, false, maximum),
            Err(error) => crate::tools::failure(
                error.code(),
                "No verification report produced; no release effects",
                maximum,
            ),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/release.rs"]
mod tests;
