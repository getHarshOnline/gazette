//! One native writer lifetime shared by CLI/MCP, not a second release state machine.
//!
//! Open first, then obtain owner-approved authorization for the reported actual generation.
//! All operations retain the same writer. Dropping without explicit close leaves interrupted
//! custody; neither response loss nor process termination authorizes reopening or retry.

mod input;
mod result;
mod reviewed;
pub(super) use input::parse;
pub use input::{
    AuthorityCommand, GitSelection, OpenRequest, Operation, PreparedInput, ReviewInput,
    ReviewedInput, parse_open, parse_operation,
};
pub use result::{AuthorityObservation, Cause, Evidence, Failure, OpenError, Outcome};

use crate::{release, store};
use gazette_adapter_authority::CommandSource;
use gazette_core::process::{CaptureRequest, Options, Request};
use gazette_ports::{
    destination::RootSeparation,
    release::{AuthorityBindingDecoder, AuthoritySource},
};
use std::{path::PathBuf, time::Duration};

/// Native held custody and fixed caller selections. No clone or serializable writer capability.
pub struct Session {
    writer: Option<store::Writer>,
    selection: OpenRequest,
    database: PathBuf,
    protected: Vec<PathBuf>,
    reviewed: Option<release::ReviewedScope>,
}

struct StrictAuthorityBindingDecoder {
    limits: release::Limits,
}

impl AuthorityBindingDecoder for StrictAuthorityBindingDecoder {
    fn decode_binding(&self, bytes: &[u8]) -> Result<gazette_core::release::Binding, &'static str> {
        release::parse_binding(bytes, self.limits).map_err(release::Error::code)
    }
}

impl Session {
    /// Acquire once and report actual native readiness; never fabricate a successor fence.
    ///
    /// # Errors
    /// Preserves acquisition uncertainty and a separate close result after failed readiness.
    pub fn open(selection: OpenRequest) -> Result<(Self, store::JournalInspection), OpenError> {
        let before = |code| OpenError::Before { code };
        selection
            .writer
            .validate()
            .map_err(|error| before(error.code()))?;
        if !selection.allow_anchor_write {
            return Err(before("store_coordinator_permission"));
        }
        let mut protected = selection.protected_roots.clone();
        if let Some(git) = &selection.git {
            if !git.is_valid() {
                return Err(before("release_session_git_selection"));
            }
            protected.push(git.isolation.clone());
        }
        if let Some(authority) = &selection.authority {
            authority
                .command
                .validate()
                .map_err(|_| before("release_session_authority_selection"))?;
            if authority.input.len() > authority.max_input_bytes
                || authority.max_input_bytes > 64 << 20
                || authority.max_output_bytes > 64 << 20
                || !(1..=300_000).contains(&authority.timeout_ms)
            {
                return Err(before("release_session_authority_limit"));
            }
            protected.push(PathBuf::from(&authority.command.directory));
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
        check_roots(&selection.coordinator, &protected).map_err(before)?;
        let root = &selection.coordinator.binding.journal_root;
        let anchor = &selection.coordinator.anchor_root;
        let mut writer = store::coordinator::acquire_writer(
            anchor,
            &selection.coordinator.binding,
            selection.expected,
            selection.writer,
            selection.allow_anchor_write,
        )
        .map_err(|error| OpenError::Acquisition {
            code: error.cause.code(),
            generation: error.generation,
        })?;
        let inspection = match writer.inspect() {
            Ok(inspection) => inspection,
            Err(error) => {
                let close_error = writer.close().err().map(|error| error.code());
                return Err(OpenError::Readiness {
                    code: error.code(),
                    close_error,
                });
            }
        };
        let database = root.join("store.sqlite");
        Ok((
            Self {
                writer: Some(writer),
                selection,
                database,
                protected,
                reviewed,
            },
            inspection,
        ))
    }

    /// Independently selected initial custody coordinates, not refreshed authority or readiness.
    pub fn selection(&self) -> &OpenRequest {
        &self.selection
    }

    /// Whether explicit close already consumed the writer, including a failed close.
    pub fn is_closed(&self) -> bool {
        self.writer.is_none()
    }

    /// Execute through the existing application owner under the same held writer.
    /// Native phase failures are values and preserve earlier evidence. Callers must prebound
    /// a recovery-coordinate response before effects and must not replay after delivery loss.
    pub fn execute(&mut self, operation: &Operation) -> Outcome {
        if matches!(operation, Operation::Close) {
            return match self.writer.take() {
                Some(writer) => match writer.close() {
                    Ok(()) => Outcome::Closed,
                    Err(error) => Failure::from(error).into(),
                },
                None => Failure::selection("release_session_closed").into(),
            };
        }
        let Some(writer) = self.writer.as_mut() else {
            return Failure::selection("release_session_closed").into();
        };
        if matches!(operation, Operation::Inspect) {
            return match writer.inspect() {
                Ok(inspection) => Outcome::Inspected { inspection },
                Err(error) => Failure::from(error).into(),
            };
        }
        // Always refresh native state; neither selection nor a prior readiness response owns it.
        let expected = match operation {
            Operation::Register { expected, .. }
            | Operation::Consume { expected, .. }
            | Operation::Dispatch { expected, .. }
            | Operation::Reconcile { expected, .. }
            | Operation::Review { expected, .. }
            | Operation::RegisterReviewed { expected, .. }
            | Operation::ConsumeReviewed { expected, .. }
            | Operation::DispatchReviewed { expected, .. } => *expected,
            Operation::Inspect | Operation::Close => unreachable!("handled above"),
        };
        match writer.inspect() {
            Ok(current) if current.header == expected => (),
            Ok(_) => return Failure::selection("release_session_generation").into(),
            Err(error) => return Failure::from(error).into(),
        }
        if matches!(
            operation,
            Operation::Review { .. }
                | Operation::RegisterReviewed { .. }
                | Operation::ConsumeReviewed { .. }
                | Operation::DispatchReviewed { .. }
        ) {
            return reviewed::execute(
                writer,
                self.reviewed.as_ref(),
                &self.selection,
                &self.database,
                &self.protected,
                operation,
                expected,
            );
        }
        if let Operation::Reconcile {
            authorization_id,
            number,
            options,
            ..
        } = operation
        {
            let Some(git) = &self.selection.git else {
                return Failure::selection("release_session_git_unconfigured").into();
            };
            let request = release::ReconcileReleaseRequest {
                database: &self.database,
                expected,
                authorization_id,
                number: *number,
            };
            let result = match &self.reviewed {
                Some(scope) => writer.reconcile_reviewed_release(scope, git, request, *options),
                None => writer.reconcile_release(git, request, *options),
            };
            return match result {
                Ok(receipt) => Outcome::Reconciled { receipt },
                Err(error) => Failure::from(error).into(),
            };
        }
        let Some(selected) = &self.selection.authority else {
            return Failure::selection("release_session_authority_unconfigured").into();
        };
        let decoder = StrictAuthorityBindingDecoder {
            limits: selected.limits,
        };
        let mut authority = CommandSource::new(
            &gazette_adapter_process::Runner,
            CaptureRequest {
                request: Request {
                    command: &selected.command,
                    input: selected.input.as_bytes(),
                    options: Options {
                        allow_execute: selected.allow_execute,
                        max_input_bytes: selected.max_input_bytes,
                        timeout: Duration::from_millis(selected.timeout_ms),
                    },
                },
                max_output_bytes: selected.max_output_bytes,
                max_stderr_bytes: None,
            },
            &decoder,
        );
        let input = match operation {
            Operation::Register { input, .. }
            | Operation::Consume { input, .. }
            | Operation::Dispatch { input, .. } => input,
            _ => unreachable!("non-preparation operations handled above"),
        };
        let preparations = release::PreparationBatchRequest {
            database: &self.database,
            expected,
            selections: &input.selections,
            protected_roots: &self.protected,
        };
        let result: Result<Outcome, Failure> = match operation {
            Operation::Register { input, options, .. } => (|| {
                // Denied registration must not execute an authority command.
                options.record.validate().map_err(Failure::from)?;
                let binding = authority
                    .acquire_authority()
                    .map_err(|error| Failure::new(error.into()))?;
                writer
                    .register_release_authorization(
                        release::PreparedAuthorizationRequest {
                            authorization: input.authorization.as_bytes(),
                            binding: &binding,
                            preparations,
                        },
                        *options,
                    )
                    .map(|receipt| Outcome::Registered { receipt })
                    .map_err(Failure::from)
            })(),
            Operation::Consume { input, options, .. } => match &self.selection.git {
                None => Err(Failure::selection("release_session_git_unconfigured")),
                Some(git) => writer
                    .consume_release_authorization(
                        &mut authority,
                        git,
                        release::ConsumeAuthorizationRequest {
                            authorization: input.authorization.as_bytes(),
                            preparations,
                        },
                        *options,
                    )
                    .map(|receipt| Outcome::Consumed { receipt })
                    .map_err(Failure::from),
            },
            Operation::Dispatch {
                input,
                ordinal,
                number,
                options,
                ..
            } => match &self.selection.git {
                None => Err(Failure::selection("release_session_git_unconfigured")),
                Some(git) => writer
                    .dispatch_release_attempt(
                        &mut authority,
                        git,
                        release::DispatchAttemptRequest {
                            authorization: input.authorization.as_bytes(),
                            preparations,
                            ordinal: *ordinal,
                            number: *number,
                        },
                        *options,
                    )
                    .map(|attempt| Outcome::Dispatched { attempt })
                    .map_err(Failure::from),
            },
            _ => unreachable!("non-authority operations handled above"),
        };
        match result {
            Ok(outcome) => outcome,
            Err(mut failure) => {
                failure.authority = authority
                    .last_observation()
                    .copied()
                    .map(AuthorityObservation::from_native);
                failure.into()
            }
        }
    }
}

// Shared selected-root qualification, not a writer or recovery admission policy.
pub(super) fn check_roots(
    coordinator: &store::coordinator::LocalCoordinatorSelection,
    protected: &[PathBuf],
) -> Result<(), &'static str> {
    if protected.len() > 4096 || protected.iter().any(|path| !path.is_absolute()) {
        return Err("release_session_protected_roots");
    }
    for path in [&coordinator.binding.journal_root, &coordinator.anchor_root] {
        gazette_adapter_fs::FileSystem
            .check_root_separation(path, protected)
            .map_err(|error| match error {
                gazette_core::destination::Error::Observation(error) => error.code(),
                gazette_core::destination::Error::SourceOverlap { .. } => {
                    "release_session_root_overlap"
                }
                gazette_core::destination::Error::Collision { .. } => {
                    "release_session_root_collision"
                }
            })?;
    }
    Ok(())
}

/// Conservative bounded-delivery fallback, prepared before any native operation.
/// A request digest is a reference to the caller's retained exact input, not a replacement for it.
#[derive(serde::Serialize, schemars::JsonSchema)]
pub struct Delivery<'a> {
    /// Stable delivery-failure category, not a native effect disposition.
    pub code: &'static str,
    /// Exact independently selected custody coordinates.
    pub coordinator: &'a store::coordinator::LocalCoordinatorSelection,
    /// Exact caller-supplied raw request digest; retain its bytes outside the response.
    pub request_sha256: gazette_core::source::Sha256Digest,
    /// Explicit uncertainty and no-retry reminder.
    pub message: &'static str,
}

impl<'a> Delivery<'a> {
    /// Bind fallback evidence before acquisition or effects, without native access.
    pub fn new(
        coordinator: &'a store::coordinator::LocalCoordinatorSelection,
        request: &[u8],
    ) -> Self {
        Self {
            code: "release_session_response_unavailable",
            coordinator,
            request_sha256: gazette_core::source::Sha256Digest::of(request),
            message: "Native effects may remain. Preserve exact request, custody and journal evidence; no complete operation response delivered. Do not automatically reopen, retry or dispatch.",
        }
    }
}
