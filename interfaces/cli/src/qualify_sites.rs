//! Standalone complete-set qualification over the shared SDK; no release preparation.

use crate::command::{Kernel, publication};
use gazette_sdk::render::LimitedWriter;
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Permit the exact configured local source reads.
    #[arg(long)]
    allow_read: bool,
    /// Permit the configured candidate checker; its process effects are not sandboxed.
    #[arg(long)]
    allow_policy_execution: bool,
    /// Complete private stdin recipe ceiling before decoding or source acquisition.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_input_bytes: usize,
    /// Complete report ceiling, including newline; no partial set is delivered.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_output_bytes: usize,
    /// Per-site output bounds; independent native aggregate limits remain enforced.
    #[command(flatten)]
    output: crate::SourceLimits,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure(
            "qualification_input",
            "cannot read complete recipe; no qualification started",
        );
    }
    let report = match Kernel::new().execute(publication::Qualify {
        recipe_json: bytes,
        max_input_bytes: args.max_input_bytes,
        allow_read: args.allow_read,
        allow_policy_execution: args.allow_policy_execution,
        output: args.output.into_limits(),
    }) {
        Ok(report) => report,
        Err(error) => return failure(error.code(), error.message()),
    };
    let complete = report.is_complete();
    let mut output = LimitedWriter::new(Vec::new(), args.max_output_bytes);
    if serde_json::to_writer(&mut output, &report).is_err()
        || output.write_all(b"\n").is_err()
        || crate::deliver_json(&output.into_inner()).is_err()
    {
        return failure(
            "qualification_response_unavailable",
            "no complete qualification report delivered; configured checkers or generators may have run; preserve the original recipe and inspect native outcomes before further work; do not infer rollback or retry; no release preparation performed",
        );
    }
    if complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn failure(code: &str, message: &str) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: {code}: {message}");
    ExitCode::FAILURE
}
