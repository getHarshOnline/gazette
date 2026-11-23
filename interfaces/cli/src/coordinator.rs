//! Private trusted-local anchor transport; the SDK owns enrollment and inspection.

use gazette_sdk::{
    source::Sha256Digest,
    store::{self, coordinator},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::json_input::Object;

#[derive(clap::Args)]
pub(super) struct EnrollArgs {
    /// Permit fresh anchor creation, not generation advancement or release authority.
    #[arg(long)]
    write_coordinator_anchor: bool,
    #[command(flatten)]
    session: Args,
}

#[derive(clap::Args)]
pub(super) struct AdoptArgs {
    /// Permit fresh successor creation; never overwrite any selected root.
    #[arg(long)]
    adopt_restore: bool,
    /// Permit replay-safe coordinator anchor transitions.
    #[arg(long)]
    write_coordinator_anchor: bool,
    /// Maximum canonical restore receipt bytes before decoding.
    #[arg(long, default_value_t = 16 * 1024)]
    max_receipt_bytes: u64,
    #[command(flatten)]
    session: Args,
}

#[derive(clap::Args)]
pub(super) struct Args {
    /// Permit the native held local reservation; not a cross-host lease.
    #[arg(long)]
    reserve_local_coordination: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
    /// Maximum complete private stdin JSON before decoding.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_input_bytes: usize,
    /// Maximum complete response including newline; enrollment fallback must fit before access.
    #[arg(long, default_value_t = 4 * 1024 * 1024)]
    max_output_bytes: usize,
}

impl Args {
    fn options(self) -> store::SessionOptions {
        store::SessionOptions {
            allow_local_reservation: self.reserve_local_coordination,
            read: self.read.options(),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentInput {
    anchor_root: PathBuf,
    journal_root: PathBuf,
    coordinator_id: String,
    principal_scope: Sha256Digest,
    protected_roots: Vec<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptionInput {
    anchor_root: PathBuf,
    binding: Object<coordinator::LocalCoordinatorBinding>,
    previous: Object<coordinator::LocalCoordinatorSeal>,
    restore_root: PathBuf,
    receipt_sha256: Sha256Digest,
    successor_root: PathBuf,
    protected_roots: Vec<PathBuf>,
}

/// Object-only local selection, shared with the managed observation transport.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectionInput {
    pub(super) anchor_root: PathBuf,
    pub(super) binding: Object<coordinator::LocalCoordinatorBinding>,
}

impl From<SelectionInput> for coordinator::LocalCoordinatorSelection {
    fn from(value: SelectionInput) -> Self {
        Self {
            anchor_root: value.anchor_root,
            binding: value.binding.0,
        }
    }
}

#[derive(Serialize)]
struct Delivery<'a> {
    code: &'static str,
    input: &'a EnrollmentInput,
    message: &'static str,
}

fn input<T: serde::de::DeserializeOwned>(maximum: usize) -> Result<T, &'static str> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((maximum as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| "coordinator_input_io")?;
    if bytes.len() > maximum {
        return Err("coordinator_input_limit");
    }
    serde_json::from_slice::<Object<T>>(&bytes)
        .map(|value| value.0)
        .map_err(|_| "coordinator_request")
}

fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, &'static str> {
    let mut bytes = gazette_sdk::render::LimitedWriter::new(Vec::new(), maximum);
    serde_json::to_writer(&mut bytes, value).map_err(|_| "coordinator_output_limit")?;
    bytes
        .write_all(b"\n")
        .map_err(|_| "coordinator_output_limit")?;
    Ok(bytes.into_inner())
}

pub(super) fn enroll(args: EnrollArgs) -> ExitCode {
    let input: EnrollmentInput = match input(args.session.max_input_bytes) {
        Ok(value) => value,
        Err(code) => return failure(code),
    };
    if !input.anchor_root.is_absolute()
        || !input.journal_root.is_absolute()
        || input.protected_roots.iter().any(|path| !path.is_absolute())
    {
        return failure("coordinator_request");
    }
    let maximum = args.session.max_output_bytes;
    // This exact private input survives response loss; it is not an invented native seal.
    let fallback = match encode(
        &Delivery {
            code: "coordinator_response_unavailable",
            input: &input,
            message: "Anchor/coordination effects may remain; preserve exact input and residue. No seal delivered, rollback inferred or automatic retry permitted.",
        },
        maximum,
    ) {
        Ok(bytes) => bytes,
        Err(_) => return failure("coordinator_response_budget"),
    };
    let result = crate::command::Kernel::new().execute(crate::command::coordinator::Enroll {
        anchor_root: input.anchor_root,
        journal_root: input.journal_root,
        coordinator_id: input.coordinator_id,
        principal_scope: input.principal_scope,
        protected_roots: input.protected_roots,
        options: args.session.options(),
        allow_anchor_write: args.write_coordinator_anchor,
    });
    match result {
        Ok(seal) => match encode(&seal, maximum) {
            Ok(bytes) => {
                if crate::deliver_json(&bytes).is_err() {
                    lost(&fallback);
                    return ExitCode::FAILURE;
                }
                ExitCode::SUCCESS
            }
            Err(_) => {
                if crate::deliver_json(&fallback).is_err() {
                    lost(&fallback);
                }
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "gazette: {}: {error}", error.code());
            // Even a native preflight error can follow permitted coordination activity.
            // Preserve exact requested coordinates, without replacing the original error.
            lost(&fallback);
            ExitCode::FAILURE
        }
    }
}

pub(super) fn inspect(args: Args) -> ExitCode {
    let input: SelectionInput = match input(args.max_input_bytes) {
        Ok(value) => value,
        Err(code) => return failure(code),
    };
    if !input.anchor_root.is_absolute() || !input.binding.journal_root.is_absolute() {
        return failure("coordinator_request");
    }
    let maximum = args.max_output_bytes;
    match crate::command::Kernel::new().execute(crate::command::coordinator::Inspect {
        anchor_root: input.anchor_root,
        binding: input.binding.0,
        options: args.options(),
    }) {
        Ok(state) => match encode(&state, maximum) {
            Ok(bytes) => match crate::deliver_json(&bytes) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => failure("coordinator_output_io"),
            },
            Err(code) => failure(code),
        },
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "gazette: {}: {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

pub(super) fn adopt(args: AdoptArgs) -> ExitCode {
    let input: AdoptionInput = match input(args.session.max_input_bytes) {
        Ok(value) => value,
        Err(code) => return failure(code),
    };
    if !input.anchor_root.is_absolute()
        || !input.binding.journal_root.is_absolute()
        || !input.restore_root.is_absolute()
        || !input.successor_root.is_absolute()
        || input.protected_roots.iter().any(|path| !path.is_absolute())
    {
        return failure("coordinator_request");
    }
    let maximum = args.session.max_output_bytes;
    let session = args.session.options();
    let result = crate::command::Kernel::new().execute(crate::command::coordinator::AdoptRestore {
        anchor_root: input.anchor_root,
        binding: input.binding.0,
        previous: input.previous.0,
        restore_root: input.restore_root,
        receipt_sha256: input.receipt_sha256,
        successor_root: input.successor_root,
        protected_roots: input.protected_roots,
        options: coordinator::RestoreAdoptionOptions {
            allow_adopt: args.adopt_restore,
            allow_anchor_write: args.write_coordinator_anchor,
            read: session.read,
            receipt_bytes: args.max_receipt_bytes,
        },
    });
    match result {
        Ok(receipt) => match encode(&receipt, maximum) {
            Ok(bytes) => match crate::deliver_json(&bytes) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => failure("coordinator_output_io"),
            },
            Err(code) => failure(code),
        },
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "gazette: {}: {error}", error.code());
            ExitCode::FAILURE
        }
    }
}

fn lost(fallback: &[u8]) {
    let mut error = io::stderr().lock();
    let _ = error.write_all(b"gazette: no complete coordinator seal delivered: ");
    let _ = error.write_all(fallback);
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: no complete coordinator report delivered; no automatic retry"
    );
    ExitCode::FAILURE
}
