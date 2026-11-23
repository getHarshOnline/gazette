//! One-shot native storage observations, never held sessions or SQL-write capabilities.

use std::path::PathBuf;

use gazette_cli::command::{self, Kernel};
use gazette_cli::sdk::store::{self, JournalHeader, ReadOptions};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Store,
    Journal,
    Registry,
    Materialization,
    Authorization,
    Leases,
    Consumption,
    Attempts,
    Reconciliations,
}

impl Route {
    pub(super) const ALL: [Self; 9] = [
        Self::Store,
        Self::Journal,
        Self::Registry,
        Self::Materialization,
        Self::Authorization,
        Self::Leases,
        Self::Consumption,
        Self::Attempts,
        Self::Reconciliations,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Store => "inspect_store",
            Self::Journal => "inspect_journal",
            Self::Registry => "inspect_registry",
            Self::Materialization => "inspect_materialization",
            Self::Authorization => "inspect_release_authorization",
            Self::Leases => "inspect_release_leases",
            Self::Consumption => "inspect_release_consumption",
            Self::Attempts => "inspect_release_attempts",
            Self::Reconciliations => "inspect_release_reconciliations",
        }
    }

    fn history_kind(self) -> Option<command::observation::HistoryKind> {
        match self {
            Self::Authorization => Some(command::observation::HistoryKind::Authorization),
            Self::Leases => Some(command::observation::HistoryKind::Leases),
            Self::Consumption => Some(command::observation::HistoryKind::Consumption),
            Self::Attempts => Some(command::observation::HistoryKind::Attempts),
            Self::Reconciliations => Some(command::observation::HistoryKind::Reconciliations),
            Self::Store | Self::Journal | Self::Registry | Self::Materialization => None,
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema, output) = match self {
            Self::Store => (
                "Use inspect_store to see what kind of SQLite database a file is, and what its header says, before you trust it as a Gazette journal. One call observes raw SQLite provider/header metadata through SDK store::observe, as CLI inspect-store does. This does not validate Gazette schema, integrity, registry rows or ownership.",
                schemars::schema_for!(Request<Observe>),
                crate::tools::output_schema::<store::Observation>(),
            ),
            Self::Journal => (
                "Use inspect_journal to learn whether a file is a sound Gazette journal and where its generation stands. One call inspects the exact supported Gazette journal schema, integrity and coordinates through SDK store::inspect_journal, as CLI inspect-journal does. A supplied expected_header is compared in full; omitted/null means observation only. Matching coordinates do not prove freshness against copied/restored state or acquire a writer fence.",
                schemars::schema_for!(Request<Expected>),
                crate::tools::output_schema::<store::JournalInspection>(),
            ),
            Self::Registry => (
                "Use inspect_registry to check a journal's stored private registry against its recorded identity. One call inspects and independently recomputes the complete stored private registry row identity under one verified journal snapshot through SDK store::inspect_registry, as CLI inspect-registry does. Returns the journal and optional registry head, not a row dump. A supplied expected_header is compared in full; omitted/null means observation only. No reconciliation, public admission or release authority is granted.",
                schemars::schema_for!(Request<Expected>),
                crate::tools::output_schema::<store::RegistryInspection>(),
            ),
            Self::Materialization => (
                "Use inspect_materialization to see what a journal recorded about writing configured output: intent, completion and frontier. One call inspects verified durable configured-output intent, completion and frontier history through SDK store::inspect_materialization. No output directory is opened or canonicalized; history does not prove current bytes, ownership, permission, completion of a pending intent or retry authority. A supplied expected_header is compared in full; omitted/null means observation only.",
                schemars::schema_for!(Request<Materialization>),
                crate::tools::output_schema::<store::MaterializationInspection>(),
            ),
            Self::Authorization => (
                "Use inspect_release_authorization to see what pending release authorization a journal holds and when it was created. One call reads exact pending authorization custody and original creation coordinates through SDK store::inspect_release_authorization. Custody is not consumption, signature reapproval or dispatch permission; historical expiry does not renew authority.",
                schemars::schema_for!(Request<History>),
                crate::tools::output_schema::<store::ReleaseAuthorizationInspection>(),
            ),
            Self::Leases => (
                "Use inspect_release_leases to see which destination leases a journal recorded. One call reads original complete destination lease-set history through SDK store::inspect_release_leases. Historical generations remain inspectable after succession; this does not acquire, renew or prove an active lease.",
                schemars::schema_for!(Request<History>),
                crate::tools::output_schema::<store::ReleaseLeaseInspection>(),
            ),
            Self::Consumption => (
                "Use inspect_release_consumption to see whether and how a release authorization was consumed. One call reads immutable one-use consumed intent and original target linkage through SDK store::inspect_release_consumption. This does not consume again, reactivate authority or dispatch effects.",
                schemars::schema_for!(Request<History>),
                crate::tools::output_schema::<store::ReleaseConsumptionInspection>(),
            ),
            Self::Attempts => (
                "Use inspect_release_attempts to see what release attempts a journal recorded and how each ended. One call reads complete ordered durable attempt starts and optional results through SDK store::inspect_release_attempts. Missing results remain explicit, never inferred as success or failure; this does not retry or sample destinations.",
                schemars::schema_for!(Request<History>),
                crate::tools::output_schema::<store::ReleaseAttemptInspection>(),
            ),
            Self::Reconciliations => (
                "Use inspect_release_reconciliations to see how a release attempt was reconciled with its destinations. One call reads and verifies the complete immutable reconciliation chain against original consumed intent and selected historical attempt evidence through SDK store::inspect_release_reconciliations. Preserves historical missing results and stored conclusions; does not resample providers, settle again or restore authority.",
                schemars::schema_for!(Request<History>),
                crate::tools::output_schema::<store::ReleaseReconciliationInspection>(),
            ),
        };
        // Native PathBuf owns decoding; discovery additionally states this transport's
        // explicit absolute-path constraint, with no current-directory inference.
        schema.ensure_object()["properties"]["path"]["pattern"] = "^/".into();
        if matches!(self, Self::Materialization) {
            schema.ensure_object()["properties"]["output_root"]["pattern"] = "^/".into();
        }
        Tool::new(
            self.name(),
            format!("{meaning} Select an existing absolute database file in a trusted private directory. Both allow_read and read.allow_coordination_files are required: SQL is read-only, but native WAL/shared-memory coordination files may be maintained. Native read ceilings apply; busy_timeout_ms is per lock attempt, not a total deadline. No database creation, migration, repair, generation advance, session reservation, arbitrary SQL or network. Failure/response loss is not rollback evidence; no partial report is returned. If a call fails, correct the selection or permission and ask again, and never read a failure as evidence of what the database holds. These tools only observe: releasing, restoring or repairing anything stays with the publisher."),
            schema.ensure_object().clone(),
        )
        .with_raw_output_schema(output)
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        if value.get("read").is_some_and(|value| !value.is_object())
            || value
                .get("expected_header")
                .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err(invalid());
        }
        let (request, record_id, output_root) = match self {
            Self::Store => serde_json::from_value::<Request<Observe>>(value)
                .map(|request| (request.with_expected(None), None, None)),
            Self::Journal | Self::Registry => serde_json::from_value::<Request<Expected>>(value)
                .map(|request| {
                    let expected = request.phase.expected_header;
                    (request.with_expected(expected), None, None)
                }),
            Self::Materialization => {
                serde_json::from_value::<Request<Materialization>>(value).map(|request| {
                    let Materialization {
                        output_root,
                        expected_header,
                    } = request.phase;
                    (
                        Request {
                            path: request.path,
                            allow_read: request.allow_read,
                            read: request.read,
                            phase: expected_header,
                        },
                        None,
                        Some(output_root),
                    )
                })
            }
            Self::Authorization
            | Self::Leases
            | Self::Consumption
            | Self::Attempts
            | Self::Reconciliations => {
                serde_json::from_value::<Request<History>>(value).map(|request| {
                    let History {
                        expected_header,
                        record_id,
                    } = request.phase;
                    (
                        Request {
                            path: request.path,
                            allow_read: request.allow_read,
                            read: request.read,
                            phase: expected_header,
                        },
                        Some(record_id),
                        None,
                    )
                })
            }
        }
        .map_err(|_| invalid())?;
        if !request.path.is_absolute()
            || output_root.as_ref().is_some_and(|root| !root.is_absolute())
        {
            return Err(invalid());
        }
        Ok(Input {
            route: self,
            request,
            record_id,
            output_root,
        })
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid storage inspection arguments; use an absolute database path and named native read/header objects from inputSchema; no database opened",
        None,
    )
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request<P> {
    /// Existing absolute database file, not a journal directory or inferred workspace.
    path: PathBuf,
    /// Explicit native database-read permission; omitted means denied.
    #[serde(default)]
    allow_read: bool,
    /// Native SQL/storage/work ceilings and independent coordination-file permission.
    #[serde(default)]
    read: ReadOptions,
    #[serde(flatten)]
    phase: P,
}

#[derive(Deserialize, JsonSchema)]
struct Observe {}

#[derive(Deserialize, JsonSchema)]
struct Expected {
    /// Independently retained complete coordinates. Omitted/null only observes current state.
    #[serde(default)]
    expected_header: Option<JournalHeader>,
}

#[derive(Deserialize, JsonSchema)]
struct History {
    /// Exact durable authorization record identifier; no latest-record inference.
    record_id: String,
    /// Independently retained complete coordinates; omitted/null means observation only.
    #[serde(default)]
    expected_header: Option<JournalHeader>,
}

#[derive(Deserialize, JsonSchema)]
struct Materialization {
    /// Exact absolute historical canonical destination coordinate; never resampled.
    output_root: PathBuf,
    /// Independently retained complete coordinates; omitted/null means observation only.
    #[serde(default)]
    expected_header: Option<JournalHeader>,
}

impl<P> Request<P> {
    fn with_expected(self, expected: Option<JournalHeader>) -> Request<Option<JournalHeader>> {
        Request {
            path: self.path,
            allow_read: self.allow_read,
            read: self.read,
            phase: expected,
        }
    }
}

pub(super) struct Input {
    route: Route,
    request: Request<Option<JournalHeader>>,
    record_id: Option<String>,
    output_root: Option<PathBuf>,
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let request = self.request;
        if !request.allow_read {
            return crate::tools::failure(
                "storage_permission",
                "Explicit database-read permission required; no database opened",
                maximum,
            );
        }
        match self.route {
            Route::Store => match Kernel::new().execute(command::store::Observe {
                path: request.path,
                options: request.read,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
            },
            Route::Journal => match Kernel::new().execute(command::store::InspectJournal {
                path: request.path,
                expected: request.phase,
                options: request.read,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
            },
            Route::Registry => match Kernel::new().execute(command::store::InspectRegistry {
                path: request.path,
                expected: request.phase,
                options: request.read,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
            },
            Route::Materialization => {
                match Kernel::new().execute(command::store::InspectMaterialization {
                    path: request.path,
                    root: self.output_root.ok_or_else(invalid)?,
                    expected: request.phase,
                    options: request.read,
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
            route => {
                let kind = route.history_kind().ok_or_else(|| {
                    ErrorData::internal_error("Storage route projection mismatch", None)
                })?;
                let record_id = self.record_id.ok_or_else(invalid)?;
                match Kernel::new().execute(command::observation::InspectHistory {
                    allow_native_reads: true,
                    path: request.path,
                    record_id,
                    expected_header: request.phase,
                    read: request.read,
                    kind,
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => {
                        crate::tools::failure(error.code(), error.message_text(), maximum)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/storage.rs"]
mod tests;
