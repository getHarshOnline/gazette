//! Borrowed execution failures shared by qualification and preparation reports.

use gazette_application::build;
use serde::Serialize;

#[derive(Serialize, schemars::JsonSchema)]
struct MathInvocation<'a> {
    request: &'a str,
    response: &'a str,
    command_sha256: gazette_core::source::Sha256Digest,
    exit: gazette_core::process::Exit,
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct MathFailure<'a> {
    code: &'static str,
    completed: Vec<MathInvocation<'a>>,
    active_command_sha256: Option<gazette_core::source::Sha256Digest>,
    active_request: Option<&'a str>,
    active_response: Option<&'a [u8]>,
    active_exit: Option<gazette_core::process::Exit>,
    process_failure: Option<gazette_core::process::Failure>,
    child_custody: Option<gazette_core::process::Cleanup>,
}

pub(super) fn math(error: &build::MathGenerationError) -> MathFailure<'_> {
    let (process_failure, child_custody) = match error.cause() {
        gazette_application::build::MathGenerationCause::Native(error) => {
            (Some(error.failure), Some(error.cleanup))
        }
        _ => (
            None,
            error
                .active_exit()
                .map(|_| gazette_core::process::Cleanup::Reaped),
        ),
    };
    MathFailure {
        code: error.code(),
        completed: error
            .completed()
            .map(|(request, response, command_sha256, exit)| MathInvocation {
                request,
                response,
                command_sha256,
                exit,
            })
            .collect(),
        active_command_sha256: error.active_command_sha256(),
        active_request: error.active_request(),
        active_response: error.active_response(),
        active_exit: error.active_exit(),
        process_failure,
        child_custody,
    }
}

#[derive(Serialize, schemars::JsonSchema)]
pub(super) struct PolicyFailure<'a> {
    code: &'static str,
    invocation: Option<&'a build::PolicyInvocation>,
    exit: Option<gazette_core::process::Exit>,
    process_failure: Option<gazette_core::process::Failure>,
    child_custody: Option<gazette_core::process::Cleanup>,
    response_sha256: Option<gazette_core::source::Sha256Digest>,
    response_bytes: Option<usize>,
}

pub(super) fn policy(error: &build::PolicyCheckError) -> PolicyFailure<'_> {
    use build::PolicyCheckError;
    let mut evidence = PolicyFailure {
        code: error.code(),
        invocation: None,
        exit: None,
        process_failure: None,
        child_custody: None,
        response_sha256: None,
        response_bytes: None,
    };
    match error {
        PolicyCheckError::OutputResponse {
            invocation,
            captured,
            ..
        }
        | PolicyCheckError::CandidateResponse {
            invocation,
            captured,
            ..
        } => {
            evidence.invocation = Some(invocation);
            evidence.exit = Some(captured.exit);
            evidence.child_custody = Some(gazette_core::process::Cleanup::Reaped);
            evidence.response_sha256 =
                Some(gazette_core::source::Sha256Digest::of(&captured.stdout));
            evidence.response_bytes = Some(captured.stdout.len());
        }
        PolicyCheckError::Rejected { invocation, exit } => {
            evidence.invocation = Some(invocation);
            evidence.exit = Some(*exit);
        }
        PolicyCheckError::Execution { invocation, source } => {
            evidence.invocation = Some(invocation);
            evidence.process_failure = Some(source.failure);
            evidence.child_custody = Some(source.cleanup);
        }
        PolicyCheckError::Refused(source) => {
            evidence.process_failure = Some(source.failure);
            evidence.child_custody = Some(source.cleanup);
        }
        _ => {}
    }
    evidence
}
