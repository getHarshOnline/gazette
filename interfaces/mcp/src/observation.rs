//! Managed diagnostic history through the shared SDK, never release orchestration.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gazette_cli::command::{Kernel, observation as command};
use gazette_cli::sdk::{release, source::Sha256Digest, store};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Record,
    Inspect,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    /// Existing private journal directory, not the database file.
    #[schemars(regex(pattern = "^/"))]
    root: PathBuf,
    /// Independently retained complete expected coordinates; never inferred or advanced here.
    expected_header: store::JournalHeader,
    /// New immutable observation coordinate; retain before calling.
    id: Sha256Digest,
    /// Exact complete target-array JSON, preserving duplicate keys for native rejection.
    targets_json: String,
    /// Fixed native Git executable, isolation and optional denied-by-default HTTPS helper.
    git: release::GitSelection,
    /// Complete explicitly protected source roots, separate from the journal.
    #[serde(default)]
    protected_roots: Vec<PathBuf>,
    /// Explicit trusted-local anchor and independently retained binding; omitted means no guard.
    #[serde(default)]
    coordinator: Option<store::coordinator::LocalCoordinatorSelection>,
    /// Independently permit selected control-anchor writes; never enroll or repair here.
    #[serde(default)]
    write_coordinator_anchor: bool,
    /// Permit destination reads and native provider execution.
    #[serde(default)]
    allow_read: bool,
    /// Separately permit supported network destinations.
    #[serde(default)]
    allow_network: bool,
    /// Permit one immutable diagnostic record, not SQL repair or release consumption.
    #[serde(default)]
    record_observation: bool,
    /// Permit local generation advancement, independently of record insertion.
    #[serde(default)]
    advance_writer_generation: bool,
    /// Permit the held local namespace reservation; not cross-host fencing.
    #[serde(default)]
    reserve_local_coordination: bool,
    /// Independent coordination-file permission and native connection limits.
    #[serde(default)]
    read: store::ReadOptions,
    /// Native parsing/revalidation bounds for the complete target selection.
    #[serde(default)]
    limits: release::Limits,
    /// Per-destination deadline, not a total batch deadline.
    #[serde(default = "default_timeout_ms")]
    #[schemars(range(max = u64::MAX))]
    timeout_ms: u64,
    /// Complete native response ceiling per destination.
    #[serde(default = "default_response_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_response_bytes: usize,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Inspect {
    /// Explicit existing database-file path, not a journal directory.
    #[schemars(regex(pattern = "^/"))]
    path: PathBuf,
    /// Exact immutable observation lookup coordinate.
    id: Sha256Digest,
    /// Compare supplied coordinates exactly; omitted/null only observes current history.
    #[serde(default)]
    expected_header: Option<store::JournalHeader>,
    /// Permit native journal reads, never destination reads.
    #[serde(default)]
    allow_read: bool,
    /// Independent coordination-file permission and all native connection bounds.
    #[serde(default)]
    read: store::ReadOptions,
    /// Native complete retained target-contract validation bounds.
    #[serde(default)]
    limits: release::Limits,
}

pub(super) enum Input {
    Record(Box<Record>),
    Inspect(Inspect),
}

fn default_timeout_ms() -> u64 {
    u64::try_from(release::RemoteReadOptions::default().timeout.as_millis())
        .expect("native bounded timeout fits milliseconds")
}

fn default_response_bytes() -> usize {
    release::RemoteReadOptions::default().max_response_bytes
}

/// Carrier-only fallback, prepared before any effect; not a second native result model.
#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Delivery<'a> {
    ReconciliationRequired {
        root: &'a Path,
        input_expected_header: store::JournalHeader,
        id: Sha256Digest,
        #[serde(skip_serializing_if = "Option::is_none")]
        coordinator: Option<&'a store::coordinator::LocalCoordinatorSelection>,
        message: &'static str,
    },
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum RecordOutput {
    Native(release::ObservationSessionReport<'static>),
    Delivery(Delivery<'static>),
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Record, Self::Inspect];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Record => "record_release_observation",
            Self::Inspect => "inspect_release_observation",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (schema, output, description) = match self {
            Self::Record => {
                let mut schema = schemars::schema_for!(Record);
                let properties = schema
                    .ensure_object()
                    .get_mut("properties")
                    .and_then(Value::as_object_mut)
                    .expect("named record input");
                properties
                    .get_mut("protected_roots")
                    .expect("protected roots")["items"]["pattern"] = "^/".into();
                let targets = properties
                    .get_mut("targets_json")
                    .and_then(Value::as_object_mut)
                    .expect("exact target JSON string");
                targets.insert("contentMediaType".into(), "application/json".into());
                targets.insert(
                    "contentSchema".into(),
                    serde_json::to_value(
                        schemars::generate::SchemaSettings::default()
                            .for_serialize()
                            .into_generator()
                            .into_root_schema_for::<Vec<release::Target>>(),
                    )
                    .expect("native target schema is JSON"),
                );
                (
                    schema,
                    crate::tools::output_schema::<RecordOutput>(),
                    "Use record_release_observation to sample the state of every release destination and append it to the journal as diagnostic history. One call acquires one exact expected local journal generation, samples the complete selected destination set, appends immutable diagnostic history and closes the writer through the shared SDK. Optional coordinator selects an existing independently retained trusted-local anchor/binding; write_coordinator_anchor separately permits its writes. Missing/null coordinator keeps the ordinary local writer, never automatic discovery or enrollment. Native reads, network access, record insertion, generation advancement, local reservation and read.allow_coordination_files are independent permissions, default denied. The selected guard rejects journal-only rollback and unresolved lifetimes; no cross-host or simultaneous anchor/journal rollback guarantee, signing, push, consumption, destination leases or settlement. Unavailable/indeterminate rows remain complete observations. Existing IDs refuse new reads but a generation may already have advanced; this tool is not retry-idempotent. Native provider execution is not sandboxed. Preserve the original anchor/binding/root/header/id before calling: cancellation, carrier loss or overflow can leave anchor, generation, reads or record effects. Inspect that exact observation; never automatically retry or infer rollback. Full native failures and close-failed receipts are retained when they fit; an effect-aware fallback including selected custody coordinates is bounded before execution. Whether to release stays with the publisher.",
                )
            }
            Self::Inspect => (
                schemars::schema_for!(Inspect),
                crate::tools::output_schema::<release::RefObservationInspection>(),
                "Use inspect_release_observation to see one diagnostic observation of release destinations that a journal recorded. One call inspects one immutable diagnostic observation in an explicit existing database through the shared SDK. Requires allow_read and independent read.allow_coordination_files; no SQL writes, generation advance, provider invocation, network, repair or resampling. Returns original creation coordinates separately from current journal evidence and explicit absence. Optional expected_header compares all coordinates, never infers freshness against copies/restores. Historical matching rows are not current destination truth, release authorization or settlement. Native target/storage and complete MCP response bounds apply; coordination-file effects may occur even on cancellation or delivery failure. Whether a destination is ready for release stays with the publisher.",
            ),
        };
        Tool::new(
            self.name(),
            description,
            schema.as_object().expect("object input schema").clone(),
        )
        .with_raw_output_schema(output)
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(matches!(self, Self::Record))
                .idempotent(matches!(self, Self::Inspect))
                .open_world(true),
        )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        if arguments.get("coordinator").is_some_and(|value| {
            !value.is_null()
                && (!value.is_object()
                    || value
                        .get("binding")
                        .is_none_or(|binding| !binding.is_object()))
        }) {
            return Err(invalid());
        }
        for key in ["read", "limits"] {
            if arguments.get(key).is_some_and(|v| !v.is_object()) {
                return Err(invalid());
            }
        }
        if arguments
            .get("expected_header")
            .is_some_and(|v| !(v.is_object() || v.is_null() && matches!(self, Self::Inspect)))
        {
            return Err(invalid());
        }
        match self {
            Self::Record => {
                let input: Record =
                    serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
                if !input.root.is_absolute()
                    || !input.git.is_valid()
                    || input.protected_roots.iter().any(|path| !path.is_absolute())
                    || input.coordinator.as_ref().is_some_and(|selected| {
                        !selected.anchor_root.is_absolute()
                            || !selected.binding.journal_root.is_absolute()
                    })
                {
                    return Err(invalid());
                }
                Ok(Input::Record(Box::new(input)))
            }
            Self::Inspect => {
                let input: Inspect =
                    serde_json::from_value(Value::Object(arguments)).map_err(|_| invalid())?;
                if !input.path.is_absolute() {
                    return Err(invalid());
                }
                Ok(Input::Inspect(input))
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid observation-history arguments; consult inputSchema; no operation started",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Record(input) => input.execute(maximum),
            Self::Inspect(input) => {
                if !input.allow_read {
                    return crate::tools::failure(
                        "storage_permission",
                        "Native history reads not permitted",
                        maximum,
                    );
                }
                match Kernel::new().execute(command::InspectObservation {
                    allow_native_reads: input.allow_read,
                    path: input.path,
                    id: input.id,
                    expected_header: input.expected_header,
                    read: input.read,
                    limits: input.limits,
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
        }
    }
}

impl Record {
    fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let fallback = Delivery::ReconciliationRequired {
            root: &self.root,
            input_expected_header: self.expected_header,
            id: self.id,
            coordinator: self.coordinator.as_ref(),
            message: "Complete observation result unavailable; selected anchor writes, generation advancement, destination reads and record insertion may have occurred. Preserve the selected anchor/binding and inspect the original root and observation ID with independently retained journal coordinates. Input expected_header is not a current observation. No automatic retry or rollback inference.",
        };
        let fallback = match crate::tools::encode_complete(&fallback, true, maximum) {
            Ok(fallback) => fallback,
            Err(_) => {
                return crate::tools::failure(
                    "observation_response_budget",
                    "No observation session started: reconciliation coordinates exceed response ceiling",
                    maximum,
                );
            }
        };
        let outcome = match Kernel::new().execute(command::RecordObservation {
            root: self.root,
            expected_header: self.expected_header,
            id: self.id,
            targets_json: self.targets_json.into_bytes(),
            git: self.git,
            protected_roots: self.protected_roots,
            coordinator: self.coordinator,
            limits: self.limits,
            options: release::ObservationSessionOptions {
                allow_anchor_write: self.write_coordinator_anchor,
                writer: store::WriterOptions {
                    allow_generation_advance: self.advance_writer_generation,
                    session: store::SessionOptions {
                        allow_local_reservation: self.reserve_local_coordination,
                        read: self.read,
                    },
                },
                observation: release::RecordObservationOptions {
                    read: release::RemoteReadOptions {
                        allow_read: self.allow_read,
                        allow_network: self.allow_network,
                        timeout: Duration::from_millis(self.timeout_ms),
                        max_response_bytes: self.max_response_bytes,
                    },
                    record: release::RefObservationWriteOptions {
                        allow_record: self.record_observation,
                    },
                },
            },
        }) {
            Ok(outcome) => outcome,
            Err(error) => {
                return crate::tools::failure(error.code(), error.message_text(), maximum);
            }
        };
        Ok(
            crate::tools::encode_complete(&outcome.report(), outcome.is_failure(), maximum)
                .unwrap_or(fallback),
        )
    }
}

#[cfg(test)]
#[path = "../tests/internal/observation.rs"]
mod tests;
