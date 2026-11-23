//! Native adapter composition and one borrowed outcome projection for all interfaces.

use super::{ObservationSessionError, ObservationSessionOptions, ObservationSessionRequest};
use gazette_application::release::{ReadbackReport, RecordObservationError};
use gazette_core::{
    release::{RemoteReadError, RemoteReadFailure},
    store::{GenerationEffect, RefObservationEffect, RefObservationReceipt},
};
use serde::{Serialize, Serializer};

/// Manage one native diagnostic observation and its journal lifetime without release effects.
/// The explicit private isolation root is always protected from journal mutation, in
/// addition to the caller's complete protected-source selection. No authority is inferred
/// from the caller, a signature, a matching destination or a stored result.
/// A selected local coordinator also protects its anchor against those roots and
/// requires independent anchor-write permission. Absence never auto-discovers a guard.
///
/// # Errors
/// Preserves refused/acquired/recorded/close-failed boundaries and native read evidence.
pub fn record_observation(
    git: &super::GitSelection,
    request: ObservationSessionRequest<'_>,
    options: ObservationSessionOptions,
) -> Result<RefObservationReceipt, ObservationSessionError> {
    if !git.is_valid() {
        return Err(ObservationSessionError::Before(
            RecordObservationError::Read(RemoteReadError::new(RemoteReadFailure::Selection)),
        ));
    }
    let mut protected = request.protected_roots.to_vec();
    protected.push(git.isolation.clone());
    let reader = gazette_adapter_git::RemoteReader {
        runner: &gazette_adapter_process::Runner,
        selection: git,
    };
    let request = ObservationSessionRequest {
        protected_roots: &protected,
        ..request
    };
    gazette_application::release::record_observation_session(
        &gazette_adapter_fs::FileSystem,
        &crate::store::coordinator::Connector {
            selection: request.coordinator,
            allow_anchor_write: options.allow_anchor_write,
        },
        &reader,
        request,
        options,
    )
}

/// One borrowed native-result projection; no copied receipt or inferred rollback.
#[derive(schemars::JsonSchema)]
#[schemars(with = "Report<'a>")]
pub struct ObservationSessionReport<'a> {
    result: &'a Result<RefObservationReceipt, ObservationSessionError>,
}

impl<'a> ObservationSessionReport<'a> {
    /// Project the complete outcome shared by direct and protocol callers.
    pub fn new(result: &'a Result<RefObservationReceipt, ObservationSessionError>) -> Self {
        Self { result }
    }
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Report<'a> {
    #[serde(rename = "observation-recorded-not-settlement")]
    Recorded {
        receipt: &'a RefObservationReceipt,
    },
    NotStarted {
        failure: Failure<'a>,
    },
    AcquisitionFailed {
        code: &'static str,
        generation: GenerationEffect,
    },
    RecordingFailed {
        failure: Failure<'a>,
        close_error: Option<&'static str>,
    },
    CloseFailed {
        receipt: &'a RefObservationReceipt,
        code: &'static str,
    },
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "phase", rename_all = "kebab-case")]
enum Failure<'a> {
    BeforeRead {
        code: &'static str,
    },
    Read {
        error: RemoteReadError,
    },
    Append {
        report: &'a ReadbackReport,
        code: &'static str,
        effect: RefObservationEffect,
    },
}

fn failure(error: &RecordObservationError) -> Failure<'_> {
    match error {
        RecordObservationError::BeforeRead(cause) => Failure::BeforeRead { code: cause.code() },
        RecordObservationError::Read(error) => Failure::Read { error: *error },
        RecordObservationError::Write { report, cause } => Failure::Append {
            report,
            code: cause.cause.code(),
            effect: cause.effect,
        },
    }
}

impl Serialize for ObservationSessionReport<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let report = match self.result {
            Ok(receipt) => Report::Recorded { receipt },
            Err(ObservationSessionError::Before(cause)) => Report::NotStarted {
                failure: failure(cause),
            },
            Err(ObservationSessionError::Acquire(cause)) => Report::AcquisitionFailed {
                code: cause.cause.code(),
                generation: cause.generation,
            },
            Err(ObservationSessionError::Record { cause, close_error }) => {
                Report::RecordingFailed {
                    failure: failure(cause),
                    close_error: close_error.as_ref().map(gazette_core::store::Error::code),
                }
            }
            Err(ObservationSessionError::Close { receipt, cause }) => Report::CloseFailed {
                receipt,
                code: cause.code(),
            },
        };
        report.serialize(serializer)
    }
}
