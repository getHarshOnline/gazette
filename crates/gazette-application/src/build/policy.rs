//! Workspace-owned policy execution over retained inputs and output, not admission authority.

mod candidate;
mod input;
mod output;
mod output_response;
mod set;

pub use candidate::CandidatePolicyRequirement;
pub(crate) use input::{PolicyDocument, policy_input};
pub use output::{OutputPolicyCheck, OutputPolicyEvidence};
pub(crate) use output_response::match_output_response;
pub use output_response::{MatchedOutputResponse, OutputPolicyRequirement, OutputResponseError};
pub(super) use set::check_sites;
pub use set::{CandidateCheckFailure, CandidateCheckOptions};

use std::fmt;

use gazette_core::{process, render, source::Sha256Digest};
use serde::Serialize;

use super::{
    AcquiredInputs, MatchedReviewResponse, PublicationReview, ReviewError, ReviewLimits,
    ReviewOutcome, ReviewResponseError, ReviewResponseLimits, ReviewSubmission,
    SelectedPublication, serialization::encode,
};

/// Exact private invocation coordinates. The command digest includes an explicitly selected
/// executable pin when present; it never infers one or authenticates policy authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PolicyInvocation {
    /// Complete retained candidate or output request delivered as compact JSON on stdin.
    pub request_sha256: Sha256Digest,
    /// Compact JSON identity of the path, optional executable pin, arguments, directory and environment.
    pub command_sha256: Sha256Digest,
}

/// Closed outcome emitted after a complete configured candidate-policy invocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum PolicyCheckStatus {
    /// The checker exited successfully; evaluator authority remains unverified.
    #[serde(rename = "policy-check-passed-unverified")]
    PassedUnverified,
}

/// The configured checker returned zero after complete input delivery.
/// This cannot be deserialized into approval or consumed as a certified artifact or release token.
/// Checker authenticity, policy evidence and final-output acceptance remain separate requirements.
#[derive(Debug, Serialize)]
pub struct PolicyCheck {
    status: PolicyCheckStatus,
    invocation: PolicyInvocation,
    review: PublicationReview,
    response: Option<MatchedReviewResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    requirement: Option<CandidatePolicyRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_source: Option<PolicyDocument>,
}

impl PolicyCheck {
    /// Exact retained request used by this invocation; never a native reread.
    pub fn review(&self) -> &PublicationReview {
        &self.review
    }

    /// Bound request and command configuration, without exposing their private payloads.
    pub fn invocation(&self) -> PolicyInvocation {
        self.invocation
    }

    /// Exact response supplied independently or captured from the selected checker. This is
    /// private evaluator evidence, not authentication or publication permission.
    pub fn response(&self) -> Option<&MatchedReviewResponse> {
        self.response.as_ref()
    }

    pub(super) fn requirement(&self) -> Option<&CandidatePolicyRequirement> {
        self.requirement.as_ref()
    }

    pub(super) fn policy_source(&self) -> Option<&PolicyDocument> {
        self.policy_source.as_ref()
    }
}

/// One private checker envelope for both one-site and complete-set execution.
#[derive(Serialize)]
struct CandidateRequest<'a, R, S> {
    format: &'static str,
    review: &'a R,
    response: &'a S,
    #[serde(skip_serializing_if = "Option::is_none")]
    requirement: Option<&'a CandidatePolicyRequirement>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_source: Option<&'a PolicyDocument>,
}

pub(crate) fn encode_candidate_request(
    review: &impl Serialize,
    response: &impl Serialize,
    requirement: Option<&CandidatePolicyRequirement>,
    policy_source: Option<&PolicyDocument>,
    maximum: usize,
) -> Result<Vec<u8>, render::Error> {
    encode(
        &CandidateRequest {
            format: "gazette-candidate-policy/1",
            review,
            response,
            requirement,
            policy_source,
        },
        maximum,
        "candidate policy stdin bytes",
    )
}

fn match_response(
    review: &PublicationReview,
    submission: Option<&ReviewSubmission>,
    limits: ReviewResponseLimits,
) -> Result<Option<MatchedReviewResponse>, PolicyCheckError> {
    let response = submission
        .map(|submission| {
            review.match_response(
                submission.response_json.as_bytes(),
                &submission.binding,
                limits,
            )
        })
        .transpose()
        .map_err(PolicyCheckError::Response)?;
    if response.as_ref().is_some_and(|response| {
        response
            .decisions()
            .iter()
            .any(|decision| decision.outcome != ReviewOutcome::Allow)
    }) {
        return Err(PolicyCheckError::Disposition);
    }
    Ok(response)
}

impl SelectedPublication<'_, '_> {
    /// Invoke the workspace-selected checker with exact candidate review bytes on stdin.
    /// Permission and execution bounds are checked before projection. Configuration omission
    /// is explicit: callers requesting this operation do not receive a synthetic success.
    /// No response body, policy schema or domain rule is interpreted by Gazette.
    ///
    /// # Errors
    /// Refuses missing configuration/permission, incomplete review, execution failure and nonzero
    /// or abnormal exit. After spawn, failures retain invocation identity and direct-child custody;
    /// no failure proves absence of external effects or authorizes automatic retry.
    pub fn check_policy(
        &self,
        runner: &impl gazette_ports::process::Runner,
        options: process::Options,
        limits: ReviewLimits,
        raster: &gazette_core::raster::Inspect<'_>,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<PolicyCheck, PolicyCheckError> {
        let command = command(self.inputs(), options)?;
        let review = self
            .review(limits, raster, markdown)
            .map_err(PolicyCheckError::Review)?;
        let input = encode_candidate_request(
            &review,
            &None::<&MatchedReviewResponse>,
            None,
            None,
            options.max_input_bytes,
        )
        .map_err(PolicyCheckError::Encoding)?;
        let invocation = invoke(runner, command, options, &input)?;
        Ok(PolicyCheck {
            status: PolicyCheckStatus::PassedUnverified,
            invocation,
            review,
            response: None,
            requirement: None,
            policy_source: None,
        })
    }
}

pub(super) fn command(
    inputs: &AcquiredInputs,
    options: process::Options,
) -> Result<&process::Command, PolicyCheckError> {
    let command = inputs
        .configuration()
        .policy_command()
        .map_err(PolicyCheckError::Binding)?
        .ok_or(PolicyCheckError::Unconfigured)?;
    options
        .validate(command, &[])
        .map_err(PolicyCheckError::Refused)?;
    Ok(command)
}

fn invoke(
    runner: &(impl gazette_ports::process::Runner + ?Sized),
    command: &process::Command,
    options: process::Options,
    input: &[u8],
) -> Result<PolicyInvocation, PolicyCheckError> {
    let invocation = PolicyInvocation {
        request_sha256: Sha256Digest::of(input),
        command_sha256: command_digest(command)?,
    };
    let exit = runner
        .run(process::Request {
            command,
            input,
            options,
        })
        .map_err(|source| PolicyCheckError::Execution { invocation, source })?;
    if exit.code != Some(0) {
        return Err(PolicyCheckError::Rejected { invocation, exit });
    }
    Ok(invocation)
}

fn command_digest(command: &process::Command) -> Result<Sha256Digest, PolicyCheckError> {
    encode(command, 1 << 20, "policy command bytes")
        .map(|bytes| Sha256Digest::of(&bytes))
        .map_err(PolicyCheckError::Encoding)
}

/// The configured policy invocation did not establish a complete zero-exit result.
#[derive(Debug)]
pub enum PolicyCheckError {
    /// A complete check failed the non-fabricable candidate-admission transition.
    Admission(super::AdmissionError),
    /// Selected policy bytes are absent, oversized or differ from independent expectations.
    PolicySource(&'static str),
    /// No workspace checker was selected; standalone local rendering remains independent.
    Unconfigured,
    /// Explicit command coordinates could not be bound from retained configuration.
    Binding(crate::configuration::RuntimeBindingError),
    /// Permission or command/options failed before projection or spawning.
    Refused(process::Error),
    /// The same candidate review used by inspection could not be completed.
    Review(ReviewError),
    /// Supplied evaluator evidence did not match the exact retained candidate.
    Response(ReviewResponseError),
    /// Original evaluator stdout and exit survive any semantic refusal after capture.
    CandidateResponse {
        /// Exact delivered request and selected evaluator.
        invocation: PolicyInvocation,
        /// Complete captured bytes and native exit; may contain private information.
        captured: process::Captured,
        /// Redacted response-matching failure.
        source: ReviewResponseError,
    },
    /// Independent output-check requirements failed before native execution.
    OutputRequirement(OutputResponseError),
    /// A complete native capture failed matching; retain original bytes and exit, never retry.
    OutputResponse {
        /// Exact delivered request and configured command.
        invocation: PolicyInvocation,
        /// Original complete stdout and reaped-child exit; may contain private data.
        captured: process::Captured,
        /// Redacted matching failure.
        source: OutputResponseError,
    },
    /// A requested occurrence was held or excluded. No silent selection repair occurs.
    Disposition,
    /// Complete bounded input or command serialization failed before spawning.
    Encoding(render::Error),
    /// Native execution failed; exact intent remains available even when custody is uncertain.
    Execution {
        /// Exact requested invocation.
        invocation: PolicyInvocation,
        /// Original classified native failure and direct-child custody.
        source: process::Error,
    },
    /// A normal nonzero or abnormal native exit, never repaired into policy acceptance.
    Rejected {
        /// Exact delivered request and selected command.
        invocation: PolicyInvocation,
        /// Native exit observation; stdout/stderr are not policy responses.
        exit: process::Exit,
    },
}

impl PolicyCheckError {
    /// Stable diagnostic class, without private configuration or review payloads.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Admission(error) => error.code(),
            Self::PolicySource(_) => "policy_source",
            Self::Unconfigured => "policy_unconfigured",
            Self::Binding(error) => error.code(),
            Self::Refused(error) | Self::Execution { source: error, .. } => error.code(),
            Self::Review(error) => error.code(),
            Self::Response(error) | Self::CandidateResponse { source: error, .. } => error.code(),
            Self::OutputRequirement(error) | Self::OutputResponse { source: error, .. } => {
                error.code()
            }
            Self::Disposition => "policy_review_disposition",
            Self::Encoding(_) => "policy_encoding",
            Self::Rejected { .. } => "policy_rejected",
        }
    }
}

impl fmt::Display for PolicyCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Self::Binding(error) = self {
            return error.fmt(f);
        }
        if let Self::PolicySource(reason) = self {
            return write!(
                f,
                "policy_source: {reason}; no checker or preparation success"
            );
        }
        write!(
            f,
            "{}: no complete configured policy-check success",
            self.code()
        )
    }
}

impl std::error::Error for PolicyCheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Admission(error) => Some(error),
            Self::Binding(error) => Some(error),
            Self::Refused(error) | Self::Execution { source: error, .. } => Some(error),
            Self::Review(error) => Some(error),
            Self::Response(error) | Self::CandidateResponse { source: error, .. } => Some(error),
            Self::OutputRequirement(error) | Self::OutputResponse { source: error, .. } => {
                Some(error)
            }
            Self::Encoding(error) => Some(error),
            Self::PolicySource(_)
            | Self::Unconfigured
            | Self::Disposition
            | Self::Rejected { .. } => None,
        }
    }
}
