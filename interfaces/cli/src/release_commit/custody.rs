//! Existing-object custody transport, separate from checkpoint and release approval.

use crate::command::{Kernel, release_artifact};
use gazette_sdk::{
    release_commit,
    source::{CommitId, Sha256Digest},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(crate) struct InspectCustodyArgs {
    #[command(flatten)]
    shared: super::SharedArgs,
    /// Exact existing commit, not a moving ref.
    commit: CommitId,
    /// Exact immutable custody coordinate.
    #[arg(long)]
    custody_id: Sha256Digest,
}

#[derive(clap::Args)]
pub(crate) struct RetainArgs {
    #[command(flatten)]
    selected: InspectCustodyArgs,
    /// Permit exact local custody ref creation and native storage synchronization.
    #[arg(long)]
    allow_retain: bool,
    /// Maximum aggregate repository bytes for complete synchronization.
    #[arg(long, default_value_t = release_commit::CustodyOptions::default().storage_bytes)]
    max_storage_bytes: u64,
    /// Configured source/output root that must remain separate from custody storage.
    #[arg(long)]
    protected_root: Vec<PathBuf>,
    /// Complete compact JSON response including LF; reserved before retention, at most 16 MiB.
    #[arg(long, default_value_t = 256 << 10)]
    max_output_bytes: usize,
}

pub(crate) fn retain(args: RetainArgs) -> ExitCode {
    let response_bytes = match super::response_bytes(args.max_output_bytes) {
        Ok(maximum) => maximum,
        Err(code) => return failure(&code),
    };
    let selected = args.selected;
    let metadata = match super::metadata(selected.shared.max_input_bytes) {
        Ok(metadata) => metadata,
        Err(reason) => return failure(&reason),
    };
    let repository = selected.shared.repository.clone();
    let output = selected.shared.expected_tree;
    let max_metadata_bytes = selected.shared.max_input_bytes;
    let limits = selected.shared.limits();
    let result = Kernel::new().execute(release_artifact::RetainCommit {
        allow_native_reads: true,
        repository,
        custody_id: selected.custody_id,
        commit: selected.commit,
        expected_tree: output,
        metadata_json: metadata,
        max_metadata_bytes,
        protected_roots: args.protected_root,
        options: release_commit::CustodyOptions {
            allow_retain: args.allow_retain,
            limits,
            storage_bytes: args.max_storage_bytes,
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
    let delivered = super::deliver(outcome);
    if let Some(message) = native_failure {
        return failure(&message);
    }
    delivered
}

pub(crate) fn inspect(args: InspectCustodyArgs) -> ExitCode {
    let metadata = match super::metadata(args.shared.max_input_bytes) {
        Ok(metadata) => metadata,
        Err(reason) => return failure(&reason),
    };
    let repository = args.shared.repository.clone();
    let output = args.shared.expected_tree;
    let max_metadata_bytes = args.shared.max_input_bytes;
    let limits = args.shared.limits();
    match Kernel::new().execute(release_artifact::InspectCustody {
        allow_native_reads: true,
        repository,
        custody_id: args.custody_id,
        commit: args.commit,
        expected_tree: output,
        metadata_json: metadata,
        max_metadata_bytes,
        limits,
    }) {
        Ok(report) => crate::write_json(&report),
        Err(error) => failure(&error),
    }
}

fn failure(error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: release_custody: {error}; no successful custody report returned"
    );
    ExitCode::FAILURE
}
