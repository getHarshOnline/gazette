//! One live/history matcher for checker-originated output evidence, not policy authority.

use std::{collections::BTreeSet, fmt};

use gazette_core::{process, publication::Key, source::Sha256Digest};
use serde::{Deserialize, Serialize};

use crate::build::{ReviewOutcome, ReviewPolicy};

/// Independently selected output-check requirements. Never derive these from a response
/// or silently copy the candidate review's policy. Qualification of the selected checker,
/// its dependencies and its policy source remains with the native policy owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputPolicyRequirement {
    /// Exact expected policy identity, revision and source bytes.
    pub policy: ReviewPolicy,
    /// Compact JSON digest of the complete configured command, including its executable pin.
    pub command_sha256: Sha256Digest,
    /// Complete stdout ceiling, from one byte through 64 MiB; no prefix is accepted.
    #[schemars(range(min = 1, max = 67108864))]
    pub response_bytes: usize,
}

impl OutputPolicyRequirement {
    /// Check the capture ceiling without invoking a process or reading native files.
    ///
    /// # Errors
    /// Refuses an empty or unsupported response allowance.
    pub fn validate(&self) -> Result<(), OutputResponseError> {
        if self.response_bytes == 0 || self.response_bytes > 64 << 20 {
            return Err(OutputResponseError::Limit);
        }
        Ok(())
    }

    /// Compare exact configured command bytes and require its explicit executable pin.
    /// This is a pure binding check, not executable-file verification or execution permission.
    ///
    /// # Errors
    /// Refuses invalid bounds, absent pin or an independently selected command mismatch.
    pub fn check_command(&self, command: &process::Command) -> Result<(), super::PolicyCheckError> {
        self.validate()
            .map_err(super::PolicyCheckError::OutputRequirement)?;
        if command.executable.is_none() || super::command_digest(command)? != self.command_sha256 {
            return Err(super::PolicyCheckError::OutputRequirement(
                OutputResponseError::Binding,
            ));
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Response {
    format: String,
    request_sha256: Sha256Digest,
    policy: ReviewPolicy,
    outcome: ReviewOutcome,
    evidence: Vec<Key>,
}

/// Exact private stdout matched against independently selected requirements and the delivered
/// request. Matching does not authenticate the evaluator or grant publication permission.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MatchedOutputResponse {
    status: &'static str,
    requirement: OutputPolicyRequirement,
    response_sha256: Sha256Digest,
    response_json: String,
}

impl MatchedOutputResponse {
    /// Original stdout text, including whitespace and evidence order.
    pub fn response_json(&self) -> &str {
        &self.response_json
    }

    /// Independent expectations used by this invocation, not newly inferred authority.
    pub fn requirement(&self) -> &OutputPolicyRequirement {
        &self.requirement
    }
}

pub(crate) fn match_output_response(
    request_sha256: Sha256Digest,
    bytes: &[u8],
    requirement: &OutputPolicyRequirement,
) -> Result<MatchedOutputResponse, OutputResponseError> {
    requirement.validate()?;
    if bytes.len() > requirement.response_bytes {
        return Err(OutputResponseError::Limit);
    }
    let value = gazette_core::json::parse(bytes).map_err(|_| OutputResponseError::Shape)?;
    let response: Response =
        serde_json::from_value(value.clone()).map_err(|_| OutputResponseError::Shape)?;
    // Duplicate-safe decoding plus exact named-object round trip rejects positional structs,
    // unknown fields and alternate enum shapes at every response boundary.
    if serde_json::to_value(&response).map_err(|_| OutputResponseError::Shape)? != value
        || response.format != "gazette-output-response/1"
    {
        return Err(OutputResponseError::Shape);
    }
    if response.request_sha256 != request_sha256 || response.policy != requirement.policy {
        return Err(OutputResponseError::Binding);
    }
    if response.evidence.is_empty()
        || response.evidence.iter().collect::<BTreeSet<_>>().len() != response.evidence.len()
    {
        return Err(OutputResponseError::Evidence);
    }
    if response.outcome != ReviewOutcome::Allow {
        return Err(OutputResponseError::Disposition);
    }
    Ok(MatchedOutputResponse {
        status: "checker-reported-policy-matched-unverified",
        requirement: requirement.clone(),
        response_sha256: Sha256Digest::of(bytes),
        response_json: std::str::from_utf8(bytes)
            .map_err(|_| OutputResponseError::Shape)?
            .to_owned(),
    })
}

/// Redacted output-response refusal. Original stdout stays in the enclosing invocation error.
#[derive(Clone, Copy, Debug)]
pub enum OutputResponseError {
    /// Unsupported capture ceiling or excessive complete response.
    Limit,
    /// Invalid closed named JSON or unsupported format.
    Shape,
    /// Command, delivered request or policy coordinates differ from independent selection.
    Binding,
    /// Empty or repeated policy-native evidence references.
    Evidence,
    /// The checker reported hold or exclude; output is not silently changed.
    Disposition,
    /// Complete captured output accompanied a nonzero or abnormal exit.
    Exit,
}

impl OutputResponseError {
    /// Stable class without response values or private checker coordinates.
    pub fn code(self) -> &'static str {
        match self {
            Self::Limit => "output_policy_response_limit",
            Self::Shape => "output_policy_response_shape",
            Self::Binding => "output_policy_response_binding",
            Self::Evidence => "output_policy_response_evidence",
            Self::Disposition => "output_policy_response_disposition",
            Self::Exit => "output_policy_response_exit",
        }
    }
}
impl fmt::Display for OutputResponseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for OutputResponseError {}
