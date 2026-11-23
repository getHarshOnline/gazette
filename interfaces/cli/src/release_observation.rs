//! Private managed observation-history transport; SDK owns lifecycle and outcome projection.

use gazette_sdk::{release, source::Sha256Digest, store};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{Kernel, observation};
use crate::json_input::Object;

#[derive(clap::Args)]
pub(super) struct RecordArgs {
    #[command(flatten)]
    observation: super::release_readback::Args,
    /// Permit immutable diagnostic history, not authorization consumption or settlement.
    #[arg(long)]
    record_observation: bool,
    /// Permit one durable local generation advance, including before a later failure.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold exclusive local coordination through native reads, append and independent read-back.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Permit selected coordinator-anchor writes, separately from journal generation/history.
    #[arg(long)]
    write_coordinator_anchor: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordInput<'a> {
    root: PathBuf,
    expected_header: Object<store::JournalHeader>,
    id: Sha256Digest,
    git: release::GitSelection,
    #[serde(borrow)]
    targets: &'a RawValue,
    protected_roots: Vec<PathBuf>,
    coordinator: Option<Object<super::coordinator::SelectionInput>>,
}

#[derive(clap::Args)]
pub(super) struct InspectArgs {
    /// Maximum complete private stdin JSON before decoding.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_input_bytes: usize,
    /// Maximum complete historical report including newline; no truncated prefix.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_output_bytes: usize,
    /// Maximum retained target set, independent from storage and encoded evidence limits.
    #[arg(long, default_value_t = release::Limits::default().targets)]
    max_targets: usize,
    /// Maximum retained target-array JSON nodes during native target-contract validation.
    #[arg(long, default_value_t = release::Limits::default().nodes)]
    max_nodes: usize,
    /// Maximum retained target-array nesting depth.
    #[arg(long, default_value_t = release::Limits::default().depth)]
    max_depth: usize,
    /// Maximum encoded retained target-array bytes, independent from the request/report.
    #[arg(long, default_value_t = release::Limits::default().bytes)]
    max_target_bytes: usize,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectInput {
    database: PathBuf,
    id: Sha256Digest,
    expected_header: Option<Object<store::JournalHeader>>,
}

fn input(maximum: usize) -> Result<Vec<u8>, &'static str> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "release_observation_input")?;
    if bytes.len() > maximum {
        return Err("release_observation_input_limit");
    }
    Ok(bytes)
}

pub(super) fn record(args: RecordArgs) -> ExitCode {
    let bytes = match input(args.observation.max_input_bytes) {
        Ok(bytes) => bytes,
        Err(code) => return failure(code, false),
    };
    let Object(input): Object<RecordInput<'_>> = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => return failure("release_observation_request", false),
    };
    let coordinator = input.coordinator.map(|Object(value)| value.into());
    let outcome = match Kernel::new().execute(observation::RecordObservation {
        root: input.root,
        expected_header: input.expected_header.0,
        id: input.id,
        targets_json: input.targets.get().as_bytes().to_vec(),
        git: input.git,
        protected_roots: input.protected_roots,
        coordinator,
        limits: args.observation.limits(),
        options: release::ObservationSessionOptions {
            allow_anchor_write: args.write_coordinator_anchor,
            writer: store::WriterOptions {
                allow_generation_advance: args.advance_writer_generation,
                session: store::SessionOptions {
                    allow_local_reservation: args.reserve_local_coordination,
                    read: args.read.options(),
                },
            },
            observation: release::RecordObservationOptions {
                read: args.observation.options(),
                record: release::RefObservationWriteOptions {
                    allow_record: args.record_observation,
                },
            },
        },
    }) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error.code(), false),
    };
    deliver(
        &outcome.report(),
        args.observation.max_output_bytes,
        outcome.is_failure(),
        true,
    )
}

pub(super) fn inspect(args: InspectArgs) -> ExitCode {
    let bytes = match input(args.max_input_bytes) {
        Ok(bytes) => bytes,
        Err(code) => return failure(code, false),
    };
    let Object(input): Object<InspectInput> = match serde_json::from_slice(&bytes) {
        Ok(input) => input,
        Err(_) => return failure("release_observation_request", false),
    };
    if !input.database.is_absolute() {
        return failure("release_observation_request", false);
    }
    let result = Kernel::new().execute(observation::InspectObservation {
        allow_native_reads: true,
        path: input.database,
        id: input.id,
        expected_header: input.expected_header.map(|header| header.0),
        read: args.read.options(),
        limits: release::Limits {
            bytes: args.max_target_bytes,
            nodes: args.max_nodes,
            depth: args.max_depth,
            targets: args.max_targets,
        },
    });
    match result {
        Ok(report) => deliver(&report, args.max_output_bytes, false, false),
        Err(error) => failure(error.code(), false),
    }
}

fn deliver(
    value: &impl serde::Serialize,
    maximum: usize,
    failed: bool,
    effectful: bool,
) -> ExitCode {
    let mut output = gazette_sdk::render::LimitedWriter::new(Vec::new(), maximum);
    if serde_json::to_writer(&mut output, value).is_err() || output.write_all(b"\n").is_err() {
        return failure("release_observation_output_limit", effectful);
    }
    if crate::deliver_json(&output.into_inner()).is_err() {
        return failure("release_observation_output_io", effectful);
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn failure(code: &str, effectful: bool) -> ExitCode {
    let detail = if effectful {
        "coordinator anchor writes, generation acquisition, destination reads or append may have occurred; preserve the selected anchor/binding, original journal coordinates and observation ID and inspect before any retry"
    } else {
        "no generation acquisition, destination read or history append attempted"
    };
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete observation-history report delivered; {detail}; no push, consumption or settlement"
    );
    ExitCode::FAILURE
}
