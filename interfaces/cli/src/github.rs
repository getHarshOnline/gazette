//! Public provider observation through the SDK; no deployment or settlement.

use crate::command::{Kernel, observation};
use gazette_sdk::github;
use serde::Deserialize;
use std::{
    io::{self, Read, Write},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Permit native executable, library and certificate reads.
    #[arg(long)]
    allow_read: bool,
    /// Permit the explicitly pinned trusted transport executable.
    #[arg(long)]
    allow_execute: bool,
    /// Permit the exact public GitHub API GET; no implicit credentials.
    #[arg(long)]
    allow_network: bool,
    /// Complete provider response ceiling; no truncated observation.
    #[arg(long, default_value_t = github::Options::default().response_bytes)]
    response_bytes: usize,
    /// Complete native capture deadline in milliseconds.
    #[arg(long, default_value_t = github::Options::default().timeout_ms)]
    timeout_ms: u64,
    /// Complete private stdin JSON ceiling before decoding.
    #[arg(long, default_value_t = 1 << 20)]
    max_input_bytes: usize,
    /// Complete result ceiling including newline; no prefix is delivered.
    #[arg(long, default_value_t = 1 << 20)]
    max_output_bytes: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    curl: github::Curl,
    selection: github::PagesBuild,
}

pub(super) fn run(args: Args) -> ExitCode {
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_input_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure("github_input");
    }
    if bytes.len() > args.max_input_bytes {
        return failure("github_input_limit");
    }
    let value = match gazette_sdk::json::parse(&bytes) {
        Ok(value)
            if value.is_object()
                && value["curl"].is_object()
                && value["curl"]["executable"].is_object()
                && value["selection"].is_object() =>
        {
            value
        }
        _ => return failure("github_request"),
    };
    let input: Input = match serde_json::from_value(value) {
        Ok(input) => input,
        Err(_) => return failure("github_request"),
    };
    let options = github::Options {
        allow_read: args.allow_read,
        allow_execute: args.allow_execute,
        allow_network: args.allow_network,
        response_bytes: args.response_bytes,
        timeout_ms: args.timeout_ms,
    };
    let report = match Kernel::new().execute(observation::ObserveGithubPages {
        curl: input.curl,
        selection: input.selection,
        options,
    }) {
        Ok(report) => report,
        Err(error) => return failure(error.code()),
    };
    let mut writer = gazette_sdk::render::LimitedWriter::new(Vec::new(), args.max_output_bytes);
    if serde_json::to_writer(&mut writer, &report).is_err() || writer.write_all(b"\n").is_err() {
        return failure("github_output_limit");
    }
    if crate::deliver_json(&writer.into_inner()).is_err() {
        return failure("github_output_io");
    }
    ExitCode::SUCCESS
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete Pages observation delivered; no deployment or settlement"
    );
    ExitCode::FAILURE
}
