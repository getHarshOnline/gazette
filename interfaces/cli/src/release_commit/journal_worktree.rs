//! Recorded-worktree inspection/recovery transport; the journal owns the recipe.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{Kernel, release_artifact};
use gazette_sdk::{
    release_commit::{
        HistoryLimits, JournalWorktreeInspectionOptions, RecoverWorktreeOptions, WorktreeLimits,
        WorktreeSyncOptions,
    },
    source::{CommitId, RevisionLimits, Sha256Digest},
    store::{JournalHeader, PreparationWriteOptions, SessionOptions, WriterOptions},
};

#[derive(clap::Args)]
pub(crate) struct InspectJournalWorktreeArgs {
    /// Existing private journal database; never initialized, migrated or repaired.
    path: PathBuf,
    /// Exact durable preparation coordinate, not a recipe supplied through stdin.
    preparation_id: Sha256Digest,
    /// Exact current commit OID to verify; never inferred from historical completion.
    commit: CommitId,
    /// Independently retained journal coordinates; omission means unpinned observation.
    #[arg(long, value_parser = crate::journal::parse_header)]
    expected_header: Option<JournalHeader>,
    /// Existing source root that must remain separate from journal and worktree.
    #[arg(long)]
    protected_root: Vec<PathBuf>,
    #[command(flatten)]
    read: crate::store::ReadArgs,
    #[command(flatten)]
    worktree: WorktreeLimitsArgs,
}

#[derive(clap::Args)]
pub(crate) struct WorktreeLimitsArgs {
    #[command(flatten)]
    output: crate::SourceLimits,
    /// Maximum decoded Git administrative and traversal metadata bytes.
    #[arg(long, default_value_t = RevisionLimits::default().metadata_bytes)]
    max_git_metadata_bytes: usize,
    /// Maximum unique reachable parent-history commits, trees and blobs.
    #[arg(long, default_value_t = HistoryLimits::default().objects)]
    max_history_objects: usize,
    /// Maximum complete decoded parent-history bytes.
    #[arg(long, default_value_t = HistoryLimits::default().bytes)]
    max_history_bytes: u64,
    /// Maximum decoded size of one historical object before reading its payload.
    #[arg(long, default_value_t = HistoryLimits::default().object_bytes)]
    max_history_object_bytes: usize,
}

impl WorktreeLimitsArgs {
    pub(crate) fn into_limits(self) -> WorktreeLimits {
        WorktreeLimits {
            output: RevisionLimits {
                content: self.output.into_limits(),
                metadata_bytes: self.max_git_metadata_bytes,
            },
            history: HistoryLimits {
                objects: self.max_history_objects,
                bytes: self.max_history_bytes,
                object_bytes: self.max_history_object_bytes,
            },
        }
    }
}

#[derive(clap::Args)]
pub(crate) struct RecoverJournalWorktreeArgs {
    /// Existing private journal database; no initialization, migration or repair.
    database: PathBuf,
    /// Exact existing preparation coordinate; its stored recipe is not replaceable.
    preparation_id: Sha256Digest,
    /// Independently selected existing commit; never inferred or regenerated.
    commit: CommitId,
    /// Complete independently retained current journal coordinates, required for recovery.
    #[arg(long, value_parser = crate::journal::parse_header)]
    expected_header: JournalHeader,
    /// Permit flushing existing output and Git storage; no recreation or repair.
    #[arg(long)]
    synchronize_worktree: bool,
    /// Permit recording or replaying this exact preparation's verified completion.
    #[arg(long)]
    record_preparation: bool,
    /// Permit a durable generation increment, including before a later failure.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold exclusive local coordination through recovery and record read-back.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Existing source root that must remain separate from journal and worktree.
    #[arg(long)]
    protected_root: Vec<PathBuf>,
    /// Maximum existing Git administrative-storage bytes during synchronization.
    #[arg(long, default_value_t = WorktreeSyncOptions::default().storage_bytes)]
    max_git_storage_bytes: u64,
    /// Complete compact response including LF; reserved before recovery, at most 16 MiB.
    #[arg(long, default_value_t = 256 << 10)]
    max_output_bytes: usize,
    #[command(flatten)]
    read: crate::store::ReadArgs,
    #[command(flatten)]
    worktree: WorktreeLimitsArgs,
}

pub(crate) fn inspect(args: InspectJournalWorktreeArgs) -> ExitCode {
    let options = JournalWorktreeInspectionOptions {
        read: args.read.options(),
        worktree: args.worktree.into_limits(),
    };
    match Kernel::new().execute(release_artifact::InspectJournalWorktree {
        allow_native_reads: true,
        database: args.path,
        preparation_id: args.preparation_id,
        expected_header: args.expected_header,
        commit: args.commit,
        protected_roots: args.protected_root,
        options,
    }) {
        Ok(report) => crate::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no joined worktree inspection report produced",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}

pub(crate) fn recover(args: RecoverJournalWorktreeArgs) -> ExitCode {
    let response_bytes = match super::checkpoint::response_bytes(args.max_output_bytes) {
        Ok(maximum) => maximum,
        Err(code) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {code}; no native recovery attempted"
            );
            return ExitCode::FAILURE;
        }
    };
    let options = RecoverWorktreeOptions {
        record: PreparationWriteOptions {
            allow_record: args.record_preparation,
        },
        writer: WriterOptions {
            allow_generation_advance: args.advance_writer_generation,
            session: SessionOptions {
                allow_local_reservation: args.reserve_local_coordination,
                read: args.read.options(),
            },
        },
        worktree: WorktreeSyncOptions {
            allow_synchronize: args.synchronize_worktree,
            limits: args.worktree.into_limits(),
            storage_bytes: args.max_git_storage_bytes,
        },
    };
    let result = Kernel::new().execute(release_artifact::RecoverJournalWorktree {
        allow_native_reads: true,
        database: args.database,
        preparation_id: args.preparation_id,
        expected_header: args.expected_header,
        commit: args.commit,
        protected_roots: args.protected_root,
        options,
        response_bytes,
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no successful recorded-worktree recovery report returned",
                error.code()
            );
            return ExitCode::FAILURE;
        }
    };
    let native_failure = outcome
        .native_failure()
        .map(|(code, message)| (code.to_owned(), message.to_owned()));
    let delivered = super::checkpoint::deliver(outcome);
    if let Some((code, message)) = native_failure {
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: {code}: {message}; no successful recorded-worktree recovery report returned"
        );
        return ExitCode::FAILURE;
    }
    delivered
}
