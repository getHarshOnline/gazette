//! Lets an operator run Gazette from a terminal: the command shell over the Gazette SDK.
//!
//! This crate builds on `gazette-sdk`; `gazette-launcher` builds the `gazette`
//! executable on it, and `gazette-mcp` projects its commands as MCP tools. [`command`]
//! owns the typed command facade also consumed by MCP. This crate maps command-line
//! arguments, explicit permissions and bounded input into those operations, then
//! formats their outcomes. It does not maintain another workspace resolver or infer
//! permission from a configured path.

/// Transport-neutral typed command facade shared by CLI and MCP projections.
pub mod command;
/// Shared SDK contracts re-exported through the application-command boundary.
///
/// Protocol projections use this namespace for transport shapes while all
/// application operations remain owned by [`command`].
pub use gazette_sdk as sdk;

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use gazette_sdk::source::{self, CommitId, Limits, RevisionLimits, Subdirectory};
use serde::Serialize;

use crate::command::{
    Kernel,
    inspection::{SourceCommand, SourceRequest, SourceSelection},
};

mod backup;
pub mod calendar;
mod check;
mod config;
mod connect;
mod coordinator;
mod delivery;
mod documentation;
mod generated;
mod github;
mod initialize;
mod journal;
mod json_input;
mod markdown;
mod materialization;
mod prepare_sites;
mod preview;
mod probe_output;
mod qualify_sites;
mod recovery_session;
mod registry;
mod release;
mod release_commit;
mod release_history;
mod release_observation;
mod release_preparations;
mod release_readback;
mod release_session;
mod render_site;
mod render_sites;
mod restore;
mod schema;
mod session;
mod stage;
mod store;
mod store_output;
mod toml;
mod verify_backup;
mod writer;

#[derive(Parser)]
#[command(
    name = "gazette",
    version,
    about = "Native publication mechanics; no coordinator required"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Choose local Gazette preferences.
    Config(config::Args),
    /// Serve discoverable SDK and authoring tools over stdio; explicit native-read permissions, no publication.
    Mcp(McpArgs),
    /// Serve explicitly supplied or selected workspace/content diagnostics over stdio; no files are changed.
    Lsp,
    /// Verify a complete explicitly supplied Gazette editor context from private stdin.
    ImportEditorContext {
        /// Read saved workspace files when reconstructing an unsaved-text handoff.
        #[arg(long)]
        allow_read: bool,
    },
    /// Print a Draft 2020-12 schema generated from one native Gazette contract.
    Schema(schema::Args),
    /// Create and read back local Git objects only; no refs, journal, signing or network.
    #[command(name = "prepare-release-commit")]
    PrepareReleaseCommit(release_commit::PrepareArgs),
    /// Read back an exact local Git commit; no writes or public certification.
    #[command(name = "verify-release-commit")]
    VerifyReleaseCommit(release_commit::VerifyArgs),
    /// Record verified local commit evidence in `SQLite`; not a prepared release or authorization.
    CheckpointReleaseCommit(release_commit::CheckpointArgs),
    /// Inspect historical checkpoint evidence; no migration or current Git verification.
    InspectReleaseCheckpoint(release_commit::InspectCheckpointArgs),
    /// Join preparation history with exact current worktree inspection; no repair or release approval.
    InspectJournalWorktree(release_commit::InspectJournalWorktreeArgs),
    /// Synchronize an exact existing recorded worktree and record completion; no recreation or release.
    RecoverJournalWorktree(release_commit::RecoverJournalWorktreeArgs),
    /// Retain exact existing objects under local custody and synchronize storage; no branch or remote effects.
    RetainReleaseCommit(release_commit::RetainArgs),
    /// Inspect exact custody ref and objects without writes; no prior synchronization is inferred.
    InspectReleaseCustody(release_commit::InspectCustodyArgs),
    /// Verify private stdin signatures and exact supplied bindings; no clock, native reads, writes or release effects.
    #[command(name = "verify-release-authorization")]
    VerifyReleaseAuthorization(release::Args),
    /// Inspect a complete preparation set from private stdin; native reads, no certification or release.
    InspectReleasePreparations(release_preparations::InspectArgs),
    /// Verify private stdin authorization against exact local preparations; no signing or release effects.
    VerifyPreparedAuthorization(release_preparations::VerifyArgs),
    /// Observe exact native Git refs from private stdin; explicit reads, no push or settlement.
    ObserveReleaseTargets(release_readback::Args),
    /// Observe one exact public GitHub Pages build; explicit read/execute/network permissions, no deployment or settlement.
    ObserveGithubPagesBuild(github::Args),
    /// Compare selected served routes with pinned local output; stdin options.http must permit reads, execution and network. No publication or settlement.
    ObserveDelivery(delivery::Args),
    /// Record complete native observations in an existing private journal; no push or consumption.
    RecordReleaseObservation(release_observation::RecordArgs),
    /// Inspect immutable observation history without destination access or active signatures.
    InspectReleaseObservation(release_observation::InspectArgs),
    /// Read original authorization custody; no revalidation, signing or destination access.
    InspectReleaseAuthorization(release_history::Args),
    /// Read original destination leases; history does not renew their ownership.
    InspectReleaseLeases(release_history::Args),
    /// Read exact consumed intent without renewing authority or contacting destinations.
    InspectReleaseConsumption(release_history::Args),
    /// Read complete immutable attempts, retaining unresolved starts without retrying.
    InspectReleaseAttempts(release_history::Args),
    /// Read complete reconciliation and settlement history; no new observations or effects.
    InspectReleaseReconciliations(release_history::Args),
    /// Enroll a fresh local journal under a separate private anchor; no generation or release effects.
    EnrollLocalCoordinator(coordinator::EnrollArgs),
    /// Inspect exact retained local anchor state; no repair, adoption or journal resampling.
    InspectLocalCoordinator(coordinator::Args),
    /// Adopt an exact latest-clean detached restore into one fresh successor root.
    #[command(name = "adopt-store-restore")]
    AdoptStoreRestore(coordinator::AdoptArgs),
    /// Replace complete private registry state from retained workspace inputs; no rendering, migration or release.
    #[command(name = "reconcile-registry")]
    ReconcileRegistry(registry::ReconcileArgs),
    /// Verify complete private registry row identity and journal coordinates; no SQL writes or migration.
    #[command(name = "inspect-registry")]
    InspectRegistry(registry::InspectArgs),
    /// Inspect durable output ownership and pending intent; no writer, output reads or retry.
    #[command(name = "inspect-materialization")]
    InspectMaterialization(materialization::InspectArgs),
    /// Check or assemble a site's authored-manual and generated-reference repository locally; no publication.
    #[command(name = "assemble-docs")]
    Documentation(documentation::Args),
    /// Check `./gazette.toml` or explicit supplied workspace TOML; optional source reads, no writes or certification.
    Check(check::Args),
    /// Render configured pinned authored inputs locally; optional fresh output, no network, database or publication.
    #[command(name = "render-site")]
    RenderSite(render_site::Args),
    /// Probe an independently pinned output tree; private evidence, no rebuild, writes or certification.
    ProbeOutput(probe_output::Args),
    /// Render an explicit local site set from private stdin JSON; no staging, network or publication.
    #[command(name = "render-sites")]
    RenderSites(render_sites::Args),
    /// Materialize a complete local site set at configured outputs with durable history; no release.
    MaterializeSites(materialization::ExecuteArgs),
    /// Materialize retained configured documentation with original native custody and explicit close.
    MaterializeDocs(materialization::DocumentationArgs),
    /// Qualify every selected site with explicit candidate checks; no stages, store writes or release.
    QualifySites(qualify_sites::Args),
    /// Acquire and render an explicit candidate, then retain checked local Git preparations; no publication.
    PrepareSites(prepare_sites::Args),
    /// Serve an exact locally rendered site set on explicit loopback sockets until stdin byte/EOF.
    PreviewSites(preview::Args),
    /// Serve one fresh production-linked render on exact loopback sockets; no registry, push or release effect.
    #[command(name = "production-preview")]
    ProductionPreview(preview::ProductionArgs),
    /// Check an explicit source composition and staging parent; no writes, certification or release.
    #[command(name = "check-stage")]
    CheckStage(stage::Args),
    /// Create and retain a fresh private source stage; no replacement, certification or release.
    Stage(stage::CreateArgs),
    /// Verify generated documentation against independent evidence; no writes, admission, cleanup or release.
    #[command(name = "verify-generated-tree")]
    Generated(generated::Args),
    /// Advance an exact expected local journal generation and hold coordination; no release or deployment.
    #[command(name = "acquire-writer")]
    Writer(writer::Args),
    /// Review and confirm output, then deliver through one held writer; signed approval is optional. Close explicitly.
    ReleaseSession(release_session::Args),
    /// Hold restricted interrupted custody for inspection/reconciliation; no generation, dispatch or clean seal.
    RecoverySession(release_session::Args),
    /// Inspect exact supported journal schema and coordinates; no SQL writes, migration, adoption or release.
    #[command(name = "inspect-journal")]
    Journal(journal::Args),
    /// Initialize an empty private journal directory; no overwrite, migration, adoption or release.
    #[command(name = "initialize-store")]
    Initialize(initialize::Args),
    /// Hold a locally reserved read-only store session until one stdin byte or EOF; no migration or release.
    #[command(name = "connect-store")]
    Connect(connect::Args),
    /// Verify retained backup bytes against an independent manifest pin; no restore, adoption or release.
    #[command(name = "verify-backup")]
    VerifyBackup(verify_backup::Input),
    /// Prepare a detached recovery copy; no replacement, migration, activation or release.
    #[command(name = "prepare-store-restore")]
    PrepareRestore(restore::Args),
    /// Create a fresh private `SQLite` backup; no overwrite, migration, restore or release.
    #[command(name = "backup-store")]
    Backup(backup::Args),
    /// Observe existing `SQLite` metadata; no create/migrate/SQL writes; coordination effects require opt-in.
    #[command(name = "inspect-store")]
    Store(store::Args),
    /// Inspect every file in a TOML tree; no writes, imports, reconciliation or release.
    #[command(name = "inspect-toml")]
    Toml(toml::Args),
    /// Inspect Markdown from stdin or an explicitly pinned tree; no writes, network, admission or release.
    #[command(name = "inspect-markdown")]
    Markdown(markdown::Args),
    /// Read and hash a source tree; no writes, network, admission or release.
    #[command(name = "inspect-source")]
    Source {
        /// Selected directory. Descendant links and special files are rejected.
        root: PathBuf,
        /// Literal relative subdirectory, resolved beneath ROOT without symlinks.
        #[arg(long)]
        subdir: Option<Subdirectory>,
        /// Verify selected bytes against this exact checked-out Git commit.
        #[arg(long, requires = "subdir")]
        commit: Option<CommitId>,
        /// Read a child at this parent-relative gitlink; ROOT/COMMIT select its parent.
        #[arg(long, requires = "commit")]
        submodule: Option<Subdirectory>,
        /// Maximum combined decoded commit/tree bytes during revision inspection.
        #[arg(long, requires = "commit", default_value_t = RevisionLimits::default().metadata_bytes)]
        max_git_metadata_bytes: usize,
        #[command(flatten)]
        limits: SourceLimits,
    },
}

#[derive(clap::Args)]
struct SourceLimits {
    /// Maximum descendant entries, including directories and hidden files.
    #[arg(long, default_value_t = Limits::default().entries)]
    max_entries: usize,
    /// Maximum descendant depth; immediate children have depth one.
    #[arg(long, default_value_t = Limits::default().depth)]
    max_depth: usize,
    /// Maximum bytes in one file.
    #[arg(long, default_value_t = Limits::default().file_bytes)]
    max_file_bytes: u64,
    /// Maximum combined bytes across all files.
    #[arg(long, default_value_t = Limits::default().total_bytes)]
    max_total_bytes: u64,
}

impl SourceLimits {
    fn into_limits(self) -> Limits {
        Limits {
            entries: self.max_entries,
            depth: self.max_depth,
            file_bytes: self.max_file_bytes,
            total_bytes: self.max_total_bytes,
        }
    }
}

/// Explicit carrier byte budgets passed to the outer MCP launcher for validation.
/// The reusable command shell has no dependency on the MCP transport.
#[derive(clap::Args)]
pub struct McpArgs {
    /// Maximum request-frame bytes (4096..16777216); default 1048576 (1 MiB).
    #[arg(long)]
    pub max_frame_bytes: Option<usize>,
    /// Maximum encoded tool-result bytes (1024..16777216); default 262144 (256 KiB).
    #[arg(long)]
    pub max_response_bytes: Option<usize>,
}

/// Result of parsing and running one invocation of the shared `gazette` command shell.
///
/// Protocol launch remains separate from application-command execution so the
/// reusable CLI package does not depend on MCP or LSP transports.
pub enum Invocation {
    /// One application command completed with this process status.
    Application(ExitCode),
    /// The launcher must start the MCP transport.
    Mcp(McpArgs),
    /// The launcher must start the LSP transport.
    Lsp,
}

/// Parse the process arguments and either execute an application command or
/// return the selected protocol transport to the outer executable launcher.
#[must_use]
pub fn run() -> Invocation {
    let mut parser = Cli::command();
    let version: &'static str =
        Box::leak(gazette_sdk::build::runtime_version_coordinate().into_boxed_str());
    parser = parser.version(version);
    let guide = command::metadata::grouped_cli_help(
        parser
            .get_subcommands()
            .map(clap::Command::get_name)
            .filter(|name| !matches!(*name, "mcp" | "lsp" | "help")),
    );
    parser = parser.before_long_help(guide);
    let matches = parser.get_matches();
    let Cli { command } = Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
    match command {
        Command::Lsp => Invocation::Lsp,
        Command::ImportEditorContext { allow_read } => {
            Invocation::Application(import_editor_context(allow_read))
        }
        Command::Mcp(args) => Invocation::Mcp(args),
        Command::Schema(args) => Invocation::Application(schema::run(args)),
        Command::PrepareReleaseCommit(args) => {
            Invocation::Application(release_commit::prepare(args))
        }
        Command::VerifyReleaseCommit(args) => Invocation::Application(release_commit::verify(args)),
        Command::CheckpointReleaseCommit(args) => {
            Invocation::Application(release_commit::record(args))
        }
        Command::InspectReleaseCheckpoint(args) => {
            Invocation::Application(release_commit::inspect(args))
        }
        Command::InspectJournalWorktree(args) => {
            Invocation::Application(release_commit::inspect_journal_worktree(args))
        }
        Command::RecoverJournalWorktree(args) => {
            Invocation::Application(release_commit::recover_journal_worktree(args))
        }
        Command::RetainReleaseCommit(args) => Invocation::Application(release_commit::retain(args)),
        Command::InspectReleaseCustody(args) => {
            Invocation::Application(release_commit::inspect_custody(args))
        }
        Command::VerifyReleaseAuthorization(args) => Invocation::Application(release::run(args)),
        Command::InspectReleasePreparations(args) => {
            Invocation::Application(release_preparations::inspect(args))
        }
        Command::VerifyPreparedAuthorization(args) => {
            Invocation::Application(release_preparations::verify(args))
        }
        Command::ObserveReleaseTargets(args) => {
            Invocation::Application(release_readback::run(args))
        }
        Command::ObserveGithubPagesBuild(args) => Invocation::Application(github::run(args)),
        Command::ObserveDelivery(args) => Invocation::Application(delivery::run(args)),
        Command::RecordReleaseObservation(args) => {
            Invocation::Application(release_observation::record(args))
        }
        Command::InspectReleaseObservation(args) => {
            Invocation::Application(release_observation::inspect(args))
        }
        Command::InspectReleaseAuthorization(args) => Invocation::Application(
            release_history::inspect(args, release_history::Kind::Authorization),
        ),
        Command::InspectReleaseLeases(args) => Invocation::Application(release_history::inspect(
            args,
            release_history::Kind::Leases,
        )),
        Command::InspectReleaseConsumption(args) => Invocation::Application(
            release_history::inspect(args, release_history::Kind::Consumption),
        ),
        Command::InspectReleaseAttempts(args) => Invocation::Application(release_history::inspect(
            args,
            release_history::Kind::Attempts,
        )),
        Command::InspectReleaseReconciliations(args) => Invocation::Application(
            release_history::inspect(args, release_history::Kind::Reconciliations),
        ),
        Command::EnrollLocalCoordinator(args) => Invocation::Application(coordinator::enroll(args)),
        Command::InspectLocalCoordinator(args) => {
            Invocation::Application(coordinator::inspect(args))
        }
        Command::AdoptStoreRestore(args) => Invocation::Application(coordinator::adopt(args)),
        Command::ReconcileRegistry(args) => Invocation::Application(registry::reconcile(args)),
        Command::InspectRegistry(args) => Invocation::Application(registry::inspect(args)),
        Command::InspectMaterialization(args) => {
            Invocation::Application(materialization::inspect(args))
        }
        Command::Documentation(args) => Invocation::Application(documentation::run(args)),
        Command::Check(args) => Invocation::Application(check::run(args)),
        Command::Config(args) => Invocation::Application(config::run(args)),
        Command::RenderSite(args) => Invocation::Application(render_site::run(args)),
        Command::ProbeOutput(args) => Invocation::Application(probe_output::run(args)),
        Command::RenderSites(args) => Invocation::Application(render_sites::run(args)),
        Command::MaterializeSites(args) => Invocation::Application(materialization::execute(args)),
        Command::MaterializeDocs(args) => {
            Invocation::Application(materialization::documentation(args))
        }
        Command::QualifySites(args) => Invocation::Application(qualify_sites::run(args)),
        Command::PrepareSites(args) => Invocation::Application(prepare_sites::run(args)),
        Command::PreviewSites(args) => Invocation::Application(preview::run(args)),
        Command::ProductionPreview(args) => Invocation::Application(preview::run_production(args)),
        Command::CheckStage(args) => Invocation::Application(stage::run(args, false, None)),
        Command::Stage(args) => Invocation::Application(stage::run(
            args.input,
            args.create_stage,
            args.name.as_deref(),
        )),
        Command::Generated(args) => Invocation::Application(generated::run(args)),
        Command::Writer(args) => Invocation::Application(writer::run(args)),
        Command::ReleaseSession(args) => Invocation::Application(release_session::run(args)),
        Command::RecoverySession(args) => Invocation::Application(recovery_session::run(args)),
        Command::Journal(args) => Invocation::Application(journal::run(args)),
        Command::Initialize(args) => Invocation::Application(initialize::run(args)),
        Command::Connect(args) => Invocation::Application(connect::run(args)),
        Command::VerifyBackup(args) => Invocation::Application(verify_backup::run(args)),
        Command::PrepareRestore(args) => Invocation::Application(restore::run(args)),
        Command::Backup(args) => Invocation::Application(backup::run(args)),
        Command::Store(args) => Invocation::Application(store::run(args)),
        Command::Toml(args) => Invocation::Application(toml::run(args)),
        Command::Markdown(args) => Invocation::Application(markdown::run(args)),
        Command::Source {
            root,
            subdir,
            commit,
            submodule,
            max_git_metadata_bytes,
            limits,
        } => {
            let limits = limits.into_limits();
            let selection = match (commit, subdir, submodule) {
                (Some(parent_commit), Some(subdirectory), Some(submodule_path)) => {
                    SourceSelection::Submodule {
                        parent_commit,
                        submodule_path,
                        subdirectory,
                        limits: RevisionLimits {
                            content: limits,
                            metadata_bytes: max_git_metadata_bytes,
                        },
                    }
                }
                (Some(commit), Some(subdirectory), None) => SourceSelection::Revision {
                    subdirectory,
                    commit,
                    limits: RevisionLimits {
                        content: limits,
                        metadata_bytes: max_git_metadata_bytes,
                    },
                },
                (None, subdirectory, None) => SourceSelection::Observation {
                    subdirectory,
                    limits,
                },
                _ => {
                    // Clap enforces this relationship before dispatch. Keep the
                    // boundary explicit if dispatch construction changes later.
                    let _ = writeln!(
                        io::stderr().lock(),
                        "gazette: revision inspection requires --commit and --subdir; no inventory produced"
                    );
                    return Invocation::Application(ExitCode::from(2));
                }
            };
            Invocation::Application(
                match Kernel::new().execute(SourceCommand(SourceRequest {
                    allow_native_reads: true,
                    root,
                    selection,
                })) {
                    Ok(outcome) => write_json(&outcome),
                    Err(error) => {
                        let _ = writeln!(
                            io::stderr().lock(),
                            "gazette: {}: {error}; no inventory produced",
                            error.code()
                        );
                        ExitCode::FAILURE
                    }
                },
            )
        }
    }
}

fn write_json(value: &impl Serialize) -> ExitCode {
    write_json_outcome(value, true)
}

/// A lost receipt cannot erase a completed local write or its recovery location.
fn write_stage_json(value: &impl Serialize, retained: Option<&source::RetainedStage>) -> ExitCode {
    let status = write_json_outcome(value, retained.is_none());
    if status != ExitCode::SUCCESS
        && let Some(retained) = retained
    {
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: local tree retained at {:?}, but receipt delivery failed; inspect before retrying; no cleanup attempted",
            retained.path()
        );
    }
    status
}

fn write_json_outcome(value: &impl Serialize, broken_pipe_is_success: bool) -> ExitCode {
    let mut output = match serde_json::to_vec_pretty(value) {
        Ok(output) => output,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "gazette: output_encoding: {error}");
            return ExitCode::FAILURE;
        }
    };
    output.push(b'\n');
    match deliver_json(&output) {
        Ok(()) => ExitCode::SUCCESS,
        // Only a read-only operation can treat a lost output pipe as success.
        Err(error) if broken_pipe_is_success && error.kind() == io::ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "gazette: output_io: {error}");
            ExitCode::FAILURE
        }
    }
}

fn deliver_json(output: &[u8]) -> io::Result<()> {
    use std::os::fd::AsFd;
    let mut stdout = io::stdout().lock();
    // StdoutRaw deliberately maps EBADF to success. A duplicated native File
    // preserves the actual write error, so a lost effect receipt cannot look
    // delivered. Keep stdout locked and flush earlier buffered output first.
    stdout
        .flush()
        .and_then(|()| stdout.as_fd().try_clone_to_owned())
        .map(std::fs::File::from)
        .and_then(|mut file| file.write_all(output))
}

fn import_editor_context(allow_read: bool) -> ExitCode {
    use command::authoring::{EditorContext, MAX_FRAME_BYTES};

    let result = (|| {
        let mut bytes = Vec::new();
        io::stdin()
            .lock()
            .take(MAX_FRAME_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ("editor_context_input", "could not read private stdin"))?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err((
                "editor_context_input_limit",
                "complete editor context exceeds the stdin frame ceiling",
            ));
        }
        let json_input::Object(context) =
            serde_json::from_slice::<json_input::Object<EditorContext>>(&bytes).map_err(|_| {
                (
                    "editor_context_input",
                    "stdin must contain one complete editor context object",
                )
            })?;
        context
            .inspect_with_native_reads(allow_read)
            .map_err(|error| (error.code(), error.reason()))
    })();
    match result {
        Ok(receipt) => write_json(&receipt),
        Err((code, reason)) => {
            let _ = writeln!(io::stderr().lock(), "gazette: {code}: {reason}");
            ExitCode::FAILURE
        }
    }
}
