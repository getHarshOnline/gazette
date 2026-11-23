//! Exact native journal writes; transports neither reconstruct history nor retry effects.

use std::path::PathBuf;

use gazette_cli::command::{Kernel, release_artifact as command};
use gazette_cli::sdk::{release_commit as commit, source, store};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy)]
pub(super) enum Route {
    Checkpoint,
    Recover,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Checkpoint, Self::Recover];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Checkpoint => "checkpoint_release_commit",
            Self::Recover => "recover_journal_worktree",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (meaning, mut schema, output) = match self {
            Self::Checkpoint => (
                "Use checkpoint_release_commit to record in the journal the Git commit you prepared for a release. One call verifies exact existing Git objects and records/replays their immutable checkpoint through the same SDK owner as CLI checkpoint-release-commit. No Git writes, worktree creation or release preparation.",
                schemars::schema_for!(Request<Checkpoint>),
                crate::tools::output_schema::<commit::journal_output::CheckpointReport<'static>>(),
            ),
            Self::Recover => (
                "Use recover_journal_worktree to finish a release worktree whose completion was interrupted. One call recovers exact existing journal-bound worktree completion through the same SDK recorded-recipe owner as CLI recover-journal-worktree. The recorded preparation alone owns worktree address, output, metadata and check binding; no replacement recipe is accepted. Synchronizes existing output/Git storage and records/replays exact completion; never recreates, repairs, rerenders or infers a commit from HEAD.",
                schemars::schema_for!(Request<Recover>),
                crate::tools::output_schema::<commit::journal_output::RecoveryReport<'static>>(),
            ),
        };
        if matches!(self, Self::Checkpoint) {
            crate::artifact::metadata_schema(&mut schema);
        }
        Tool::new(self.name(), format!("{meaning} Require independent allow_read, generation advance, local reservation, SQLite coordination-file permission and the operation's record/synchronization permissions. Supply exact expected_header, explicit absolute paths and every protected root. Complete native bounds apply. Response custody is reserved before effects; failed or compact delivery remains an error preserving original coordinates/progress. A generation may advance before a later failure. No initialization, migration, authorization, signing, network or publication. Never infer rollback or retry after failure, cancellation or lost delivery; inspect original native state. Whether the commit is ever released stays with the publisher."), schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        for pointer in [
            "/expected_header",
            "/read",
            "/expected_tree",
            "/objects",
            "/objects/content",
            "/worktree_limits",
            "/worktree_limits/output",
            "/worktree_limits/output/content",
            "/worktree_limits/history",
        ] {
            if value.pointer(pointer).is_some_and(|v| !v.is_object()) {
                return Err(invalid());
            }
        }
        let input = match self {
            Self::Checkpoint => serde_json::from_value(value).map(Input::Checkpoint),
            Self::Recover => serde_json::from_value(value).map(Input::Recover),
        }
        .map_err(|_| invalid())?;
        let valid = match &input {
            Input::Checkpoint(r) => {
                r.paths_valid() && r.mode.root.is_absolute() && r.mode.repository.is_absolute()
            }
            Input::Recover(r) => r.paths_valid() && r.mode.database.is_absolute(),
        };
        if !valid {
            return Err(invalid());
        }
        Ok(input)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request<Mode> {
    /// Explicit permission for configured local native reads; no network or discovery.
    allow_read: bool,
    /// Independently retained complete current journal expectation, never inferred.
    expected_header: store::JournalHeader,
    /// Permit generation advance independently from history writes or synchronization.
    advance_writer_generation: bool,
    /// Permit the native writer's exclusive local coordination lifetime.
    reserve_local_coordination: bool,
    /// Native SQL ceilings and independently selected coordination-file permission.
    #[serde(default)]
    read: store::ReadOptions,
    /// Complete explicitly selected protected source/output roots, each absolute.
    protected_roots: Vec<PathBuf>,
    #[serde(flatten)]
    mode: Mode,
}

impl<Mode> Request<Mode> {
    fn paths_valid(&self) -> bool {
        self.protected_roots.iter().all(|path| path.is_absolute())
    }

    fn writer(&self) -> store::WriterOptions {
        store::WriterOptions {
            allow_generation_advance: self.advance_writer_generation,
            session: store::SessionOptions {
                allow_local_reservation: self.reserve_local_coordination,
                read: self.read,
            },
        }
    }
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Checkpoint {
    /// Existing private journal directory; not the `SQLite` database file.
    root: PathBuf,
    /// Existing independent repository; only exact native objects are read.
    repository: PathBuf,
    commit: source::CommitId,
    checkpoint_id: source::Sha256Digest,
    expected_tree: source::TreeIdentity,
    /// Exact native recipe JSON, not a materialized map or changed history recipe.
    metadata_json: String,
    #[serde(default = "crate::artifact::metadata_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_metadata_bytes: usize,
    record_checkpoint: bool,
    #[serde(default)]
    objects: source::RevisionLimits,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Recover {
    /// Existing native journal database file; its immutable record supplies the recipe.
    database: PathBuf,
    preparation_id: source::Sha256Digest,
    commit: source::CommitId,
    record_preparation: bool,
    synchronize_worktree: bool,
    #[serde(default)]
    worktree_limits: commit::WorktreeLimits,
    #[serde(default = "storage_bytes")]
    #[schemars(range(max = u64::MAX))]
    storage_bytes: u64,
}

fn storage_bytes() -> u64 {
    commit::WorktreeSyncOptions::default().storage_bytes
}

pub(super) enum Input {
    Checkpoint(Request<Checkpoint>),
    Recover(Request<Recover>),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let outcome = match self {
            Self::Checkpoint(r) => {
                let writer = r.writer();
                Kernel::new().execute(command::CheckpointCommit {
                    allow_native_reads: r.allow_read,
                    root: r.mode.root,
                    expected_header: r.expected_header,
                    checkpoint_id: r.mode.checkpoint_id,
                    repository: r.mode.repository,
                    commit: r.mode.commit,
                    expected_tree: r.mode.expected_tree,
                    metadata_json: r.mode.metadata_json.into_bytes(),
                    max_metadata_bytes: r.mode.max_metadata_bytes,
                    protected_roots: r.protected_roots,
                    options: commit::CheckpointOptions {
                        objects: r.mode.objects,
                        record: commit::CheckpointWriteOptions {
                            allow_record: r.mode.record_checkpoint,
                        },
                        writer,
                    },
                    response_bytes: maximum,
                })
            }
            Self::Recover(r) => {
                let writer = r.writer();
                Kernel::new().execute(command::RecoverJournalWorktree {
                    allow_native_reads: r.allow_read,
                    database: r.mode.database,
                    preparation_id: r.mode.preparation_id,
                    expected_header: r.expected_header,
                    commit: r.mode.commit,
                    protected_roots: r.protected_roots,
                    options: commit::RecoverWorktreeOptions {
                        record: store::PreparationWriteOptions {
                            allow_record: r.mode.record_preparation,
                        },
                        writer,
                        worktree: commit::WorktreeSyncOptions {
                            allow_synchronize: r.mode.synchronize_worktree,
                            limits: r.mode.worktree_limits,
                            storage_bytes: r.mode.storage_bytes,
                        },
                    },
                    response_bytes: maximum,
                })
            }
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) if error.reconciliation_required() => return Err(delivery()),
            Err(error) => {
                return crate::tools::failure(error.code(), error.message_text(), maximum);
            }
        };
        let failed = outcome.is_failure();
        crate::tools::encoded(outcome.into_bytes(), failed).map_err(|_| delivery())
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid journal commit arguments or insufficient response reservation; use exact coordinates, absolute roots and named options; no native operation attempted",
        None,
    )
}

fn delivery() -> ErrorData {
    ErrorData::internal_error(
        "Native journal result delivery contract failed; preserve original coordinates and inspect native effects; never infer rollback or retry",
        None,
    )
}
