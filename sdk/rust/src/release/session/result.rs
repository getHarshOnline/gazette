//! Loss-aware transport projection of existing native receipts and failed phases.

use crate::{process, release, store};
use serde::Serialize;

/// Native refusal classification; never converts missing delivery into rollback or retry.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Cause {
    /// Shared session selection or lifecycle refusal before the requested operation.
    Selection {
        /// Stable redacted code.
        code: &'static str,
    },
    /// Native journal/custody verification failed.
    Store {
        /// Existing native code.
        code: &'static str,
    },
    /// Independently selected current authority could not be obtained.
    Authority {
        /// Existing authority code.
        code: &'static str,
    },
    /// Existing pure/native preparation or target verification failed.
    Verification {
        /// Existing verifier code.
        code: &'static str,
    },
    /// Native destination observation refused, preserving direct-child custody.
    Read {
        /// Existing complete redacted failure.
        error: release::RemoteReadError,
    },
    /// Native dispatch selection refused before durable start.
    DispatchSelection {
        /// Existing complete redacted dispatch failure.
        error: release::DispatchError,
    },
    /// Immutable registration write did not produce a complete receipt.
    Registration {
        /// Existing journal code.
        code: &'static str,
        /// Conservative native write boundary.
        effect: store::ReleaseAuthorizationEffect,
    },
    /// One journal append failed; earlier returned/acquired evidence remains separate.
    Write {
        /// Existing journal code.
        code: &'static str,
        /// Lease, consumption, start, finish or reconciliation append.
        append: &'static str,
        /// Conservative native write boundary.
        effect: store::ReleaseWriteEffect,
    },
}

impl From<release::AuthorityError> for Cause {
    fn from(error: release::AuthorityError) -> Self {
        Self::Authority {
            code: match error {
                release::AuthorityError::Unavailable => "release_authority_unavailable",
                release::AuthorityError::Invalid => "release_authority_invalid",
            },
        }
    }
}

/// Evidence acquired before an operation failed; not necessarily durably retained.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// No additional phase evidence was supplied by the native operation.
    None,
    /// Leases/readbacks survive a failed pending-to-consumed transition.
    Consumption {
        /// Complete acquired lease set, if returned.
        leases: Option<Box<store::ReleaseLeases>>,
        /// Complete sequential pre-state observations, if returned.
        observations: Option<Box<release::ReadbackReport>>,
    },
    /// Actual dispatch boundaries survive failed durable result delivery.
    Attempt {
        /// Native pre-state read, including conflict/unavailability.
        prestate: Option<Box<release::TargetReadback>>,
        /// Successfully returned durable start, if any.
        started: Option<Box<store::ReleaseAttemptStart>>,
        /// Actual native effect/read-back result, not necessarily durably recorded.
        observed: Option<Box<store::ReleaseAttemptResult>>,
    },
    /// Complete acquired reconciliation observation, not necessarily durably recorded.
    Reconciliation {
        /// Actual observed original set and frontier.
        observed: Option<Box<store::ReleaseReconciliation>>,
    },
}

/// Redacted evidence from the most recent acquisition, not publisher approval.
// This public projection deliberately preserves the selected native provider's wire shape
// without exposing that concrete adapter as part of the SDK contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
#[schemars(rename = "Observation")]
pub enum AuthorityObservation {
    /// No complete successful capture was obtained. Custody covers the direct child only.
    ExecutionFailed {
        /// Stable transport failure without command, environment or output disclosure.
        failure: process::Failure,
        /// Known direct-child custody; never proof of no external effects.
        cleanup: process::Cleanup,
    },
    /// Full capture completed, but the provider did not report successful acquisition.
    NonzeroExit {
        /// Reaped direct-child exit; a missing code is not success.
        exit: process::Exit,
        /// Complete captured stdout size, without its possibly private bytes.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
    /// The provider exited successfully, but its complete output was not a strict binding.
    InvalidBinding {
        /// Existing strict release-parser diagnostic code.
        code: &'static str,
        /// Complete captured stdout size; output contents are not retained.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
    /// Complete successful output decoded as the native binding shape, not admitted authority.
    Acquired {
        /// Complete captured stdout size; no binding or output is retained in the source.
        #[schemars(range(max = usize::MAX))]
        stdout_bytes: usize,
    },
}

impl AuthorityObservation {
    pub(super) fn from_native(observation: gazette_adapter_authority::Observation) -> Self {
        use gazette_adapter_authority::Observation as Native;
        match observation {
            Native::ExecutionFailed { failure, cleanup } => {
                Self::ExecutionFailed { failure, cleanup }
            }
            Native::NonzeroExit { exit, stdout_bytes } => Self::NonzeroExit { exit, stdout_bytes },
            Native::InvalidBinding { code, stdout_bytes } => {
                Self::InvalidBinding { code, stdout_bytes }
            }
            Native::Acquired { stdout_bytes } => Self::Acquired { stdout_bytes },
        }
    }
}

/// Lossless native phase projection with optional public-authority transport evidence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct Failure {
    /// Failed phase and its stable diagnostic.
    pub cause: Cause,
    /// Earlier acquired evidence, preserving uncertainty.
    pub evidence: Evidence,
    /// Last acquisition attempt only; never current publisher truth or approval.
    pub authority: Option<AuthorityObservation>,
}

impl Failure {
    pub(in crate::release) fn selection(code: &'static str) -> Self {
        Self::new(Cause::Selection { code })
    }

    pub(super) fn new(cause: Cause) -> Self {
        Self {
            cause,
            evidence: Evidence::None,
            authority: None,
        }
    }
}

/// Distinguish refused acquisition from an acquired generation whose readiness failed.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum OpenError {
    /// Refused before acquiring either native reservation.
    Before {
        /// Stable redacted selection/permission code.
        code: &'static str,
    },
    /// Native acquisition failed; anchor effects may precede the generation attempt.
    Acquisition {
        /// Existing native code.
        code: &'static str,
        /// Conservative journal generation-write evidence only.
        generation: store::GenerationEffect,
    },
    /// Writer acquisition succeeded but native readiness could not be returned.
    Readiness {
        /// Existing inspection failure.
        code: &'static str,
        /// Separate explicit close failure; None means close returned successfully.
        close_error: Option<&'static str>,
    },
}

/// Exact native operation result, separate from response delivery, approval and publication.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    /// Current held native journal inspection.
    Inspected {
        /// Native result, not cached session state.
        inspection: store::JournalInspection,
    },
    /// Complete verified native output ready for operator review; no approval is granted.
    Reviewed {
        /// Exact document whose digest requires separate explicit confirmation.
        approval: release::ReviewedApproval,
    },
    /// Original pending bytes registered, not consumed.
    Registered {
        /// Exact independently read-back native receipt.
        receipt: store::ReleaseAuthorizationReceipt,
    },
    /// Exact pending intent consumed once, not dispatched.
    Consumed {
        /// Original complete set and consumed authority.
        receipt: store::ReleaseConsumptionReceipt,
    },
    /// Returned native attempt; provider success is not complete-set settlement.
    Dispatched {
        /// Durable original start and result, including exact replay.
        attempt: store::ReleaseAttempt,
    },
    /// Complete original set reconciled through existing native settlement rules.
    Reconciled {
        /// Exact durable receipt; may still require reconciliation.
        receipt: store::ReleaseReconciliationReceipt,
    },
    /// Explicit native lifetime close succeeded; not a release-success assertion.
    Closed,
    /// Failure preserves earlier evidence and never implies automatic retry.
    Failed {
        /// Phase-specific native evidence.
        failure: Failure,
    },
}

impl Outcome {
    /// Whether the requested operation returned native failure, regardless of earlier effects.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

impl From<Failure> for Outcome {
    fn from(failure: Failure) -> Self {
        Self::Failed { failure }
    }
}

impl From<store::Error> for Failure {
    fn from(error: store::Error) -> Self {
        Self::new(Cause::Store { code: error.code() })
    }
}

impl From<release::RegisterAuthorizationError> for Failure {
    fn from(error: release::RegisterAuthorizationError) -> Self {
        Self::new(match error {
            release::RegisterAuthorizationError::Verification(error) => {
                Cause::Verification { code: error.code() }
            }
            release::RegisterAuthorizationError::Store(error) => Cause::Registration {
                code: error.cause.code(),
                effect: error.effect,
            },
        })
    }
}

impl From<release::ConsumeAuthorizationError> for Failure {
    fn from(error: release::ConsumeAuthorizationError) -> Self {
        use release::ConsumeFailure as F;
        Self {
            cause: match error.cause {
                F::Store(error) => Cause::Store { code: error.code() },
                F::Authority(error) => error.into(),
                F::Verification(error) => Cause::Verification { code: error.code() },
                F::Read(error) => Cause::Read { error },
                F::Lease(error) => Cause::Write {
                    code: error.cause.code(),
                    append: "lease",
                    effect: error.effect,
                },
                F::Consumption(error) => Cause::Write {
                    code: error.cause.code(),
                    append: "consumption",
                    effect: error.effect,
                },
            },
            evidence: Evidence::Consumption {
                leases: error.leases,
                observations: error.observations,
            },
            authority: None,
        }
    }
}

impl From<release::DispatchAttemptError> for Failure {
    fn from(error: release::DispatchAttemptError) -> Self {
        use release::DispatchAttemptFailure as F;
        Self {
            cause: match error.cause {
                F::Store(error) => Cause::Store { code: error.code() },
                F::Authority(error) => error.into(),
                F::Verification(error) => Cause::Verification { code: error.code() },
                F::Read(error) => Cause::Read { error },
                F::Selection(error) => Cause::DispatchSelection { error },
                F::Start(error) => Cause::Write {
                    code: error.cause.code(),
                    append: "start",
                    effect: error.effect,
                },
                F::Finish(error) => Cause::Write {
                    code: error.cause.code(),
                    append: "finish",
                    effect: error.effect,
                },
            },
            evidence: Evidence::Attempt {
                prestate: error.prestate,
                started: error.started,
                observed: error.observed,
            },
            authority: None,
        }
    }
}

impl From<release::ReconcileReleaseError> for Failure {
    fn from(error: release::ReconcileReleaseError) -> Self {
        use release::ReconcileReleaseFailure as F;
        Self {
            cause: match error.cause {
                F::Store(error) => Cause::Store { code: error.code() },
                F::Targets(error) => Cause::Verification { code: error.code() },
                F::Read(error) => Cause::Read { error },
                F::Record(error) => Cause::Write {
                    code: error.cause.code(),
                    append: "reconciliation",
                    effect: error.effect,
                },
            },
            evidence: Evidence::Reconciliation {
                observed: error.observed,
            },
            authority: None,
        }
    }
}
