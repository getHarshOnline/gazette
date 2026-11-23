//! Served-output read-back through the shared SDK, without compilation or settlement.

use crate::command::{Kernel, observation};
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Complete private stdin JSON ceiling before decoding or native reads.
    #[arg(long, default_value_t = 1 << 20)]
    max_input_bytes: usize,
    /// Complete report ceiling including newline; never deliver a partial route set.
    #[arg(long, default_value_t = 1 << 20)]
    max_output_bytes: usize,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut input = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut input)
        .is_err()
    {
        return failure("delivery_input");
    }
    let outcome = match Kernel::new().execute(observation::ObserveDelivery {
        request_json: input,
        max_input_bytes: args.max_input_bytes,
    }) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error.code()),
    };
    let matches = outcome.matches_selected_routes();
    let mut output = gazette_sdk::render::LimitedWriter::new(Vec::new(), args.max_output_bytes);
    if serde_json::to_writer(&mut output, outcome.report()).is_err()
        || output.write_all(b"\n").is_err()
    {
        return failure("delivery_output_limit");
    }
    if crate::deliver_json(&output.into_inner()).is_err() {
        return failure("delivery_output_io");
    }
    if matches {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete served-output report delivered; no publication or settlement"
    );
    ExitCode::FAILURE
}
