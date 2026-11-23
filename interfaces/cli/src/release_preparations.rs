//! Private bounded transport for the shared native preparation/signature join.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{
    release, release_commit::JournalWorktreeInspectionOptions, store::JournalHeader,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::{
    command::{Kernel, release_artifact as command},
    json_input::Object,
};

#[derive(clap::Args)]
pub(super) struct InspectArgs {
    /// Maximum complete private stdin envelope bytes, before JSON decoding.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_input_bytes: usize,
    /// Maximum complete ordered native preparation selection.
    #[arg(long, default_value_t = release::PreparationBatchOptions::default().targets)]
    max_targets: usize,
    /// Maximum bytes per retained evidence document and complete batch projection.
    #[arg(long, default_value_t = release::PreparationBatchOptions::default().evidence_bytes)]
    max_evidence_bytes: usize,
    /// Maximum complete serialized private response bytes; no partial response is returned.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_output_bytes: usize,
    #[command(flatten)]
    read: crate::store::ReadArgs,
    #[command(flatten)]
    worktree: crate::release_commit::WorktreeLimitsArgs,
}

impl InspectArgs {
    fn options(self) -> release::PreparationBatchOptions {
        release::PreparationBatchOptions {
            inspection: JournalWorktreeInspectionOptions {
                read: self.read.options(),
                worktree: self.worktree.into_limits(),
            },
            targets: self.max_targets,
            evidence_bytes: self.max_evidence_bytes,
        }
    }
}

#[derive(clap::Args)]
pub(super) struct VerifyArgs {
    #[command(flatten)]
    inspection: InspectArgs,
    /// Maximum bytes per authorization/binding and their canonical projections.
    #[arg(long, default_value_t = release::Limits::default().bytes)]
    max_authorization_bytes: usize,
    /// Maximum retained JSON nodes per authorization or independent binding.
    #[arg(long, default_value_t = release::Limits::default().nodes)]
    max_authorization_nodes: usize,
    /// Maximum JSON nesting per authorization or independent binding.
    #[arg(long, default_value_t = release::Limits::default().depth)]
    max_authorization_depth: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input<'a> {
    database: PathBuf,
    expected_header: Object<JournalHeader>,
    #[serde(borrow)]
    selections: Vec<&'a RawValue>,
    protected_roots: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerificationInput<'a> {
    database: PathBuf,
    expected_header: Object<JournalHeader>,
    #[serde(borrow)]
    selections: Vec<&'a RawValue>,
    protected_roots: Vec<PathBuf>,
    #[serde(borrow)]
    authorization: &'a RawValue,
    #[serde(borrow)]
    binding: &'a RawValue,
}

fn read(maximum: usize) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "release_preparation_input")?;
    if bytes.len() > maximum {
        return Err("release_preparation_input_limit");
    }
    Ok(bytes)
}

fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, &'static str> {
    serde_json::from_slice::<Object<T>>(bytes)
        .map(|Object(value)| value)
        .map_err(|_| "release_preparation_request")
}

pub(super) fn inspect(args: InspectArgs) -> ExitCode {
    let bytes = match read(args.max_input_bytes) {
        Ok(v) => v,
        Err(code) => return failure(code),
    };
    let input: Input<'_> = match decode(&bytes) {
        Ok(v) => v,
        Err(code) => return failure(code),
    };
    let maximum = args.max_output_bytes;
    let selections_json = match serde_json::to_string(&input.selections) {
        Ok(value) => value,
        Err(_) => return failure("release_preparation_request"),
    };
    match Kernel::new().execute(command::InspectPreparations {
        allow_native_reads: true,
        database: input.database,
        expected_header: input.expected_header.0,
        selections_json,
        protected_roots: input.protected_roots,
        options: args.options(),
    }) {
        Ok(report) => output(&report, maximum),
        Err(error) => failure(error.code()),
    }
}

pub(super) fn verify(args: VerifyArgs) -> ExitCode {
    let bytes = match read(args.inspection.max_input_bytes) {
        Ok(v) => v,
        Err(code) => return failure(code),
    };
    let Object(input): Object<VerificationInput<'_>> = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => return failure("release_preparation_request"),
    };
    let limits = release::Limits {
        bytes: args.max_authorization_bytes,
        nodes: args.max_authorization_nodes,
        depth: args.max_authorization_depth,
        targets: args.inspection.max_targets,
    };
    let authorization_json = input.authorization.get().to_owned();
    let binding_json = input.binding.get().to_owned();
    let selections_json = match serde_json::to_string(&input.selections) {
        Ok(value) => value,
        Err(_) => return failure("release_preparation_request"),
    };
    let input = Input {
        database: input.database,
        expected_header: input.expected_header,
        selections: input.selections,
        protected_roots: input.protected_roots,
    };
    let maximum = args.inspection.max_output_bytes;
    match Kernel::new().execute(command::VerifyPreparedAuthorization {
        allow_native_reads: true,
        authorization_json,
        binding_json,
        database: input.database,
        expected_header: input.expected_header.0,
        selections_json,
        protected_roots: input.protected_roots,
        preparation_options: args.inspection.options(),
        authorization_limits: limits,
    }) {
        Ok(report) => output(&report, maximum),
        Err(error) => failure(error.code()),
    }
}

fn output(report: &impl Serialize, maximum: usize) -> ExitCode {
    let mut writer = gazette_sdk::render::LimitedWriter::new(Vec::new(), maximum);
    let encoded = serde_json::to_writer(&mut writer, report);
    if encoded.is_err() || writer.exceeded() || writer.write_all(b"\n").is_err() {
        return failure("release_preparation_output_limit");
    }
    if crate::deliver_json(&writer.into_inner()).is_err() {
        return failure("release_preparation_output_io");
    }
    ExitCode::SUCCESS
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete preparation verification report returned; no signing or release effects"
    );
    ExitCode::FAILURE
}
