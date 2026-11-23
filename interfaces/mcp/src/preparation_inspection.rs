//! Complete native preparation inspection and signature joins, never release activation.

use std::path::PathBuf;

use gazette_cli::command::{Kernel, release_artifact as command};
use gazette_cli::sdk::{release, store::JournalHeader};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Inspect,
    Verify,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request<P> {
    /// Existing absolute private database file, never an inferred workspace or journal directory.
    #[schemars(regex(pattern = "^/"))]
    database: PathBuf,
    /// Complete independently retained current journal coordinates; always compared exactly.
    expected_header: JournalHeader,
    /// Exact complete ordered selection-array JSON. Raw strings preserve duplicate-key evidence.
    selections_json: String,
    /// All configured inputs that must remain separate from the journal and recorded worktrees.
    #[serde(default)]
    protected_roots: Vec<PathBuf>,
    /// Explicit permission for native journal, recorded evidence and Git/worktree reads.
    #[serde(default)]
    allow_read: bool,
    /// Native complete-set, evidence, storage, output and history bounds. `SQLite` coordination-file
    /// permission remains independent and defaults to denied inside inspection.read.
    #[serde(default)]
    options: release::PreparationBatchOptions,
    #[serde(flatten)]
    phase: P,
}

#[derive(Deserialize, JsonSchema)]
struct Inspection {}

#[derive(Deserialize, JsonSchema)]
struct Verification {
    /// Exact received authorization JSON, not normalized through a transport object.
    authorization_json: String,
    /// Independently supplied binding JSON, including trusted keys, time and exact expectations.
    /// This is not native authority acquisition and must never be derived from the signed body.
    binding_json: String,
    /// Native signature/input work bounds, separate from native preparation inspection bounds.
    #[serde(default)]
    limits: release::Limits,
}

pub(super) struct Input {
    request: Request<Option<Verification>>,
}

impl<P> Request<P> {
    fn with_phase(self, phase: Option<Verification>) -> Request<Option<Verification>> {
        Request {
            database: self.database,
            expected_header: self.expected_header,
            selections_json: self.selections_json,
            protected_roots: self.protected_roots,
            allow_read: self.allow_read,
            options: self.options,
            phase,
        }
    }
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Inspect, Self::Verify];

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Inspect => "inspect_release_preparations",
            Self::Verify => "verify_prepared_authorization",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (mut schema, output, meaning) = match self {
            Self::Inspect => (
                schemars::schema_for!(Request<Inspection>),
                crate::tools::output_schema::<release::PreparationBatch>(),
                "Use inspect_release_preparations to see every retained local preparation of a release, with its evidence. One call inspects one complete ordered set of retained local preparations through the same SDK as CLI inspect-release-preparations. Joins journal history, exact current worktrees/objects, output and original check evidence. Returns private targets and evidence identities, not public admission or certification.",
            ),
            Self::Verify => (
                schemars::schema_for!(Request<Verification>),
                crate::tools::output_schema::<release::PreparedAuthorizationReport>(),
                "Use verify_prepared_authorization to check a supplied release authorization against the preparations a journal holds. One call verifies exact supplied authorization against an independently supplied binding and the complete native local preparation set through the same SDK as CLI verify-prepared-authorization. Raw authorization/binding JSON reaches the native signature verifier unchanged. No key admission, current authority acquisition or remote freshness is inferred. A matching signature and native join are not certification or release activation.",
            ),
        };
        let properties = schema.ensure_object()["properties"]
            .as_object_mut()
            .expect("named preparation request schema");
        properties
            .get_mut("protected_roots")
            .expect("protected root selection")["items"]["pattern"] = "^/".into();
        let selections = properties
            .get_mut("selections_json")
            .and_then(Value::as_object_mut)
            .expect("raw selection-array field");
        selections.insert("contentMediaType".into(), "application/json".into());
        selections.insert(
            "contentSchema".into(),
            serde_json::to_value(
                schemars::generate::SchemaSettings::draft2020_12()
                    .for_serialize()
                    .with(|settings| settings.inline_subschemas = true)
                    .into_generator()
                    .into_root_schema_for::<Vec<release::PreparationSelection>>(),
            )
            .expect("native selection schema"),
        );
        Tool::new(
            self.name(),
            format!("{meaning} Requires allow_read and independent options.inspection.read.allow_coordination_files. Use absolute selected database/protected paths; no current-directory inference. Native limits apply independently of MCP request/response ceilings. The entire selection succeeds or returns a redacted native failure, never a successful subset. SQL reads may maintain local WAL/shared-memory coordination files; cancellation or carrier loss does not prove absence of those effects. No initialization, migration, repair, reservation, generation advance, synchronization, source acquisition, signing, leases, consumption, provider/network invocation or push. If the selection fails, nothing is returned for any item: correct it and ask again. Whether to release stays with the publisher."),
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
        if value.get("expected_header").is_none_or(|v| !v.is_object())
            || ["options", "limits"].iter().any(|key| {
                value
                    .get(*key)
                    .is_some_and(|v| !v.is_object() || !named_options(v))
            })
        {
            return Err(invalid());
        }
        let request = match self {
            Self::Inspect => serde_json::from_value::<Request<Inspection>>(value)
                .map(|request| request.with_phase(None)),
            Self::Verify => serde_json::from_value::<Request<Verification>>(value).map(|request| {
                let Request {
                    database,
                    expected_header,
                    selections_json,
                    protected_roots,
                    allow_read,
                    options,
                    phase,
                } = request;
                Request {
                    database,
                    expected_header,
                    selections_json,
                    protected_roots,
                    allow_read,
                    options,
                    phase: Some(phase),
                }
            }),
        }
        .map_err(|_| invalid())?;
        if !request.database.is_absolute()
            || request
                .protected_roots
                .iter()
                .any(|path| !path.is_absolute())
        {
            return Err(invalid());
        }
        Ok(Input { request })
    }
}

// These native bounds contain named objects and scalars, never positional structures or lists.
// Field names, value types and defaults remain owned by the native deserializers.
fn named_options(value: &Value) -> bool {
    match value {
        Value::Object(fields) => fields.values().all(named_options),
        Value::Array(_) => false,
        _ => true,
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid preparation inspection arguments; use inputSchema's named objects, raw JSON strings and absolute paths; no native inspection performed",
        None,
    )
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let request = self.request;
        let failure = |code: &str| {
            crate::tools::failure(
                code,
                "No complete preparation verification report returned; no signing or release effects",
                maximum,
            )
        };
        if let Some(verification) = request.phase {
            match Kernel::new().execute(command::VerifyPreparedAuthorization {
                allow_native_reads: request.allow_read,
                database: request.database,
                expected_header: request.expected_header,
                selections_json: request.selections_json,
                protected_roots: request.protected_roots,
                preparation_options: request.options,
                authorization_json: verification.authorization_json,
                binding_json: verification.binding_json,
                authorization_limits: verification.limits,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => failure(error.code()),
            }
        } else {
            match Kernel::new().execute(command::InspectPreparations {
                allow_native_reads: request.allow_read,
                database: request.database,
                expected_header: request.expected_header,
                selections_json: request.selections_json,
                protected_roots: request.protected_roots,
                options: request.options,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => failure(error.code()),
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/preparation_inspection.rs"]
mod tests;
