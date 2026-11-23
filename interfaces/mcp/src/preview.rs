//! Opaque protocol projection over Kernel-owned preview lifetime and observations.

use gazette_cli::command::{
    Kernel,
    preview::{self as command, Outcome as CommandOutcome},
};
use gazette_cli::sdk::{preview::session, source::Sha256Digest};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

mod input;
pub(super) use input::Input;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Plan,
    Start,
    ProductionPlan,
    ProductionStart,
    Inspect,
    Poll,
    Stop,
}

impl Route {
    pub(super) const ALL: [Self; 7] = [
        Self::Plan,
        Self::Start,
        Self::ProductionPlan,
        Self::ProductionStart,
        Self::Inspect,
        Self::Poll,
        Self::Stop,
    ];
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan_preview",
            Self::Start => "start_preview",
            Self::ProductionPlan => "plan_production_preview",
            Self::ProductionStart => "start_production_preview",
            Self::Inspect => "inspect_preview",
            Self::Poll => "poll_preview",
            Self::Stop => "stop_preview",
        }
    }
    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema) = match self {
            Self::Plan => (
                "Use plan_preview to see what a local or production-linked preview would touch before you start it. One call resolves the exact preview selection and portable effect classes before any source, filesystem, listener, process, network, Git or release operation. This plans developer-local or production-linked preview only; it is not deploy-all dry-run.",
                schemars::schema_for!(input::Start),
            ),
            Self::Start => (
                "Use start_preview to serve a publication locally so you can look at it. One call admits one local preview worker over the same publication recipe, acquisition, compiler, fixed watch lifecycle and HTTP transport as CLI preview-sites. Requires independent local rendering, native-read and listener permissions. Returns starting custody, not listener readiness: inspect or poll until the native ready observation. No source/store/output writes, remote provider reads, signing or publication.",
                schemars::schema_for!(input::Start),
            ),
            Self::ProductionPlan => (
                "Use plan_production_preview to see what a production-linked preview would touch before you start it. One call resolves one fresh production-linked preview and its complete portable effect classes before source, listener or process work. Registry reconciliation is explicitly unconfigured; no staged artifact, local URL rewriting, watch, outbound network, Git mutation, preparation, authority consumption or release effect is admitted.",
                schemars::schema_for!(input::Production),
            ),
            Self::ProductionStart => (
                "Use start_production_preview to serve a fresh production-linked render locally for review. One call starts one held fresh production-linked render on exact loopback listeners through the native preview owner. Registry reconciliation is explicitly unconfigured. This is noncertified no-push serving, not release preparation or deployment.",
                schemars::schema_for!(input::Production),
            ),
            Self::Inspect => (
                "Use inspect_preview to learn what a running preview is doing. One call reads the native worker snapshot without rebuilding or serving an HTTP step. A latest state is historical while stale, stopping or terminated. finished means the SDK worker actually ended; stop_requested alone never proves closed listeners.",
                schemars::schema_for!(input::Handle),
            ),
            Self::Poll => (
                "Use poll_preview to follow what a preview has done since your last look. One call reads non-consuming bounded native observations after an exact cursor. Repeating a cursor can recover retained responses. An overwritten prefix is an explicit gap; latest state and terminal result remain available. Polling does not drive HTTP, source capture or rebuilds.",
                schemars::schema_for!(input::Poll),
            ),
            Self::Stop => (
                "Use stop_preview to end a preview you started. One call requests cooperative stop at native checkpoints. Native work already in progress is not cancelled retroactively. release_handle=true removes custody only after the worker actually finishes; otherwise retain the handle and inspect again. No automatic restart, eviction or expiry.",
                schemars::schema_for!(input::Stop),
            ),
        };
        Tool::new(self.name(), format!("{meaning} Four process-local handles maximum, including completed handles until explicitly released. Source/Theme/workspace bindings stay fixed. Actual serving runs independently of MCP polling. Output overflow preserves the handle and original request digest; never automatically retry start. Service teardown requests stop but individual response loss does not prove closure. Preview is private, loopback and noncertified; browser scripts may still contact external hosts. At the handle limit, stop a finished preview with release_handle=true. Whether a previewed site is published stays with its publisher."), schema.ensure_object().clone())
            .with_raw_output_schema(match self {
                Self::Plan => crate::tools::output_schema::<gazette_cli::sdk::preview::runner::Plan>(),
                Self::ProductionPlan => crate::tools::output_schema::<gazette_cli::sdk::production_preview::Plan>(),
                _ => crate::tools::output_schema::<Report<'static>>(),
            })
            .with_annotations(ToolAnnotations::new()
                .read_only(matches!(self, Self::Plan | Self::ProductionPlan | Self::Inspect | Self::Poll))
                .destructive(matches!(self, Self::Start | Self::ProductionStart | Self::Stop))
                .idempotent(matches!(self, Self::Plan | Self::ProductionPlan | Self::Inspect | Self::Poll))
                .open_world(true))
    }
    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        input::decode(self, arguments)
    }
}

#[derive(Serialize, JsonSchema)]
struct Report<'a> {
    session: &'a str,
    /// The original start request, never a renewed source binding or permission.
    request_sha256: Sha256Digest,
    /// Registry custody, not listener readiness or continued native health.
    retained: bool,
    #[serde(flatten)]
    outcome: Outcome<'a>,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a> {
    Observed {
        snapshot: &'a session::Snapshot,
    },
    Polled {
        observations: &'a session::Observations,
    },
    Released {
        terminal: &'a session::Terminal,
    },
    Failed {
        code: &'a str,
    },
    /// Complete result could not fit. The original handle remains usable when retained.
    Delivery {
        code: &'static str,
    },
}

pub(super) struct Sessions {
    kernel: Arc<Kernel>,
}

struct Delivery<'a> {
    handle: &'a str,
    digest: Sha256Digest,
    maximum: usize,
}

impl<'a> Delivery<'a> {
    fn new(handle: &'a str, digest: Sha256Digest, maximum: usize) -> Result<Self, ErrorData> {
        let this = Self {
            handle,
            digest,
            maximum,
        };
        // Both custody branches are fixed-width; reserve complete fallback before native effects.
        for retained in [true, false] {
            crate::tools::encode_complete(
                &this.report(
                    retained,
                    Outcome::Delivery {
                        code: "preview_response_unavailable",
                    },
                ),
                true,
                maximum,
            )
            .map_err(|_| {
                ErrorData::invalid_params(
                    "Response budget cannot preserve preview custody; no operation started",
                    None,
                )
            })?;
        }
        Ok(this)
    }
    fn report(&self, retained: bool, outcome: Outcome<'a>) -> Report<'a> {
        Report {
            session: self.handle,
            request_sha256: self.digest,
            retained,
            outcome,
        }
    }
    fn finish(
        &self,
        retained: bool,
        outcome: Outcome<'_>,
        failed: bool,
    ) -> Result<CallToolResult, ErrorData> {
        let report = Report {
            session: self.handle,
            request_sha256: self.digest,
            retained,
            outcome,
        };
        crate::tools::encode_complete(&report, failed, self.maximum).or_else(|_| {
            crate::tools::encode_complete(&self.report(retained, Outcome::Delivery {
                code: "preview_response_unavailable",
            }), true, self.maximum)
        }).map_err(|_| ErrorData::internal_error(
            "Reserved preview delivery contract failed; preserve original handle and request; no automatic restart", None))
    }
}

impl Default for Sessions {
    fn default() -> Self {
        Self::new(Arc::new(Kernel::new()))
    }
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
        if let Input::Plan(request) = input {
            let (publication, addresses, options, _) = request.into_command_parts()?;
            return match self.kernel.execute(command::Plan {
                publication,
                addresses,
                options,
            }) {
                Ok(plan) => crate::tools::encode(&plan, false, maximum),
                Err(error) => command_failure(&error, maximum),
            };
        }
        if let Input::ProductionPlan(request) = input {
            let parts = request.into_command_parts()?;
            return match self.kernel.execute(command::ProductionPlan {
                publication: parts.publication,
                registry: parts.registry,
                artifact: parts.artifact,
                addresses: parts.addresses,
                options: parts.options,
            }) {
                Ok(plan) => crate::tools::encode(&plan, false, maximum),
                Err(error) => command_failure(&error, maximum),
            };
        }
        if let Input::Start { request, digest } = input {
            let mut random = [0u8; 32];
            getrandom::fill(&mut random).map_err(|_| {
                ErrorData::internal_error("Handle entropy unavailable; no operation started", None)
            })?;
            let handle: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            let delivery = Delivery::new(&handle, digest, maximum)?;
            let (publication, addresses, options, history) = request.into_command_parts()?;
            return match self.kernel.execute(command::Start {
                handle: handle.clone(),
                request_sha256: digest,
                publication,
                addresses,
                options,
                history,
            }) {
                Ok(report) => deliver(&delivery, &report),
                Err(error) => command_failure(&error, maximum),
            };
        }
        if let Input::ProductionStart { request, digest } = input {
            let mut random = [0u8; 32];
            getrandom::fill(&mut random).map_err(|_| {
                ErrorData::internal_error("Handle entropy unavailable; no operation started", None)
            })?;
            let handle: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            let delivery = Delivery::new(&handle, digest, maximum)?;
            let parts = request.into_command_parts()?;
            return match self.kernel.execute(command::ProductionStart {
                handle: handle.clone(),
                request_sha256: digest,
                publication: parts.publication,
                registry: parts.registry,
                artifact: parts.artifact,
                addresses: parts.addresses,
                options: parts.options,
                history: parts.history,
            }) {
                Ok(report) => deliver(&delivery, &report),
                Err(error) => command_failure(&error, maximum),
            };
        }
        let handle = match &input {
            Input::Plan(_) => unreachable!("plan handled"),
            Input::ProductionPlan(_) => unreachable!("production plan handled"),
            Input::Inspect(input) => &input.session,
            Input::Poll(input) => &input.session,
            Input::Stop(input) => &input.session,
            Input::Start { .. } => unreachable!("start handled"),
            Input::ProductionStart { .. } => unreachable!("production start handled"),
        };
        // Reserve fixed-width custody fallback before a stop can change native state.
        Delivery::new(handle, Sha256Digest::of(&[]), maximum)?;
        let result = match input {
            Input::Plan(_) => unreachable!("plan handled"),
            Input::ProductionPlan(_) => unreachable!("production plan handled"),
            Input::Inspect(input) => self.kernel.execute(command::Inspect {
                handle: input.session,
            }),
            Input::Poll(input) => self.kernel.execute(command::Poll {
                handle: input.session,
                after: input.after,
                maximum: input.maximum,
            }),
            Input::Stop(input) => self.kernel.execute(command::Stop {
                handle: input.session,
                release_handle: input.release_handle,
            }),
            Input::Start { .. } => unreachable!("start handled"),
            Input::ProductionStart { .. } => unreachable!("production start handled"),
        };
        match result {
            Ok(report) => {
                let delivery = Delivery::new(&report.session, report.request_sha256, maximum)?;
                deliver(&delivery, &report)
            }
            Err(error) => command_failure(&error, maximum),
        }
    }
}

fn deliver(delivery: &Delivery<'_>, report: &command::Report) -> Result<CallToolResult, ErrorData> {
    let outcome = match &report.outcome {
        CommandOutcome::Observed(snapshot) => Outcome::Observed { snapshot },
        CommandOutcome::Polled(observations) => Outcome::Polled { observations },
        CommandOutcome::Released(terminal) => Outcome::Released { terminal },
        CommandOutcome::Failed(code) => Outcome::Failed { code },
    };
    delivery.finish(report.retained, outcome, report.failed())
}

fn command_failure(error: &command::Failure, maximum: usize) -> Result<CallToolResult, ErrorData> {
    if matches!(error, command::Failure::HandleCollision) {
        Err(ErrorData::internal_error(error.reason().to_owned(), None))
    } else {
        crate::tools::failure(error.code(), error.reason(), maximum)
    }
}

#[cfg(test)]
#[path = "../tests/internal/preview.rs"]
mod tests;
