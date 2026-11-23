//! One shared owned request projection for CLI and MCP; no interface-owned release rules.

use super::super::{
    ConsumeAuthorizationOptions, DispatchAttemptOptions, Limits, PreparationSelection,
    ReconcileReleaseOptions, RegisterAuthorizationOptions,
};
use crate::store::{JournalHeader, WriterOptions, coordinator::LocalCoordinatorSelection};
use gazette_core::process::Command;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub use gazette_core::release::GitSelection;

/// Explicit native public-authority acquisition, independent of received authorization bytes.
/// The selected provider owns freshness and meaning; execution is not its admission.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorityCommand {
    /// Literal executable, arguments, working directory and complete environment.
    pub command: Command,
    /// Finite provider-specific stdin, not reconstructed from a signed release request.
    pub input: String,
    /// Explicit acknowledgement of selected native command execution.
    #[serde(default)]
    pub allow_execute: bool,
    /// Complete stdin byte ceiling, checked before any process exists.
    #[schemars(range(max = usize::MAX))]
    pub max_input_bytes: usize,
    /// Complete stdout byte ceiling; prefixes are never accepted.
    #[schemars(range(max = usize::MAX))]
    pub max_output_bytes: usize,
    /// Shared post-spawn deadline in milliseconds.
    #[schemars(range(max = u64::MAX))]
    pub timeout_ms: u64,
    /// Existing strict binding parser limits.
    #[serde(default)]
    pub limits: Limits,
}

/// Acquire one anchored writer before requesting approval for its actual reported generation.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    /// Independently retained local custody selection, never inferred from current bytes.
    pub coordinator: LocalCoordinatorSelection,
    /// Complete expected header before acquisition; never predict or sign its successor.
    pub expected: JournalHeader,
    /// Source, staging, output and backup roots that custody must not overlap.
    pub protected_roots: Vec<PathBuf>,
    /// Existing denied-by-default writer effects and native limits.
    #[serde(default)]
    pub writer: WriterOptions,
    /// Explicit anchor-state mutation permission, separate from SQL writes.
    #[serde(default)]
    pub allow_anchor_write: bool,
    /// Optional selected native Git operations; absence does not block inspection/close.
    #[serde(default)]
    pub git: Option<GitSelection>,
    /// Optional independent public-authority provider; never a cached binding.
    #[serde(default)]
    pub authority: Option<AuthorityCommand>,
    /// Independent workspace scopes and owner credentials for ordinary reviewed releases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "super::super::ReviewedSelection")]
    pub reviewed: Option<super::super::ReviewedSelection>,
}

/// Complete original authorization and ordered native preparation selection.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparedInput {
    /// Exact private received JSON bytes as a string; never normalize the enclosed JSON.
    pub authorization: String,
    /// Complete ordered selected set; the application refuses silent set reduction.
    pub selections: Vec<PreparationSelection>,
}

/// Exact plan coordinates; constructing a review does not approve or register it.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    /// Caller-selected unique release record identifier.
    pub record_id: String,
    /// Caller-selected one-use identifier within the coordinator scope.
    pub nonce: String,
    /// Explicit review time, retained in the digest the operator must confirm.
    pub approved_at: u64,
    /// Complete original preparation set in the independently selected site order.
    pub selections: Vec<PreparationSelection>,
}

/// Original reviewed bytes and separate explicit confirmation, never self-approval.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedInput {
    /// Exact original JSON returned for review; whitespace is retained in history.
    pub approval: String,
    /// Digest explicitly confirmed by the operator after reviewing the complete output.
    pub confirmed_sha256: crate::source::Sha256Digest,
    /// Current explicit invocation time; cannot precede the original review.
    pub observed_at: u64,
    /// Original full preparation selection.
    pub selections: Vec<PreparationSelection>,
}

/// One explicit operation under the already-held writer, not a hidden acquisition or retry.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Inspect current held native coordinates; does not renew authority.
    Inspect,
    /// Prepare the exact complete native set for review without granting approval.
    Review {
        /// Actual held journal generation.
        expected: JournalHeader,
        /// Caller-selected record and complete native preparation set.
        input: ReviewInput,
        /// Native verification bounds and read permissions; no registration permission.
        #[serde(default)]
        options: super::super::PreparedAuthorizationOptions,
    },
    /// Register explicit operator confirmation under independently resolved workspace scopes.
    RegisterReviewed {
        /// Actual held journal generation.
        expected: JournalHeader,
        /// Exact reviewed bytes plus separate confirmation.
        input: ReviewedInput,
        /// Registration permission and verification bounds.
        #[serde(default)]
        options: RegisterAuthorizationOptions,
    },
    /// Consume the exact reviewed set once after complete prestate reads.
    ConsumeReviewed {
        /// Actual held journal generation.
        expected: JournalHeader,
        /// Original confirmed set; no reduced selection is accepted.
        input: ReviewedInput,
        /// Independent lease, consume and read permissions.
        #[serde(default)]
        options: ConsumeAuthorizationOptions,
    },
    /// Dispatch one explicit reviewed attempt; historical replay never repeats its effect.
    DispatchReviewed {
        /// Actual held journal generation.
        expected: JournalHeader,
        /// Original complete confirmed set.
        input: ReviewedInput,
        /// Original target ordinal.
        ordinal: u32,
        /// Explicit one-based attempt coordinate.
        number: u64,
        /// Independent dispatch, read and evidence-write permissions.
        #[serde(default)]
        options: DispatchAttemptOptions,
    },
    /// Verify against newly acquired public authority and retain original pending bytes.
    Register {
        /// Exact held generation received in readiness, not a rewritten authorization fence.
        expected: JournalHeader,
        /// Complete original intent.
        input: PreparedInput,
        /// Explicit registration effects and existing validation limits.
        #[serde(default)]
        options: RegisterAuthorizationOptions,
    },
    /// Acquire fresh authority twice, hold all leases and consume exact pending intent once.
    Consume {
        /// Exact current held generation.
        expected: JournalHeader,
        /// Complete original intent.
        input: PreparedInput,
        /// Separate inspection, read, lease and consumption effects.
        #[serde(default)]
        options: ConsumeAuthorizationOptions,
    },
    /// Attempt one consumed target; exact replay is history, never a second provider call.
    Dispatch {
        /// Exact original held generation.
        expected: JournalHeader,
        /// Complete original intent.
        input: PreparedInput,
        /// Original zero-based target position.
        ordinal: u32,
        /// Explicit one-based attempt coordinate.
        #[schemars(range(max = u64::MAX))]
        number: u64,
        /// Independent observation, evidence and native dispatch permissions.
        #[serde(default)]
        options: DispatchAttemptOptions,
    },
    /// Read the complete original set and durably settle observations; no fresh approval needed.
    Reconcile {
        /// Exact current held generation; historical original fences remain unchanged.
        expected: JournalHeader,
        /// Original consumed record identifier.
        authorization_id: String,
        /// Consecutive reconciliation coordinate or exact historical replay.
        #[schemars(range(max = u64::MAX))]
        number: u64,
        /// Explicit reads and evidence writes only; no dispatch permission.
        #[serde(default)]
        options: ReconcileReleaseOptions,
    },
    /// Explicitly seal and close this lifetime, independently of release settlement.
    Close,
}

/// Shared strict transport decoding, preserving nested JSON object shape and duplicate refusal.
///
/// # Errors
/// Rejects excessive input, duplicate/unknown fields, positional objects and malformed values.
pub fn parse_open(bytes: &[u8]) -> Result<OpenRequest, &'static str> {
    parse(bytes)
}

/// Decode one bounded operation without interpreting or normalizing enclosed authorization JSON.
///
/// # Errors
/// Rejects excessive input, duplicates, unknown fields and positional object representations.
pub fn parse_operation(bytes: &[u8]) -> Result<Operation, &'static str> {
    parse(bytes)
}

pub(in crate::release) fn parse<T: serde::de::DeserializeOwned + Serialize>(
    bytes: &[u8],
) -> Result<T, &'static str> {
    if bytes.len() > 16 << 20 {
        return Err("release_session_input_limit");
    }
    let supplied = gazette_core::json::parse(bytes).map_err(|_| "release_session_request")?;
    if !supplied.is_object() {
        return Err("release_session_request");
    }
    let value: T =
        serde_json::from_value(supplied.clone()).map_err(|_| "release_session_request")?;
    let normalized = serde_json::to_value(&value).map_err(|_| "release_session_request")?;
    if !same_shape(&supplied, &normalized) {
        return Err("release_session_request");
    }
    Ok(value)
}

// Missing defaults are permitted; supplied values must retain their exact structural shape.
fn same_shape(supplied: &serde_json::Value, normalized: &serde_json::Value) -> bool {
    match (supplied, normalized) {
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => left
            .iter()
            .all(|(key, value)| right.get(key).is_some_and(|other| same_shape(value, other))),
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(a, b)| same_shape(a, b))
        }
        _ => supplied == normalized,
    }
}
