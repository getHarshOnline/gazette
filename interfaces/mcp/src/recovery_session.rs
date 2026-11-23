//! Separate bounded handles over restricted SDK interrupted-recovery custody.

use std::sync::Arc;

use gazette_cli::command::{Kernel, recovery_session as command};
use gazette_cli::sdk::{
    release::{recovery_session as session, session::Delivery},
    render::LimitedWriter,
    store::JournalInspection,
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const INPUT_BYTES: usize = 16 << 20;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Open,
    Operate,
    Close,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Open<T> {
    request: T,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Operate<T> {
    session: String,
    operation: T,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Close {
    session: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Report<'a> {
    Ready {
        session: &'a str,
        inspection: &'a JournalInspection,
    },
    OpenFailed {
        session: &'a str,
        error: &'a session::OpenError,
    },
    Outcome {
        session: &'a str,
        outcome: &'a session::Outcome,
    },
    Delivery {
        session: &'a str,
        delivery: Delivery<'a>,
    },
}

impl Route {
    pub(super) const ALL: [Self; 3] = [Self::Open, Self::Operate, Self::Close];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Open => "open_recovery_session",
            Self::Operate => "operate_recovery_session",
            Self::Close => "close_recovery_session",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema) = match self {
            Self::Open => (
                "Use open_recovery_session to recover a journal left interrupted by a release, without taking the ordinary writer. One call acquires exact snapshot-matching interrupted custody without advancing generation. The default unstarted profile requires no attempt starts; explicit local-transactions permits only verified child-free local starts across the entire journal. Receive-pack history is refused. Return actual readiness and a distinct process-local recovery handle. Four sessions maximum. Optional Git selection is fixed for this lifetime. No ordinary writer or authority provider is acquired. An open_failed handle is not active; a delivery fallback after successful acquisition retains its active handle for inspection and explicit close.",
                schemars::schema_for!(Open<session::OpenRequest>),
            ),
            Self::Operate => (
                "Use operate_recovery_session to inspect, reconcile or seal an interrupted journal under the recovery handle you hold. One call invokes inspect, reconcile, explicit close or explicitly permitted seal through the restricted SDK capability. Reconciliation retains original consumed intent with separate evidence-write and Git-read permissions. Seal additionally requires anchor-write permission and verified terminal history across the whole journal; it consumes the handle even on failure and grants no writer generation. Receive-pack history remains unsupported. Registration, consumption, dispatch and writer conversion are unavailable. Other failed operations retain the handle.",
                schemars::schema_for!(Operate<session::Operation>),
            ),
            Self::Close => (
                "Use close_recovery_session to end the recovery lifetime. One call explicitly releases restricted native custody and removes the handle, including on native close failure. The anchor remains interrupted even after successful reconciliation and close. Close is not settlement, restoration of ordinary writer authority, or publication success.",
                schemars::schema_for!(Close),
            ),
        };
        Tool::new(self.name(), format!("{meaning} No expiry or silent eviction. Sessions serialize bounded operations; a busy response starts no operation. Retain exact requests and independent custody coordinates. Before effects, response capacity is checked for a handle-bound native-coordinate fallback. Overflow/cancellation/carrier loss never means rollback or permission to retry. Server termination drops restricted custody without claiming clean close. These are private native capabilities, not a sandbox, cross-host lock, publisher approval or a public release. If a call fails or its response is lost, keep the original request and inspect native state before doing anything else. Whether to release, settle or approve stays with the publisher."), schema.ensure_object().clone())
            .with_raw_output_schema(crate::tools::output_schema::<Report<'static>>())
            .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        let raw = bounded(&value)?;
        match self {
            Self::Open => {
                let input: Open<Value> = serde_json::from_value(value).map_err(|_| invalid())?;
                let request =
                    session::parse_open(&bounded(&input.request)?).map_err(|_| invalid())?;
                Ok(Input::Open {
                    request: Box::new(request),
                    raw,
                })
            }
            Self::Operate => {
                let input: Operate<Value> = serde_json::from_value(value).map_err(|_| invalid())?;
                let operation =
                    session::parse_operation(&bounded(&input.operation)?).map_err(|_| invalid())?;
                Ok(Input::Operate {
                    handle: input.session,
                    operation: Box::new(operation),
                    raw,
                })
            }
            Self::Close => {
                let input: Close = serde_json::from_value(value).map_err(|_| invalid())?;
                Ok(Input::Operate {
                    handle: input.session,
                    operation: Box::new(session::Operation::Close),
                    raw,
                })
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid bounded recovery session request; use the named SDK shapes in inputSchema; no native operation started",
        None,
    )
}

fn bounded(value: &Value) -> Result<Vec<u8>, ErrorData> {
    let mut output = LimitedWriter::new(Vec::new(), INPUT_BYTES);
    serde_json::to_writer(&mut output, value).map_err(|_| invalid())?;
    Ok(output.into_inner())
}

pub(super) enum Input {
    Open {
        request: Box<session::OpenRequest>,
        raw: Vec<u8>,
    },
    Operate {
        handle: String,
        operation: Box<session::Operation>,
        raw: Vec<u8>,
    },
}

pub(super) struct Sessions {
    kernel: Arc<Kernel>,
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new(Arc::new(Kernel::new()))
    }
}

fn fallback(
    handle: &str,
    selection: &session::OpenRequest,
    raw: &[u8],
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    crate::tools::encode_complete(&Report::Delivery {
        session: handle,
        delivery: Delivery::new(&selection.coordinator, raw),
    }, true, maximum).map_err(|_| ErrorData::invalid_params("Response budget cannot retain recovery custody coordinates; no native operation started", None))
}

impl Sessions {
    pub(super) fn new(kernel: Arc<Kernel>) -> Self {
        Self { kernel }
    }

    pub(super) fn execute(
        &self,
        input: Input,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        match input {
            Input::Open { request, raw } => {
                let mut random = [0u8; 32];
                getrandom::fill(&mut random).map_err(|_| {
                    ErrorData::internal_error(
                        "Recovery handle entropy unavailable; no native operation started",
                        None,
                    )
                })?;
                let handle: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
                let mut reserved = None;
                let result = self.kernel.execute(command::Open {
                    handle,
                    request: *request,
                    reserve: |handle: &str, selection: &session::OpenRequest| {
                        reserved = Some(fallback(handle, selection, &raw, maximum)?);
                        Ok::<(), ErrorData>(())
                    },
                });
                match result {
                    Ok(command::OpenReport {
                        session: handle,
                        outcome: command::OpenOutcome::Ready { inspection },
                        ..
                    }) => {
                        let response = crate::tools::encode_complete(
                            &Report::Ready {
                                session: &handle,
                                inspection: &inspection,
                            },
                            false,
                            maximum,
                        )
                        .unwrap_or_else(|_| reserved.expect("pre-effect fallback reserved"));
                        Ok(response)
                    }
                    Ok(command::OpenReport {
                        session: handle,
                        outcome: command::OpenOutcome::Failed { error },
                        ..
                    }) => Ok(crate::tools::encode_complete(
                        &Report::OpenFailed {
                            session: &handle,
                            error: &error,
                        },
                        true,
                        maximum,
                    )
                    .unwrap_or_else(|_| reserved.expect("pre-effect fallback reserved"))),
                    Err(error) => command_failure(error, maximum),
                }
            }
            Input::Operate {
                handle,
                operation,
                raw,
            } => {
                let mut reserved = None;
                let result = self.kernel.execute(command::Operate {
                    handle,
                    operation: *operation,
                    reserve: |handle: &str, selection: &session::OpenRequest| {
                        reserved = Some(fallback(handle, selection, &raw, maximum)?);
                        Ok::<(), ErrorData>(())
                    },
                });
                let report = match result {
                    Ok(report) => report,
                    Err(error) => return command_failure(error, maximum),
                };
                let failed = report.outcome.is_error();
                Ok(crate::tools::encode_complete(
                    &Report::Outcome {
                        session: &report.session,
                        outcome: &report.outcome,
                    },
                    failed,
                    maximum,
                )
                .unwrap_or_else(|_| reserved.expect("pre-effect fallback reserved")))
            }
        }
    }
}

fn command_failure(
    error: command::Failure<ErrorData>,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match error {
        command::Failure::Busy => crate::tools::failure(
            "recovery_session_busy",
            "Another recovery session operation is active; this request started no native operation",
            maximum,
        ),
        command::Failure::Unavailable => crate::tools::failure(
            "recovery_session_unavailable",
            "Session registry failed; preserve original native evidence; do not automatically reopen or retry",
            maximum,
        ),
        command::Failure::Capacity => crate::tools::failure(
            "recovery_session_capacity",
            "Explicitly close an existing session; none expires or is evicted automatically",
            maximum,
        ),
        command::Failure::Unknown => crate::tools::failure(
            "recovery_session_unknown",
            "Unknown or explicitly closed handle; no operation started; inspect retained native evidence before any new acquisition",
            maximum,
        ),
        command::Failure::HandleCollision => Err(ErrorData::internal_error(
            "Recovery handle collision; no native operation started",
            None,
        )),
        command::Failure::Reservation(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "../tests/internal/recovery_session.rs"]
mod tests;
