//! Thin local checkpoint transport; application owns verification, replay and effects.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{Kernel, release_artifact};
use gazette_sdk::{
    release_commit::{CheckpointOptions, CheckpointWriteOptions},
    source::{CommitId, Sha256Digest},
    store::{JournalHeader, SessionOptions, WriterOptions},
};

#[derive(clap::Args)]
pub(crate) struct CheckpointArgs {
    #[command(flatten)]
    shared: super::SharedArgs,
    /// Existing initialized private journal directory, separate from repository and sources.
    root: PathBuf,
    /// Exact existing commit OID; never a moving reference.
    commit: CommitId,
    /// Explicit immutable record coordinate; changed intent requires a new coordinate.
    #[arg(long)]
    checkpoint_id: Sha256Digest,
    /// Complete independently retained current journal coordinates.
    #[arg(long, value_parser = crate::journal::parse_header)]
    expected_header: JournalHeader,
    /// Permit recording immutable local verification evidence, not release preparation.
    #[arg(long)]
    record_checkpoint: bool,
    /// Permit a separate durable generation increment, including before a later failure.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold exclusive local coordination through record read-back.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Configured source/output root that must remain separate from the journal.
    #[arg(long)]
    protected_root: Vec<PathBuf>,
    #[command(flatten)]
    read: crate::store::ReadArgs,
    /// Complete compact response including LF; reserved before journal writes, at most 16 MiB.
    #[arg(long, default_value_t = 256 << 10)]
    max_output_bytes: usize,
}

#[derive(clap::Args)]
pub(crate) struct InspectCheckpointArgs {
    /// Existing private database; no initialization or schema upgrade.
    path: PathBuf,
    /// Exact immutable checkpoint coordinate.
    checkpoint_id: Sha256Digest,
    /// Independently retained coordinates; omission means observation only.
    #[arg(long, value_parser = crate::journal::parse_header)]
    expected_header: Option<JournalHeader>,
    #[command(flatten)]
    read: crate::store::ReadArgs,
}

pub(crate) fn record(args: CheckpointArgs) -> ExitCode {
    let record = CheckpointWriteOptions {
        allow_record: args.record_checkpoint,
    };
    let writer = WriterOptions {
        allow_generation_advance: args.advance_writer_generation,
        session: SessionOptions {
            allow_local_reservation: args.reserve_local_coordination,
            read: args.read.options(),
        },
    };
    let response_bytes = match response_bytes(args.max_output_bytes) {
        Ok(maximum) => maximum,
        Err(code) => return failure(code, &code),
    };
    let metadata = match super::metadata(args.shared.max_input_bytes) {
        Ok(metadata) => metadata,
        Err(reason) => return failure("commit_metadata_input", &reason),
    };
    let repository = args.shared.repository.clone();
    let output = args.shared.expected_tree;
    let max_metadata_bytes = args.shared.max_input_bytes;
    let objects = args.shared.limits();
    let result = Kernel::new().execute(release_artifact::CheckpointCommit {
        allow_native_reads: true,
        root: args.root,
        expected_header: args.expected_header,
        checkpoint_id: args.checkpoint_id,
        repository,
        commit: args.commit,
        expected_tree: output,
        metadata_json: metadata,
        max_metadata_bytes,
        protected_roots: args.protected_root,
        options: CheckpointOptions {
            objects,
            record,
            writer,
        },
        response_bytes,
    });
    let outcome = match result {
        Ok(outcome) => outcome,
        Err(error) => return failure(error.code(), &error),
    };
    let native_failure = outcome
        .native_failure()
        .map(|(code, message)| (code.to_owned(), message.to_owned()));
    let delivered = deliver(outcome);
    if let Some((code, message)) = native_failure {
        return failure(&code, &message);
    }
    delivered
}

pub(super) fn response_bytes(maximum: usize) -> Result<usize, &'static str> {
    maximum
        .checked_sub(1)
        .filter(|_| maximum <= 16 << 20)
        .ok_or("journal_response_limit")
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
            "gazette: journal receipt delivery failed; preserve exact evidence above if available and inspect native history. No automatic retry, recreation, authorization or release follows from lost output."
        );
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub(crate) fn inspect(args: InspectCheckpointArgs) -> ExitCode {
    match Kernel::new().execute(release_artifact::InspectCheckpoint {
        allow_native_reads: true,
        path: args.path,
        checkpoint_id: args.checkpoint_id,
        expected_header: args.expected_header,
        read: args.read.options(),
    }) {
        Ok(report) => crate::write_json(&report),
        Err(error) => failure(error.code(), &error),
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no successful checkpoint report returned"
    );
    ExitCode::FAILURE
}
