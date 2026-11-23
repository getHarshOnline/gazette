//! Bounded preparation observations through the same native readers as the CLI.

use std::path::PathBuf;

use gazette_cli::command::{Kernel, release_artifact as command};
use gazette_cli::sdk::{
    release_commit::{self, JournalWorktreeInspectionOptions, WorktreeLimits},
    source::{CommitId, Sha256Digest},
    store::{JournalHeader, ReadOptions},
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Checkpoint,
    Worktree,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Checkpoint, Self::Worktree];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Checkpoint => "inspect_release_checkpoint",
            Self::Worktree => "inspect_journal_worktree",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema, output) = match self {
            Self::Checkpoint => (
                "Use inspect_release_checkpoint to see what commit checkpoint a journal recorded. One call inspects an exact historical local commit checkpoint through SDK release_commit::inspect_checkpoint, as CLI inspect-release-checkpoint does. An absent record remains absent. This does not verify current Git objects or certify a prepared release.",
                schemars::schema_for!(Request<Checkpoint>),
                crate::tools::output_schema::<release_commit::CheckpointInspection>(),
            ),
            Self::Worktree => (
                "Use inspect_journal_worktree to compare a recorded preparation with the Git worktree and output as they are now. One call joins an exact recorded preparation to current local Git/output evidence through SDK release_commit::inspect_journal_worktree, as CLI inspect-journal-worktree does. The immutable journal recipe owns the worktree path and output expectation; callers supply the exact commit and every configured protected root. Matching bracketed observations are not an atomic snapshot or proof of anti-rollback. Missing completion remains missing, even when current bytes verify.",
                schemars::schema_for!(Request<Worktree>),
                crate::tools::output_schema::<release_commit::JournalWorktreeInspection>(),
            ),
        };
        schema.ensure_object()["properties"]["path"]["pattern"] = "^/".into();
        if matches!(self, Self::Worktree) {
            schema.ensure_object()["properties"]["protected_roots"]["items"]["pattern"] =
                "^/".into();
        }
        Tool::new(
            self.name(),
            format!("{meaning} Select an existing absolute database file in a trusted private directory. Both allow_read and read.allow_coordination_files are required: SQL is read-only, but SQLite may maintain WAL/shared-memory coordination files. expected_header is an independently retained complete expectation; omitted/null observes without a freshness claim. Native limits and server response bounds apply. Returned evidence contains private paths and is not public output. No SQL/Git data writes, initialization, migration, repair, synchronization, completion append, writer acquisition, generation advance, network, signing or release. Failure or response loss proves neither rollback nor absence of earlier effects; no partial report is returned. If a call fails, correct the selection or permission and ask again. Whether a prepared release goes ahead stays with the publisher."),
            schema.ensure_object().clone(),
        )
        .with_raw_output_schema(output)
        .with_annotations(ToolAnnotations::new().read_only(false).destructive(false).idempotent(true).open_world(true))
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        for pointer in [
            "/read",
            "/worktree_limits",
            "/worktree_limits/output",
            "/worktree_limits/output/content",
            "/worktree_limits/history",
        ] {
            if value
                .pointer(pointer)
                .is_some_and(|value| !value.is_object())
            {
                return Err(invalid());
            }
        }
        if value
            .get("expected_header")
            .is_some_and(|value| !value.is_null() && !value.is_object())
        {
            return Err(invalid());
        }
        match self {
            Self::Checkpoint => {
                let request: Request<Checkpoint> =
                    serde_json::from_value(value).map_err(|_| invalid())?;
                request.validate_path()?;
                Ok(Input::Checkpoint(request))
            }
            Self::Worktree => {
                let request: Request<Worktree> =
                    serde_json::from_value(value).map_err(|_| invalid())?;
                request.validate_path()?;
                if request
                    .phase
                    .protected_roots
                    .iter()
                    .any(|root| !root.is_absolute())
                {
                    return Err(invalid());
                }
                Ok(Input::Worktree(request))
            }
        }
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid preparation inspection arguments; use absolute paths, exact native coordinates and named options from inputSchema; no native inspection started",
        None,
    )
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request<P> {
    /// Existing absolute database file, not its containing journal directory.
    path: PathBuf,
    /// Permit this native inspection; omission denies access.
    #[serde(default)]
    allow_read: bool,
    /// Bounded native database reads and independent `SQLite` coordination permission.
    #[serde(default)]
    read: ReadOptions,
    /// Independently retained complete coordinates; omitted/null only observes.
    #[serde(default)]
    expected_header: Option<JournalHeader>,
    #[serde(flatten)]
    phase: P,
}

impl<P> Request<P> {
    fn validate_path(&self) -> Result<(), ErrorData> {
        if self.path.is_absolute() {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Checkpoint {
    /// Exact historical checkpoint identity; no latest-record discovery.
    checkpoint_id: Sha256Digest,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Worktree {
    /// Exact immutable preparation identity, never a caller-supplied recipe.
    preparation_id: Sha256Digest,
    /// Independently selected exact commit OID; no moving references.
    commit: CommitId,
    /// Complete configured source roots that must remain separate from native storage.
    protected_roots: Vec<PathBuf>,
    /// Native output, administrative metadata and complete parent-history ceilings.
    #[serde(default)]
    worktree_limits: WorktreeLimits,
}

pub(super) enum Input {
    Checkpoint(Request<Checkpoint>),
    Worktree(Request<Worktree>),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Checkpoint(request) => match Kernel::new().execute(command::InspectCheckpoint {
                allow_native_reads: request.allow_read,
                path: request.path,
                checkpoint_id: request.phase.checkpoint_id,
                expected_header: request.expected_header,
                read: request.read,
            }) {
                Ok(report) => crate::tools::encode(&report, false, maximum),
                Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
            },
            Self::Worktree(request) => {
                match Kernel::new().execute(command::InspectJournalWorktree {
                    allow_native_reads: request.allow_read,
                    database: request.path,
                    preparation_id: request.phase.preparation_id,
                    expected_header: request.expected_header,
                    commit: request.phase.commit,
                    protected_roots: request.phase.protected_roots,
                    options: JournalWorktreeInspectionOptions {
                        read: request.read,
                        worktree: request.phase.worktree_limits,
                    },
                }) {
                    Ok(report) => crate::tools::encode(&report, false, maximum),
                    Err(error) => crate::tools::failure(error.code(), &error.to_string(), maximum),
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/preparation_history.rs"]
mod tests;
