//! Restricted interrupted custody shared by CLI/MCP; never an ordinary writer session.
//!
//! Native continuity and the selected durable attempt profile govern admission. Closing releases
//! reservations but leaves the anchor interrupted, even after complete no-effect settlement.

mod input;
pub use input::{OpenRequest, Operation, parse_open, parse_operation};

use super::{ReconcileReleaseRequest, session};
use crate::store;
use serde::Serialize;

/// Recovery acquisition does not create or advance a writer generation.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum OpenError {
    /// Invalid or denied selection before native custody acquisition.
    Before {
        /// Existing selection or permission code.
        code: &'static str,
    },
    /// Exact interrupted custody could not be acquired; no ordinary writer fallback.
    Acquisition {
        /// Existing native refusal code.
        code: &'static str,
    },
    /// Recovery was acquired but independently inspected readiness failed.
    Readiness {
        /// Existing native inspection code.
        code: &'static str,
        /// Separate reservation-release failure, never a clean-seal assertion.
        close_error: Option<&'static str>,
    },
}

/// Only results reachable through the restricted native capability are advertised.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Outcome {
    /// Exact held journal state, not cached readiness.
    Inspected {
        /// Original native result.
        inspection: store::JournalInspection,
    },
    /// Complete original consumed set observed and durably reconciled.
    Reconciled {
        /// Original receipt, including replay and unresolved dispositions.
        receipt: store::ReleaseReconciliationReceipt,
    },
    /// Reservations released successfully; the anchor remains interrupted.
    Closed,
    /// Exact clean custody, not a fresh writer or renewed publication authorization.
    Sealed {
        /// Existing native anchor result, with the unchanged generation.
        seal: store::coordinator::LocalCoordinatorSeal,
    },
    /// Existing loss-aware native failure projection, not a second recovery error model.
    Failed {
        /// Preserves acquired observations and uncertain evidence writes.
        failure: session::Failure,
    },
}

impl Outcome {
    /// Native failure is distinct from response delivery or settlement status.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

impl From<session::Failure> for Outcome {
    fn from(failure: session::Failure) -> Self {
        Self::Failed { failure }
    }
}

/// No writer, authority provider, dispatch method or conversion capability is exposed.
pub struct Session {
    reconciler: Option<store::coordinator::Reconciler>,
    selection: OpenRequest,
    reviewed: Option<super::ReviewedScope>,
}

impl Session {
    /// Acquire the exact retained generation with restricted native recovery custody.
    ///
    /// # Errors
    /// Refuses denied, overlapping, changed or unsupported started history; never resets an anchor.
    pub fn open(selection: OpenRequest) -> Result<(Self, store::JournalInspection), OpenError> {
        let before = |code| OpenError::Before { code };
        selection
            .session
            .validate()
            .map_err(|error| before(error.code()))?;
        let mut protected = selection.protected_roots.clone();
        if let Some(git) = &selection.git {
            if !git.is_valid() {
                return Err(before("release_session_git_selection"));
            }
            protected.push(git.isolation.clone());
        }
        let reviewed = selection
            .reviewed
            .as_ref()
            .map(|reviewed| {
                if selection
                    .git
                    .as_ref()
                    .is_some_and(|git| git.credentials.is_some())
                {
                    return Err("release_reviewed_ambiguous_credentials");
                }
                if let crate::build::WorkspaceInput::Local { root, .. } = &reviewed.workspace {
                    protected.push(root.clone());
                }
                reviewed.resolve()
            })
            .transpose()
            .map_err(before)?;
        session::check_roots(&selection.coordinator, &protected).map_err(before)?;
        let mut reconciler = store::coordinator::recover_release(
            &selection.coordinator.anchor_root,
            &selection.coordinator.binding,
            selection.expected,
            selection.profile,
            selection.session,
            selection.allow_anchor_write,
        )
        .map_err(|error| OpenError::Acquisition { code: error.code() })?;
        let inspection = match reconciler.inspect() {
            Ok(inspection) => inspection,
            Err(error) => {
                let close_error = reconciler.close().err().map(|error| error.code());
                return Err(OpenError::Readiness {
                    code: error.code(),
                    close_error,
                });
            }
        };
        Ok((
            Self {
                reconciler: Some(reconciler),
                selection,
                reviewed,
            },
            inspection,
        ))
    }

    /// Original independently retained coordinates, not current custody evidence.
    pub fn selection(&self) -> &OpenRequest {
        &self.selection
    }

    /// Explicit close consumes custody even if final native verification fails.
    pub fn is_closed(&self) -> bool {
        self.reconciler.is_none()
    }

    /// Project one restricted operation through the existing native owner.
    /// Prebound delivery coordinates before calling; response loss never authorizes retry.
    pub fn execute(&mut self, operation: &Operation) -> Outcome {
        if let Operation::Seal { allow_seal } = operation {
            return match self.reconciler.take() {
                Some(reconciler) => match reconciler.seal(*allow_seal) {
                    Ok(seal) => Outcome::Sealed { seal },
                    Err(error) => session::Failure::from(error).into(),
                },
                None => session::Failure::selection("recovery_session_closed").into(),
            };
        }
        if matches!(operation, Operation::Close) {
            return match self.reconciler.take() {
                Some(reconciler) => match reconciler.close() {
                    Ok(()) => Outcome::Closed,
                    Err(error) => session::Failure::from(error).into(),
                },
                None => session::Failure::selection("recovery_session_closed").into(),
            };
        }
        let Some(reconciler) = self.reconciler.as_mut() else {
            return session::Failure::selection("recovery_session_closed").into();
        };
        let current = match reconciler.inspect() {
            Ok(inspection) => inspection,
            Err(error) => return session::Failure::from(error).into(),
        };
        match operation {
            Operation::Inspect => Outcome::Inspected {
                inspection: current,
            },
            Operation::Reconcile {
                expected,
                authorization_id,
                number,
                options,
            } => {
                if current.header != *expected {
                    return session::Failure::selection("release_session_generation").into();
                }
                let Some(git) = &self.selection.git else {
                    return session::Failure::selection("release_session_git_unconfigured").into();
                };
                let request = ReconcileReleaseRequest {
                    database: &self
                        .selection
                        .coordinator
                        .binding
                        .journal_root
                        .join("store.sqlite"),
                    expected: *expected,
                    authorization_id,
                    number: *number,
                };
                let result = match &self.reviewed {
                    Some(scope) => {
                        reconciler.reconcile_reviewed_release(scope, git, request, *options)
                    }
                    None => reconciler.reconcile_release(git, request, *options),
                };
                match result {
                    Ok(receipt) => Outcome::Reconciled { receipt },
                    Err(error) => session::Failure::from(error).into(),
                }
            }
            Operation::Close | Operation::Seal { .. } => unreachable!("custody consumed above"),
        }
    }
}
