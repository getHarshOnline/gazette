//! Ordinary local store lifetimes; no release coordinator, SQL command surface or reacquisition.

use gazette_cli::command::{Kernel, store_session as command};
use gazette_cli::sdk::{render::LimitedWriter, source::Sha256Digest, store};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{path::PathBuf, sync::Arc};

const INPUT_BYTES: usize = 1 << 20;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Open,
    Inspect,
    Close,
}

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Selection {
    Connection {
        /// Existing database; never created, migrated or adopted.
        #[schemars(regex(pattern = "^/"))]
        path: PathBuf,
        /// Existing private namespace held by cooperating local callers.
        #[schemars(regex(pattern = "^/"))]
        coordination_root: PathBuf,
        /// Required true, independently of coordination/reservation permissions.
        allow_native_reads: bool,
        /// Native lifetime connection budget and permissions.
        options: store::SessionOptions,
    },
    Writer {
        /// Existing private initialized journal directory.
        #[schemars(regex(pattern = "^/"))]
        root: PathBuf,
        /// Independently retained complete coordinates; no current-header fallback.
        expected_header: store::JournalHeader,
        /// Required true; never authorizes generation advancement by itself.
        allow_native_reads: bool,
        /// Independent native generation/reservation/coordination permissions and limits.
        options: store::WriterOptions,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Open {
    request: Selection,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Handle {
    session: String,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Open,
    Inspect,
    Close,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FailurePhase {
    Acquisition,
    Readiness,
    Inspection,
    Close,
}

#[derive(Serialize, JsonSchema)]
struct Failure<'a> {
    phase: FailurePhase,
    code: &'a str,
    /// Only the original failed writer acquisition supplies this native effect.
    generation: Option<store::GenerationEffect>,
    /// Readiness failures explicitly close their acquired native lifetime; this is its separate error.
    close_error: Option<&'a str>,
}

#[derive(Serialize, JsonSchema)]
struct Report<'a> {
    session: &'a str,
    /// Only true while this service retains an actual native object for the handle.
    active: bool,
    #[serde(flatten)]
    outcome: Outcome<'a>,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a> {
    /// The reserved carrier contract failed; retain the original request and inspect native state.
    DeliveryContract {
        phase: Phase,
        request_sha256: Sha256Digest,
        acquired: bool,
        acquired_header: Option<store::JournalHeader>,
        generation: Option<store::GenerationEffect>,
    },
    Connected {
        observation: &'a store::Observation,
    },
    Acquired {
        inspection: &'a store::JournalInspection,
    },
    Observed {
        observation: &'a store::Observation,
    },
    Inspected {
        inspection: &'a store::JournalInspection,
    },
    Closed,
    Failed {
        failure: &'a Failure<'a>,
        /// Native acquisition succeeded, even if its later readiness/close failed.
        acquired: bool,
        /// Original successful readiness only; never represented as a new inspection.
        acquired_header: Option<store::JournalHeader>,
    },
    Delivery {
        code: &'static str,
        phase: Phase,
        request_sha256: Sha256Digest,
        /// Original explicit locator/permission selection; not renewed authority.
        selection: &'a Selection,
        acquired: bool,
        acquired_header: Option<store::JournalHeader>,
        failure: Option<&'a Failure<'a>>,
    },
}

impl Route {
    pub(super) const ALL: [Self; 3] = [Self::Open, Self::Inspect, Self::Close];
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Open => "open_store_session",
            Self::Inspect => "inspect_store_session",
            Self::Close => "close_store_session",
        }
    }
    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema) = match self {
            Self::Open => (
                "Use open_store_session to hold a journal open across several calls, either read-only or to advance its generation once. One call opens an ordinary SQL-read-only connection or advances exactly the independently expected ordinary journal generation once. Native read, reservation, coordination and generation permissions remain independent. Return actual readiness and a retained opaque handle. Failed acquisition preserves original generation uncertainty; failed readiness includes separate explicit close evidence. No release coordinator or release operation is acquired.",
                schemars::schema_for!(Open),
            ),
            Self::Inspect => (
                "Use inspect_store_session to look at the state of a journal you already hold open. One call reads fresh native state from the same held connection or writer without reacquisition. Connection observations are raw metadata, not accepted journal schema. Writer inspections revalidate the originally acquired generation. Native failure permanently invalidates that lifetime; cached readiness never replaces a failed read.",
                schemars::schema_for!(Handle),
            ),
            Self::Close => (
                "Use close_store_session when you finish with a held journal. One call explicitly consumes the handle and closes its native lifetime, removing the handle even on close failure. No generation is reset, release is settled or failed history repaired.",
                schemars::schema_for!(Handle),
            ),
        };
        Tool::new(self.name(), format!("{meaning} Four process-local handles maximum, no expiry or eviction. Operations serialize under a nonblocking registry lock. Response capacity is checked before native work; successful-open overflow retains the active handle. Inactive correlation tokens do not name a live session. Carrier loss never proves rollback or permits automatic retry. Service termination drops resources without successful-close evidence. Native local reservation is not cross-host/copy/restore fencing. If a call fails or its response is lost, keep the original request and inspect native state before doing anything else. Settling or releasing from a journal stays with the publisher."), schema.ensure_object().clone())
            .with_raw_output_schema(crate::tools::output_schema::<Report<'static>>())
            .with_annotations(ToolAnnotations::new().read_only(false).open_world(true).idempotent(false).destructive(!matches!(self, Self::Inspect)))
    }
    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        let mut bytes = LimitedWriter::new(Vec::new(), INPUT_BYTES);
        serde_json::to_writer(&mut bytes, &value).map_err(|_| invalid())?;
        let digest = Sha256Digest::of(&bytes.into_inner());
        for pointer in [
            "/request",
            "/request/options",
            "/request/options/read",
            "/request/options/session",
            "/request/options/session/read",
            "/request/expected_header",
        ] {
            if value.pointer(pointer).is_some_and(|item| !item.is_object()) {
                return Err(invalid());
            }
        }
        match self {
            Self::Open => {
                let input: Open = serde_json::from_value(value).map_err(|_| invalid())?;
                input.request.validate()?;
                Ok(Input::Open {
                    request: Box::new(input.request),
                    digest,
                })
            }
            Self::Inspect | Self::Close => {
                let input: Handle = serde_json::from_value(value).map_err(|_| invalid())?;
                if input.session.len() != 64
                    || !input
                        .session
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
                {
                    return Err(invalid());
                }
                Ok(Input::Handle {
                    session: input.session,
                    close: matches!(self, Self::Close),
                    digest,
                })
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid ordinary-store session selection; use complete named objects, absolute paths and explicit native permissions; no operation started",
        None,
    )
}

impl Selection {
    fn validate(&self) -> Result<(), ErrorData> {
        let permitted = match self {
            Self::Connection {
                path,
                coordination_root,
                allow_native_reads,
                options,
            } => {
                *allow_native_reads
                    && path.is_absolute()
                    && coordination_root.is_absolute()
                    && options.validate().is_ok()
            }
            Self::Writer {
                root,
                allow_native_reads,
                options,
                ..
            } => *allow_native_reads && root.is_absolute() && options.validate().is_ok(),
        };
        if permitted { Ok(()) } else { Err(invalid()) }
    }
}

impl From<Selection> for command::Selection {
    fn from(selection: Selection) -> Self {
        match selection {
            Selection::Connection {
                path,
                coordination_root,
                allow_native_reads,
                options,
            } => Self::Connection {
                path,
                coordination_root,
                allow_native_reads,
                options,
            },
            Selection::Writer {
                root,
                expected_header,
                allow_native_reads,
                options,
            } => Self::Writer {
                root,
                expected_header,
                allow_native_reads,
                options,
            },
        }
    }
}

impl From<&command::Selection> for Selection {
    fn from(selection: &command::Selection) -> Self {
        match selection {
            command::Selection::Connection {
                path,
                coordination_root,
                allow_native_reads,
                options,
            } => Self::Connection {
                path: path.clone(),
                coordination_root: coordination_root.clone(),
                allow_native_reads: *allow_native_reads,
                options: *options,
            },
            command::Selection::Writer {
                root,
                expected_header,
                allow_native_reads,
                options,
            } => Self::Writer {
                root: root.clone(),
                expected_header: *expected_header,
                allow_native_reads: *allow_native_reads,
                options: *options,
            },
        }
    }
}

pub(super) enum Input {
    Open {
        request: Box<Selection>,
        digest: Sha256Digest,
    },
    Handle {
        session: String,
        close: bool,
        digest: Sha256Digest,
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

/// Prove the complete lossless diagnostic carrier fits before effects. Selected paths are original
/// request locators, not newly created native paths. Native success observations may be larger.
struct Reservation {
    maximum: usize,
}
impl Reservation {
    fn new(
        handle: &str,
        selection: &Selection,
        digest: Sha256Digest,
        maximum: usize,
    ) -> Result<Self, ErrorData> {
        let code = "\0".repeat(256);
        let failure = Failure {
            phase: FailurePhase::Acquisition,
            code: &code,
            generation: Some(store::GenerationEffect::NotAttempted),
            close_error: Some(&code),
        };
        let witness = Report {
            session: handle,
            active: false,
            outcome: Outcome::Delivery {
                code: "store_session_response_unavailable",
                phase: Phase::Inspect,
                request_sha256: digest,
                selection,
                acquired: false,
                acquired_header: Some(store::JournalHeader {
                    schema_version: u32::MAX,
                    journal_id: [255; 32],
                    journal_epoch: u64::MAX,
                    writer_fence: u64::MAX,
                }),
                failure: Some(&failure),
            },
        };
        crate::tools::encode_complete(&witness, true, maximum).map_err(|_| {
            ErrorData::invalid_params(
                "Response budget cannot retain ordinary-store custody; no native operation started",
                None,
            )
        })?;
        Ok(Self { maximum })
    }
    fn finish(
        &self,
        full: &Report<'_>,
        fallback: &Report<'_>,
        is_error: bool,
    ) -> Result<CallToolResult, ErrorData> {
        if let Ok(result) = crate::tools::encode_complete(full, is_error, self.maximum) {
            return Ok(result);
        }
        if let Ok(result) = crate::tools::encode_complete(fallback, true, self.maximum) {
            return Ok(result);
        }
        let Outcome::Delivery {
            phase,
            request_sha256,
            acquired,
            acquired_header,
            failure,
            ..
        } = &fallback.outcome
        else {
            return Err(ErrorData::internal_error(
                "Invalid ordinary-store delivery selection; no automatic retry",
                None,
            ));
        };
        // Only fixed-width original effect coordinates survive a broken native-code/string bound.
        // This is strictly smaller than the pre-effect witness, never unbounded ErrorData evidence.
        crate::tools::encode_complete(&Report {
            session: fallback.session, active: fallback.active,
            outcome: Outcome::DeliveryContract { phase: *phase, request_sha256: *request_sha256,
                acquired: *acquired, acquired_header: *acquired_header,
                generation: failure.and_then(|failure| failure.generation),
            },
        }, true, self.maximum).map_err(|_| ErrorData::internal_error(
            format!("store_session_delivery_contract: session={}; active={}; preserve original request; no automatic retry", fallback.session, fallback.active), None))
    }
}

struct Delivery<'a> {
    handle: &'a str,
    selection: &'a Selection,
    digest: Sha256Digest,
    phase: Phase,
}
impl Delivery<'_> {
    fn report<'a>(
        &'a self,
        active: bool,
        acquired: bool,
        acquired_header: Option<store::JournalHeader>,
        failure: Option<&'a Failure<'a>>,
    ) -> Report<'a> {
        Report {
            session: self.handle,
            active,
            outcome: Outcome::Delivery {
                code: "store_session_response_unavailable",
                phase: self.phase,
                request_sha256: self.digest,
                selection: self.selection,
                acquired,
                acquired_header,
                failure,
            },
        }
    }
}

impl Sessions {
    pub(super) fn new(kernel: Arc<Kernel>) -> Self {
        Self { kernel }
    }

    pub(super) fn materialize(
        &self,
        input: crate::materialization::Input,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        input.execute(&self.kernel, maximum)
    }

    pub(super) fn materialize_documentation(
        &self,
        input: crate::documentation_materialization::Input,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        input.execute(&self.kernel, maximum)
    }

    pub(super) fn execute(
        &self,
        input: Input,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        match input {
            Input::Open { request, digest } => {
                request.validate()?;
                let mut random = [0u8; 32];
                getrandom::fill(&mut random).map_err(|_| {
                    ErrorData::internal_error(
                        "Handle entropy unavailable; no native operation started",
                        None,
                    )
                })?;
                let handle: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
                let wire = *request;
                let mut reserved = None;
                let result = self.kernel.execute(command::Open {
                    handle,
                    selection: wire.clone().into(),
                    reserve: |handle: &str, _: &command::Selection| {
                        reserved = Some(Reservation::new(handle, &wire, digest, maximum)?);
                        Ok::<(), ErrorData>(())
                    },
                });
                let report = match result {
                    Ok(report) => report,
                    Err(error) => return command_failure(error, maximum),
                };
                finish(
                    &report,
                    &wire,
                    digest,
                    Phase::Open,
                    reserved.expect("pre-effect reservation"),
                )
            }
            Input::Handle {
                session,
                close,
                digest,
            } => {
                let mut reserved = None;
                let result = self.kernel.execute(command::Operate {
                    handle: session,
                    close,
                    reserve: |handle: &str,
                              selection: &command::Selection,
                              acquired_header,
                              close| {
                        let wire = Selection::from(selection);
                        let reservation = Reservation::new(handle, &wire, digest, maximum)?;
                        reserved = Some((reservation, wire, acquired_header));
                        let _ = close;
                        Ok::<(), ErrorData>(())
                    },
                });
                let report = match result {
                    Ok(report) => report,
                    Err(error) => return command_failure(error, maximum),
                };
                let (reservation, wire, _) = reserved.expect("pre-effect reservation");
                finish(
                    &report,
                    &wire,
                    digest,
                    if close { Phase::Close } else { Phase::Inspect },
                    reservation,
                )
            }
        }
    }
}

fn finish(
    report: &command::Report,
    selection: &Selection,
    digest: Sha256Digest,
    phase: Phase,
    reservation: Reservation,
) -> Result<CallToolResult, ErrorData> {
    let delivery = Delivery {
        handle: &report.session,
        selection,
        digest,
        phase,
    };
    match &report.outcome {
        command::Outcome::Connected { observation } => reservation.finish(
            &Report {
                session: &report.session,
                active: report.active,
                outcome: Outcome::Connected { observation },
            },
            &delivery.report(report.active, true, report.acquired_header, None),
            false,
        ),
        command::Outcome::Acquired { inspection } => reservation.finish(
            &Report {
                session: &report.session,
                active: report.active,
                outcome: Outcome::Acquired { inspection },
            },
            &delivery.report(report.active, true, report.acquired_header, None),
            false,
        ),
        command::Outcome::Observed { observation } => reservation.finish(
            &Report {
                session: &report.session,
                active: report.active,
                outcome: Outcome::Observed { observation },
            },
            &delivery.report(report.active, true, report.acquired_header, None),
            false,
        ),
        command::Outcome::Inspected { inspection } => reservation.finish(
            &Report {
                session: &report.session,
                active: report.active,
                outcome: Outcome::Inspected { inspection },
            },
            &delivery.report(report.active, true, report.acquired_header, None),
            false,
        ),
        command::Outcome::Closed => reservation.finish(
            &Report {
                session: &report.session,
                active: false,
                outcome: Outcome::Closed,
            },
            &delivery.report(false, true, report.acquired_header, None),
            false,
        ),
        command::Outcome::Failed { failure, acquired } => {
            let failure = Failure {
                phase: match failure.phase {
                    command::NativeFailurePhase::Acquisition => FailurePhase::Acquisition,
                    command::NativeFailurePhase::Readiness => FailurePhase::Readiness,
                    command::NativeFailurePhase::Inspection => FailurePhase::Inspection,
                    command::NativeFailurePhase::Close => FailurePhase::Close,
                },
                code: failure.code,
                generation: failure.generation,
                close_error: failure.close_error,
            };
            reservation.finish(
                &Report {
                    session: &report.session,
                    active: report.active,
                    outcome: Outcome::Failed {
                        failure: &failure,
                        acquired: *acquired,
                        acquired_header: report.acquired_header,
                    },
                },
                &delivery.report(
                    report.active,
                    *acquired,
                    report.acquired_header,
                    Some(&failure),
                ),
                true,
            )
        }
    }
}

pub(super) fn command_failure(
    error: command::Failure<ErrorData>,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match error {
        command::Failure::Busy => crate::tools::failure(
            "store_session_busy",
            "Another ordinary-store operation is active; no operation started",
            maximum,
        ),
        command::Failure::Unavailable => crate::tools::failure(
            "store_session_unavailable",
            "Registry failed; no automatic reacquisition; preserve native evidence",
            maximum,
        ),
        command::Failure::Capacity => crate::tools::failure(
            "store_session_capacity",
            "Explicitly close an existing handle; none expires or is evicted",
            maximum,
        ),
        command::Failure::Unknown => crate::tools::failure(
            "store_session_unknown",
            "Unknown or closed ordinary-store handle; no operation started",
            maximum,
        ),
        command::Failure::HandleCollision => Err(ErrorData::internal_error(
            "Handle collision; no native operation started",
            None,
        )),
        command::Failure::Selection => Err(invalid()),
        command::Failure::NotWriter => crate::tools::failure(
            "store_session_not_writer",
            "This operation requires an already-held ordinary writer; no source, journal or output effect started",
            maximum,
        ),
        command::Failure::Reservation(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "../tests/internal/store_session.rs"]
mod tests;
