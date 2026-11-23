//! One-shot store effects project native SDK permissions and lossless creation evidence.

use std::path::PathBuf;

use gazette_cli::command::{self, Kernel};
use gazette_cli::sdk::{
    source::Sha256Digest,
    store::{self, output},
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Initialize,
    Backup,
    Verify,
    Restore,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Initialize {
    /// Existing empty private directory; never an inferred workspace or live store.
    directory: PathBuf,
    #[serde(default)]
    options: store::InitializeOptions,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Backup {
    /// Existing `SQLite` file; never initialized or repaired.
    source: PathBuf,
    /// Existing private output parent; each authorized call creates fresh output.
    parent: PathBuf,
    #[schemars(length(min = 1))]
    authored_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    staged_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    published_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    live_journal_roots: Vec<PathBuf>,
    #[serde(default)]
    allow_read: bool,
    #[serde(default)]
    options: store::BackupOptions,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Verify {
    directory: PathBuf,
    /// Pin retained from the original receipt, not recomputed from suspect output.
    manifest_sha256: Sha256Digest,
    #[serde(default)]
    allow_read: bool,
    #[serde(default)]
    options: store::BackupVerifyOptions,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Restore {
    /// Independently pinned backup directory, not a live store.
    backup: PathBuf,
    manifest_sha256: Sha256Digest,
    /// Existing private parent for a fresh detached recovery copy.
    parent: PathBuf,
    #[schemars(length(min = 1))]
    authored_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    staged_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    published_roots: Vec<PathBuf>,
    #[schemars(length(min = 1))]
    live_journal_roots: Vec<PathBuf>,
    #[serde(default)]
    allow_read: bool,
    #[serde(default)]
    options: store::RestoreOptions,
}

impl Route {
    pub(super) const ALL: [Self; 4] = [Self::Initialize, Self::Backup, Self::Verify, Self::Restore];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Initialize => "initialize_store",
            Self::Backup => "backup_store",
            Self::Verify => "verify_backup",
            Self::Restore => "prepare_store_restore",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema, result, paths) = match self {
            Self::Initialize => (
                "Use initialize_store to create a fresh, empty journal. One call initializes one fresh journal in an existing empty private directory through SDK store::initialize. Native allow_create, reservation and coordination permissions are independently required. Never overwrite, migrate, enroll authority or activate a recovered store.",
                schemars::schema_for!(Initialize),
                crate::tools::output_schema::<output::Report<'static, store::InitializationReceipt>>(
                ),
                vec!["directory"],
            ),
            Self::Backup => (
                "Use backup_store to take a consistent copy of an existing journal. One call copies a consistent existing SQLite snapshot into a newly created private backup directory through SDK store::backup. Explicit allow_read and native creation/coordination permissions are required. Retain the original manifest pin independently; no retry, replacement or restore is implied.",
                schemars::schema_for!(Backup),
                crate::tools::output_schema::<output::Report<'static, store::BackupReceipt>>(),
                vec!["source", "parent"],
            ),
            Self::Verify => (
                "Use verify_backup to confirm that a backup still matches the manifest pin you retained when you made it. One call verifies an existing backup against the original independently retained manifest pin through SDK store::verify_backup. Explicit allow_read and native coordination permission are required. SQL is read-only, but coordination files may be maintained. Never repair, migrate, restore, create a database or renew release authority.",
                schemars::schema_for!(Verify),
                crate::tools::output_schema::<store::BackupInspection>(),
                vec!["directory"],
            ),
            Self::Restore => (
                "Use prepare_store_restore to prepare a detached recovery copy from a pinned backup without touching the live store. One call prepares a fresh detached recovery copy from the exact pinned backup through SDK store::prepare_restore. Explicit allow_read and native allow_prepare/coordination permissions are required. Never replace or activate a live store, repair output, advance a writer generation or revive authority.",
                schemars::schema_for!(Restore),
                crate::tools::output_schema::<output::Report<'static, store::RestoreReceipt>>(),
                vec!["backup", "parent"],
            ),
        };
        for path in paths {
            schema.ensure_object()["properties"][path]["pattern"] = "^/".into();
        }
        if matches!(self, Self::Backup | Self::Restore) {
            for roots in [
                "authored_roots",
                "staged_roots",
                "published_roots",
                "live_journal_roots",
            ] {
                schema.ensure_object()["properties"][roots]["items"]["pattern"] = "^/".into();
            }
        }
        let verifies = matches!(self, Self::Verify);
        Tool::new(self.name(), format!("{meaning} Use explicit absolute native paths in a trusted private namespace. Native work/byte bounds apply. Creation tools reserve lossless output-locator and original-pin response capacity before work and invoke the native operation once. Failed work may leave private residue; delivery failure is not rollback or permission to retry. No cleanup, network, publication or release effects. Whether the result is ever used or restored, and any release that follows, stays with the operator and publisher."), schema.ensure_object().clone())
            .with_raw_output_schema(result)
            .with_annotations(ToolAnnotations::new().read_only(false).destructive(!verifies).idempotent(verifies).open_world(true))
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        if value
            .get("options")
            .is_some_and(|options| !options.is_object() || !named(options))
        {
            return Err(invalid());
        }
        let input = match self {
            Self::Initialize => {
                Input::Initialize(serde_json::from_value(value).map_err(|_| invalid())?)
            }
            Self::Backup => Input::Backup(serde_json::from_value(value).map_err(|_| invalid())?),
            Self::Verify => Input::Verify(serde_json::from_value(value).map_err(|_| invalid())?),
            Self::Restore => Input::Restore(serde_json::from_value(value).map_err(|_| invalid())?),
        };
        let absolute = match &input {
            Input::Initialize(request) => request.directory.is_absolute(),
            Input::Backup(request) => {
                request.source.is_absolute()
                    && request.parent.is_absolute()
                    && roots_absolute(request)
            }
            Input::Verify(request) => request.directory.is_absolute(),
            Input::Restore(request) => {
                request.backup.is_absolute()
                    && request.parent.is_absolute()
                    && restore_roots_absolute(request)
            }
        };
        if !absolute {
            return Err(invalid());
        }
        Ok(input)
    }
}

fn all_absolute(groups: [&[PathBuf]; 4]) -> bool {
    groups
        .into_iter()
        .all(|roots| !roots.is_empty() && roots.iter().all(|root| root.is_absolute()))
}

fn roots_absolute(request: &Backup) -> bool {
    all_absolute([
        &request.authored_roots,
        &request.staged_roots,
        &request.published_roots,
        &request.live_journal_roots,
    ])
}

fn restore_roots_absolute(request: &Restore) -> bool {
    all_absolute([
        &request.authored_roots,
        &request.staged_roots,
        &request.published_roots,
        &request.live_journal_roots,
    ])
}

fn protected(
    authored: Vec<PathBuf>,
    staged: Vec<PathBuf>,
    published: Vec<PathBuf>,
    live_journals: Vec<PathBuf>,
) -> command::store::LifecycleProtectedRoots {
    command::store::LifecycleProtectedRoots {
        authored,
        staged,
        published,
        live_journals,
    }
}

// These native options contain named structs and scalar leaves, never positional records.
fn named(value: &Value) -> bool {
    match value {
        Value::Array(_) => false,
        Value::Object(fields) => fields.values().all(named),
        _ => true,
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Use absolute native paths and the closed named option objects from inputSchema; no store operation started",
        None,
    )
}

pub(super) enum Input {
    Initialize(Initialize),
    Backup(Backup),
    Verify(Verify),
    Restore(Restore),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let allowed = match &self {
            Self::Initialize(_) => true,
            Self::Backup(request) => request.allow_read,
            Self::Verify(request) => request.allow_read,
            Self::Restore(request) => request.allow_read,
        };
        if !allowed {
            return crate::tools::failure(
                "storage_permission",
                "Explicit allow_read required; no store opened or output created",
                maximum,
            );
        }
        match self {
            Self::Initialize(request) => {
                let reserved = reservation(request.options.output, maximum)?;
                let result = Kernel::new().execute(command::store::Initialize {
                    directory: request.directory,
                    options: request.options,
                });
                complete(reserved.initialize(&result), maximum)
            }
            Self::Backup(request) => {
                let reserved = reservation(request.options.output, maximum)?;
                let result = Kernel::new().execute(command::store::Backup {
                    source: request.source,
                    parent: request.parent,
                    protected: protected(
                        request.authored_roots,
                        request.staged_roots,
                        request.published_roots,
                        request.live_journal_roots,
                    ),
                    options: request.options,
                });
                complete(reserved.backup(&result), maximum)
            }
            Self::Restore(request) => {
                let reserved = reservation(request.options.output, maximum)?;
                let result = Kernel::new().execute(command::store::PrepareRestore {
                    backup: request.backup,
                    manifest_sha256: request.manifest_sha256,
                    parent: request.parent,
                    protected: protected(
                        request.authored_roots,
                        request.staged_roots,
                        request.published_roots,
                        request.live_journal_roots,
                    ),
                    options: request.options,
                });
                complete(reserved.restore(&result), maximum)
            }
            Self::Verify(request) => {
                match Kernel::new().execute(command::store::VerifyBackup {
                    directory: request.directory,
                    manifest_sha256: request.manifest_sha256,
                    options: request.options,
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
        }
    }
}

fn reservation(
    limits: store::OutputLimits,
    maximum: usize,
) -> Result<output::Reservation, ErrorData> {
    output::Reservation::new(limits, maximum).map_err(|code| {
        ErrorData::invalid_params(
            format!(
                "{code}: native work not started; response must retain exact output coordinates"
            ),
            None,
        )
    })
}

fn complete<T: Serialize>(
    result: Result<output::Report<'_, T>, output::ContractError<'_, T>>,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    let report = result.map_err(|error| ErrorData::internal_error(
        "Store output delivery contract was violated after native work; original locator and evidence follow, without a bounded-delivery guarantee; do not infer rollback or retry",
        Some(serde_json::to_value(error.delivery).expect("native lossless delivery evidence")),
    ))?;
    crate::tools::encode_complete(&report, report.is_error(), maximum).map_err(|_| {
        ErrorData::internal_error("Store response encoding failed after native work; retain this original outcome and locator, do not infer rollback or automatically retry", Some(serde_json::to_value(&report).expect("same prebounded native store report")))
    })
}

#[cfg(test)]
#[path = "../tests/internal/store_lifecycle.rs"]
mod tests;
