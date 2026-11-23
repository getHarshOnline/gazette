//! Private exact target selection and explicit native ref-observation effects.

use std::{
    io::{self, Read, Write},
    process::ExitCode,
    time::Duration,
};

use gazette_sdk::release;
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::command::{Kernel, observation};
use crate::json_input::Object;

#[derive(clap::Args)]
pub(super) struct Args {
    /// Permit the selected Git executable and destination reads; never a push.
    #[arg(long)]
    allow_read: bool,
    /// Additionally permit the supported HTTPS observation profile; no implicit credentials.
    #[arg(long)]
    allow_network: bool,
    /// Complete target-set deadline; each native read receives only the remaining budget.
    #[arg(long, default_value_t = 30_000)]
    timeout_ms: u64,
    /// Maximum complete native stdout bytes per target, never a truncation allowance.
    #[arg(long, default_value_t = 64 * 1024)]
    max_response_bytes: usize,
    /// Maximum complete private stdin JSON bytes before decoding.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    pub(super) max_input_bytes: usize,
    /// Maximum complete report including its final newline; no prefix is delivered.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    pub(super) max_output_bytes: usize,
    /// Maximum complete ordered target set; each target receives an explicit result.
    #[arg(long, default_value_t = release::Limits::default().targets)]
    max_targets: usize,
    /// Maximum decoded target-array JSON nodes.
    #[arg(long, default_value_t = release::Limits::default().nodes)]
    max_nodes: usize,
    /// Maximum decoded target-array nesting depth.
    #[arg(long, default_value_t = release::Limits::default().depth)]
    max_depth: usize,
}

impl Args {
    pub(super) fn limits(&self) -> release::Limits {
        release::Limits {
            bytes: self.max_input_bytes,
            nodes: self.max_nodes,
            depth: self.max_depth,
            targets: self.max_targets,
        }
    }

    pub(super) fn options(&self) -> release::RemoteReadOptions {
        release::RemoteReadOptions {
            allow_read: self.allow_read,
            allow_network: self.allow_network,
            timeout: Duration::from_millis(self.timeout_ms),
            max_response_bytes: self.max_response_bytes,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input<'a> {
    git: release::GitSelection,
    #[serde(borrow)]
    targets: &'a RawValue,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure("release_read_input");
    }
    if bytes.len() > args.max_input_bytes {
        return failure("release_read_input_limit");
    }
    let Object(input): Object<Input<'_>> = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => return failure("release_read_request"),
    };
    let report = match Kernel::new().execute(observation::ObserveTargets {
        targets_json: input.targets.get().as_bytes().to_vec(),
        git: input.git,
        limits: args.limits(),
        options: args.options(),
    }) {
        Ok(value) => value,
        Err(error) => return failure(error.code()),
    };
    let mut writer = gazette_sdk::render::LimitedWriter::new(Vec::new(), args.max_output_bytes);
    if serde_json::to_writer(&mut writer, &report).is_err()
        || writer.exceeded()
        || writer.write_all(b"\n").is_err()
    {
        return failure("release_read_output_limit");
    }
    if crate::deliver_json(&writer.into_inner()).is_err() {
        return failure("release_read_output_io");
    }
    ExitCode::SUCCESS
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete ref-observation report delivered; no push or consumption"
    );
    ExitCode::FAILURE
}
