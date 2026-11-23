//! Checker-originated candidate decisions, bound before rendering and never promoted to approval.

use gazette_core::{process, source::Sha256Digest};
use serde::{Deserialize, Serialize};

use super::{PolicyCheckError, PolicyInvocation, command_digest};
use crate::build::{
    MatchedReviewResponse, PublicationReview, ReviewOutcome, ReviewPolicy, ReviewResponseError,
    ReviewResponseLimits,
};

/// Independently selected policy and native evaluator, not values inferred from stdout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CandidatePolicyRequirement {
    /// Exact policy identity, revision and source bytes.
    pub policy: ReviewPolicy,
    /// Digest of the complete configured command, which must include an executable pin.
    pub command_sha256: Sha256Digest,
}

impl CandidatePolicyRequirement {
    /// Bind the native evaluator before execution without opening its executable.
    ///
    /// # Errors
    /// Refuses a missing executable pin or a command different from independent selection.
    pub fn check_command(&self, command: &process::Command) -> Result<(), PolicyCheckError> {
        if command.executable.is_none() || command_digest(command)? != self.command_sha256 {
            return Err(PolicyCheckError::Response(ReviewResponseError::Binding(
                "candidate command",
            )));
        }
        Ok(())
    }
}

pub(super) fn capture(
    runner: &dyn gazette_ports::process::CaptureRunner,
    command: &process::Command,
    options: process::Options,
    input: &[u8],
    review: &PublicationReview,
    requirement: &CandidatePolicyRequirement,
    limits: ReviewResponseLimits,
) -> Result<(PolicyInvocation, MatchedReviewResponse, usize), PolicyCheckError> {
    requirement.check_command(command)?;
    let invocation = PolicyInvocation {
        request_sha256: Sha256Digest::of(input),
        command_sha256: command_digest(command)?,
    };
    let captured = runner
        .capture(process::CaptureRequest {
            request: process::Request {
                command,
                input,
                options,
            },
            max_output_bytes: limits.bytes,
            max_stderr_bytes: None,
        })
        .map_err(|source| PolicyCheckError::Execution { invocation, source })?;
    let matched = if captured.exit.code == Some(0) {
        review
            .match_captured_response(&captured.stdout, &requirement.policy, limits)
            .and_then(|response| {
                if response
                    .decisions()
                    .iter()
                    .any(|decision| decision.outcome != ReviewOutcome::Allow)
                {
                    Err(ReviewResponseError::Disposition)
                } else {
                    Ok(response)
                }
            })
    } else {
        Err(ReviewResponseError::Exit)
    };
    let bytes = captured.stdout.len();
    let response = matched.map_err(|source| PolicyCheckError::CandidateResponse {
        invocation,
        captured,
        source,
    })?;
    Ok((invocation, response, bytes))
}
