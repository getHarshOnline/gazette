//! Private metadata transport for local object preparation, not full prepare-release.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{
    release_commit::CommitOptions,
    source::{CommitId, RevisionLimits, TreeIdentity},
};

use crate::command::{Kernel, release_artifact};
use crate::json_input::Object;

mod checkpoint;
mod custody;
mod journal_worktree;
pub(super) use checkpoint::{CheckpointArgs, InspectCheckpointArgs, inspect, record};
pub(super) use custody::{InspectCustodyArgs, RetainArgs, inspect as inspect_custody, retain};
pub(crate) use journal_worktree::WorktreeLimitsArgs;
pub(super) use journal_worktree::{
    InspectJournalWorktreeArgs, RecoverJournalWorktreeArgs, inspect as inspect_journal_worktree,
    recover as recover_journal_worktree,
};

#[derive(clap::Args)]
struct SharedArgs {
    /// Exact existing bare local repository; never discovered, initialized or fetched.
    repository: PathBuf,
    /// Complete independently retained inventory identity JSON (format and sha256).
    #[arg(long, value_parser = parse_tree)]
    expected_tree: TreeIdentity,
    /// Maximum complete private metadata JSON bytes, before decoding or native access.
    #[arg(long, default_value_t = 16 * 1024)]
    max_input_bytes: usize,
    /// Maximum decoded Git commit/tree metadata bytes.
    #[arg(long, default_value_t = RevisionLimits::default().metadata_bytes)]
    max_git_metadata_bytes: usize,
    #[command(flatten)]
    limits: super::SourceLimits,
}

impl SharedArgs {
    fn limits(self) -> RevisionLimits {
        RevisionLimits {
            content: self.limits.into_limits(),
            metadata_bytes: self.max_git_metadata_bytes,
        }
    }
}

#[derive(clap::Args)]
pub(super) struct PrepareArgs {
    #[command(flatten)]
    shared: SharedArgs,
    /// Existing complete retained output tree; captured and matched before object writes.
    output_root: PathBuf,
    /// Permit local object writes only; no refs, worktree changes or durable release receipt.
    #[arg(long)]
    allow_write_objects: bool,
    /// Additional native source root that must remain separate from the repository.
    #[arg(long)]
    protected_root: Vec<PathBuf>,
    /// Complete compact JSON response including LF; reserved before object writes, at most 16 MiB.
    #[arg(long, default_value_t = 256 << 10)]
    max_output_bytes: usize,
}

#[derive(clap::Args)]
pub(super) struct VerifyArgs {
    #[command(flatten)]
    shared: SharedArgs,
    /// Exact full commit OID, never a branch name or moving reference.
    commit: CommitId,
}

fn parse_tree(value: &str) -> Result<TreeIdentity, String> {
    if value.len() > 256 {
        return Err("expected tree identity exceeds 256 bytes".into());
    }
    serde_json::from_str::<Object<TreeIdentity>>(value)
        .map(|Object(identity)| identity)
        .map_err(|_| "malformed expected tree identity".into())
}

fn metadata(maximum: usize) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "metadata input could not be read; no native operation attempted")?;
    if bytes.len() > maximum {
        return Err("commit metadata input exceeds configured limit");
    }
    Ok(bytes)
}

pub(super) fn prepare(args: PrepareArgs) -> ExitCode {
    let response_bytes = match response_bytes(args.max_output_bytes) {
        Ok(maximum) => maximum,
        Err(code) => return failure(&code),
    };
    let metadata = match metadata(args.shared.max_input_bytes) {
        Ok(metadata) => metadata,
        Err(reason) => return failure(&reason),
    };
    let repository = args.shared.repository.clone();
    let expected_tree = args.shared.expected_tree;
    let max_metadata_bytes = args.shared.max_input_bytes;
    let limits = args.shared.limits();
    let result = Kernel::new().execute(release_artifact::PrepareCommit {
        allow_native_reads: true,
        repository,
        output_root: args.output_root,
        expected_tree,
        metadata_json: metadata,
        max_metadata_bytes,
        protected_roots: args.protected_root,
        options: CommitOptions {
            allow_write_objects: args.allow_write_objects,
            limits,
        },
        response_bytes,
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => return failure(&error),
    };
    let native_failure = outcome
        .native_failure()
        .map(|(_, message)| message.to_owned());
    let delivered = deliver(outcome);
    if let Some(message) = native_failure {
        return failure(&message);
    }
    delivered
}

pub(super) fn response_bytes(maximum: usize) -> Result<usize, &'static str> {
    maximum
        .checked_sub(1)
        .filter(|_| maximum <= 16 << 20)
        .ok_or("release_commit_response_limit")
}

pub(super) fn deliver(outcome: release_artifact::EncodedOutcome) -> ExitCode {
    let failed = outcome.is_failure();
    let mut bytes = outcome.into_bytes();
    bytes.push(b'\n');
    if crate::deliver_json(&bytes).is_err() {
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(&bytes);
        let _ = writeln!(
            stderr,
            "gazette: local Git report delivery failed; retain exact evidence above if this carrier is available. Native effects may remain; no automatic retry, cleanup, journal or remote effect."
        );
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub(super) fn verify(args: VerifyArgs) -> ExitCode {
    let metadata = match metadata(args.shared.max_input_bytes) {
        Ok(metadata) => metadata,
        Err(reason) => return failure(&reason),
    };
    let repository = args.shared.repository.clone();
    let expected_tree = args.shared.expected_tree;
    let max_metadata_bytes = args.shared.max_input_bytes;
    let limits = args.shared.limits();
    match Kernel::new().execute(release_artifact::VerifyCommit {
        allow_native_reads: true,
        repository,
        commit: args.commit,
        expected_tree,
        metadata_json: metadata,
        max_metadata_bytes,
        limits,
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => failure(&error),
    }
}

fn failure(error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: release_commit: {error}; no successful commit report returned; no refs, journal, signing or network effects"
    );
    ExitCode::FAILURE
}
