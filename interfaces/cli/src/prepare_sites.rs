//! Direct local preparation over the shared SDK recipe; no compiler or journal logic here.

use crate::command::{Kernel, publication};
use gazette_sdk::{build, release_commit, store};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Local preparation only: not certification, authorization consumption or publication.
    #[arg(long, required = true)]
    local: bool,
    /// Permit explicitly configured pinned local source reads.
    #[arg(long)]
    allow_read: bool,
    /// Permit exact fresh named stage creation and retention; no existing-child adoption.
    #[arg(long)]
    create_stage: bool,
    /// Permit immutable preparation phase records in the existing journal.
    #[arg(long)]
    record_preparation: bool,
    /// Permit local writer generation advancement, including possible interrupted attempts.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Permit held local namespace reservations during native phases.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Permit writes to the explicitly selected coordinator anchor; never enroll or repair it.
    #[arg(long)]
    write_coordinator_anchor: bool,
    /// Permit independent local Git metadata/objects and detached commit creation.
    #[arg(long)]
    initialize_worktree: bool,
    /// Permit the exact configured policy executable; it is not sandboxed by Gazette.
    #[arg(long)]
    allow_policy_execution: bool,
    /// Complete private request ceiling before decoding or native source reads.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_input_bytes: usize,
    /// Complete private result ceiling; failure after effects requires native reconciliation.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_output_bytes: usize,
    #[command(flatten)]
    read: crate::store::ReadArgs,
    #[command(flatten)]
    output: crate::SourceLimits,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut options = build::LocalPreparationRunOptions {
        allow_read: args.allow_read,
        output: args.output.into_limits(),
        preparation: build::LocalPreparationOptions {
            allow_anchor_write: args.write_coordinator_anchor,
            stage: build::PreparationOptions {
                allow_stage: args.create_stage,
                record: store::PreparationWriteOptions {
                    allow_record: args.record_preparation,
                },
                writer: store::WriterOptions {
                    allow_generation_advance: args.advance_writer_generation,
                    session: store::SessionOptions {
                        allow_local_reservation: args.reserve_local_coordination,
                        read: args.read.options(),
                    },
                },
                ..Default::default()
            },
            policy: gazette_sdk::process::Options {
                allow_execute: args.allow_policy_execution,
                ..Default::default()
            },
            worktree: release_commit::WorktreeOptions {
                allow_initialize: args.initialize_worktree,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    options.preparation.stage.output = options.output;
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return fail("cannot read complete private request; no preparation started");
    }
    let result = match Kernel::new().execute(publication::Prepare {
        recipe_json: bytes,
        max_input_bytes: args.max_input_bytes,
        max_output_bytes: args.max_output_bytes,
        local: args.local,
        options,
    }) {
        Ok(result) => result,
        Err(error) => return fail(error.message()),
    };
    let delivered = {
        let mut output = io::stdout().lock();
        output
            .write_all(result.bytes())
            .and_then(|()| output.write_all(b"\n"))
            .and_then(|()| output.flush())
            .is_ok()
    };
    if !delivered {
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: local preparation response unavailable; the pre-reserved native reconciliation receipt could not be delivered; effects may have occurred; preserve the original request and inspect native history before further work; do not retry, recreate or infer rollback"
        );
        return ExitCode::FAILURE;
    }
    if !result.is_failure() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn fail(message: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: local_preparation_input: {message}"
    );
    ExitCode::FAILURE
}
