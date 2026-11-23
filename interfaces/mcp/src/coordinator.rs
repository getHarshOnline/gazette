//! Explicit trusted-local coordinator operations through the existing SDK owners.

use gazette_cli::command::{self, Kernel};
use gazette_cli::sdk::{
    source::Sha256Digest,
    store::{self, coordinator},
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Enroll,
    Inspect,
    Adopt,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    /// Existing empty private anchor directory, independently retained outside backups.
    #[schemars(regex(pattern = "^/"))]
    anchor_root: PathBuf,
    /// Existing fresh native journal directory, never a restored database to adopt.
    #[schemars(regex(pattern = "^/"))]
    journal_root: PathBuf,
    /// Exact selected owner ID; no hashing, trimming or normalization.
    coordinator_id: String,
    /// Independently selected scope-binding digest, not a principal identity or credential.
    principal_scope: Sha256Digest,
    /// Explicit complete protected source/output/stage/backup roots; no inferred paths.
    protected_roots: Vec<PathBuf>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Enroll {
    #[serde(flatten)]
    input: Enrollment,
    /// Permit native local reads, independent of every effect permission.
    #[serde(default)]
    allow_read: bool,
    /// Permit fresh bounded anchor persistence; not journal generation or release effects.
    #[serde(default)]
    write_coordinator_anchor: bool,
    /// Permit native held local reservations, not a destination lease.
    #[serde(default)]
    reserve_local_coordination: bool,
    /// Native bounds and separately permitted coordination-file maintenance.
    #[serde(default)]
    read: store::ReadOptions,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Inspect {
    /// Explicit existing private anchor directory.
    #[schemars(regex(pattern = "^/"))]
    anchor_root: PathBuf,
    /// Independently retained exact native binding; never inferred from current anchor bytes.
    binding: coordinator::LocalCoordinatorBinding,
    /// Permit local reads only; no database observation, repair or resampling follows.
    #[serde(default)]
    allow_read: bool,
    /// Permit the native held local reservation during bounded anchor inspection.
    #[serde(default)]
    reserve_local_coordination: bool,
    /// Native bounds and independently permitted coordination-file maintenance.
    #[serde(default)]
    read: store::ReadOptions,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Adopt {
    #[schemars(regex(pattern = "^/"))]
    anchor_root: PathBuf,
    binding: coordinator::LocalCoordinatorBinding,
    previous: coordinator::LocalCoordinatorSeal,
    #[schemars(regex(pattern = "^/"))]
    restore_root: PathBuf,
    receipt_sha256: Sha256Digest,
    #[schemars(regex(pattern = "^/"))]
    successor_root: PathBuf,
    protected_roots: Vec<PathBuf>,
    #[serde(default)]
    allow_read: bool,
    #[serde(default)]
    options: coordinator::RestoreAdoptionOptions,
}

pub(super) enum Input {
    Enroll(Enroll),
    Inspect(Inspect),
    Adopt(Box<Adopt>),
}

/// Carrier-only reconciliation coordinates, not a synthetic native seal.
#[derive(Serialize, JsonSchema)]
struct Delivery<'a> {
    code: DeliveryCode,
    input: &'a Enrollment,
    message: &'static str,
}

#[derive(Serialize, JsonSchema)]
enum DeliveryCode {
    #[serde(rename = "coordinator_response_unavailable")]
    Unavailable,
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum EnrollOutput {
    Native(Box<coordinator::LocalCoordinatorSeal>),
    Delivery(Delivery<'static>),
}

impl Route {
    pub(super) const ALL: [Self; 3] = [Self::Enroll, Self::Inspect, Self::Adopt];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Enroll => "enroll_local_coordinator",
            Self::Inspect => "inspect_local_coordinator",
            Self::Adopt => "adopt_store_restore",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (mut schema, output, description) = match self {
            Self::Enroll => (
                schemars::schema_for!(Enroll),
                crate::tools::output_schema::<EnrollOutput>(),
                "Use enroll_local_coordinator once, to bind a fresh journal to a trusted-local anchor so later writers can be guarded against journal-only rollback on one host. One call enrolls one fresh generation-zero native journal under a separately retained trusted-local anchor through the SDK. Requires independent allow_read, write_coordinator_anchor, reserve_local_coordination and read.allow_coordination_files, default denied. No journal creation, generation advance, signing, release consumption or remote effects. Exact selected labels authenticate no owner; this single-host profile does not detect jointly restored anchor/journal copies or provide cross-host leases. Existing anchors/history are never adopted or repaired. Private response contains the native seal. A bounded coordinate fallback must fit before execution. Cancellation, lost carrier or overflow can leave anchor/coordination effects: preserve original input and residue, never infer rollback or automatically retry. If it fails, keep the original input and any residue it names, and check with inspect_local_coordinator. Which host is trusted, and whether to enroll, stay with the operator.",
            ),
            Self::Inspect => (
                schemars::schema_for!(Inspect),
                crate::tools::output_schema::<coordinator::LocalCoordinatorState>(),
                "Use inspect_local_coordinator to see the retained state of a trusted-local coordinator before opening a writer or recovering. One call inspects retained trusted-local coordinator state against an independently selected exact binding through the SDK. Requires independent allow_read, reserve_local_coordination and read.allow_coordination_files. No journal resampling, writer generation, anchor repair/adoption, signing or release effects. Clean is retained evidence, not proof a prior close/report succeeded; InProgress cannot infer whether a journal mutation happened. Native and complete response bounds apply. Permitted coordination effects may survive cancellation or carrier loss. If the state is InProgress or the call fails, do not infer what happened to the journal; inspect the journal natively. What to do next stays with the operator.",
            ),
            Self::Adopt => (
                schemars::schema_for!(Adopt),
                crate::tools::output_schema::<coordinator::RestoreAdoptionReceipt>(),
                "Use adopt_store_restore to bring a verified detached restore into service as a fresh successor root. One call verifies one exact latest-clean detached restore and adopts it into a fresh successor root through the SDK. Requires explicit read, adoption and anchor-write permissions. The old journal and restore remain unchanged; the successor advances exactly one fence. Stale, dirty, mismatched or uncertain evidence stops without fallback, cleanup, release or deployment. If the evidence is stale, dirty, mismatched or uncertain, nothing is adopted: go back to the backup and restore evidence. Adopting a restore stays with the operator.",
            ),
        };
        let object = schema.ensure_object();
        if matches!(self, Self::Enroll) {
            object["properties"]["protected_roots"]["items"]["pattern"] = "^/".into();
        }
        Tool::new(
            self.name(),
            description,
            schema.as_object().expect("named input schema").clone(),
        )
        .with_raw_output_schema(output)
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(matches!(self, Self::Enroll | Self::Adopt))
                .idempotent(matches!(self, Self::Inspect | Self::Adopt))
                .open_world(true),
        )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        if arguments
            .get("read")
            .is_some_and(|value| !value.is_object())
            || matches!(self, Self::Inspect | Self::Adopt)
                && !arguments.get("binding").is_some_and(Value::is_object)
        {
            return Err(invalid());
        }
        match self {
            Self::Enroll => {
                let input: Enroll =
                    serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
                if !input.input.anchor_root.is_absolute()
                    || !input.input.journal_root.is_absolute()
                    || input
                        .input
                        .protected_roots
                        .iter()
                        .any(|path| !path.is_absolute())
                {
                    return Err(invalid());
                }
                Ok(Input::Enroll(input))
            }
            Self::Inspect => {
                let input: Inspect =
                    serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
                if !input.anchor_root.is_absolute() || !input.binding.journal_root.is_absolute() {
                    return Err(invalid());
                }
                Ok(Input::Inspect(input))
            }
            Self::Adopt => {
                let input: Adopt =
                    serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
                if !input.anchor_root.is_absolute()
                    || !input.binding.journal_root.is_absolute()
                    || !input.restore_root.is_absolute()
                    || !input.successor_root.is_absolute()
                    || input.protected_roots.iter().any(|path| !path.is_absolute())
                {
                    return Err(invalid());
                }
                Ok(Input::Adopt(Box::new(input)))
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid coordinator arguments; consult inputSchema; no operation started",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Enroll(input) => input.execute(maximum),
            Self::Inspect(input) => {
                if !input.allow_read {
                    return denied(maximum);
                }
                match Kernel::new().execute(command::coordinator::Inspect {
                    anchor_root: input.anchor_root,
                    binding: input.binding,
                    options: store::SessionOptions {
                        allow_local_reservation: input.reserve_local_coordination,
                        read: input.read,
                    },
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
            Self::Adopt(input) => {
                if !input.allow_read {
                    return denied(maximum);
                }
                match Kernel::new().execute(command::coordinator::AdoptRestore {
                    anchor_root: input.anchor_root,
                    binding: input.binding,
                    previous: input.previous,
                    restore_root: input.restore_root,
                    receipt_sha256: input.receipt_sha256,
                    successor_root: input.successor_root,
                    protected_roots: input.protected_roots,
                    options: input.options,
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
        }
    }
}

fn denied(maximum: usize) -> Result<CallToolResult, ErrorData> {
    crate::tools::failure(
        "storage_permission",
        "Native coordinator reads not permitted; no operation started",
        maximum,
    )
}

impl Enroll {
    fn fallback(&self, maximum: usize) -> Result<CallToolResult, crate::tools::EncodingError> {
        crate::tools::encode_complete(
            &Delivery {
                code: DeliveryCode::Unavailable,
                input: &self.input,
                message: "Anchor/coordination effects may remain; preserve exact input and residue. No seal delivered, rollback inferred or automatic retry permitted.",
            },
            true,
            maximum,
        )
    }

    fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        if !self.allow_read {
            return denied(maximum);
        }
        let fallback = match self.fallback(maximum) {
            Ok(value) => value,
            Err(_) => {
                return crate::tools::failure(
                    "coordinator_response_budget",
                    "No enrollment started: exact reconciliation input exceeds response ceiling",
                    maximum,
                );
            }
        };
        let result = Kernel::new().execute(command::coordinator::Enroll {
            anchor_root: self.input.anchor_root,
            journal_root: self.input.journal_root,
            coordinator_id: self.input.coordinator_id,
            principal_scope: self.input.principal_scope,
            protected_roots: self.input.protected_roots,
            options: store::SessionOptions {
                allow_local_reservation: self.reserve_local_coordination,
                read: self.read,
            },
            allow_anchor_write: self.write_coordinator_anchor,
        });
        match result {
            Ok(report) => {
                Ok(crate::tools::encode_complete(&report, false, maximum).unwrap_or(fallback))
            }
            Err(error) => {
                // Generic encoding may itself substitute a context-free limit record. Use the
                // already bounded effect-aware fallback instead if the native error cannot fit.
                Ok(
                    crate::tools::failure_complete(error.code(), &error.to_string(), maximum)
                        .unwrap_or(fallback),
                )
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/coordinator.rs"]
mod tests;
