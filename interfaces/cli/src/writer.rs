//! Explicit local generation advancement and lifetime; no release consumption.

use super::session::{emit, wait_for_close};
use crate::command::{Kernel, store_session as command};
use gazette_sdk::store::{self, JournalHeader, SessionOptions, WriterOptions};
use serde::Serialize;
use std::{
    convert::Infallible,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

fn reserve_open(_: &str, _: &command::Selection) -> Result<(), Infallible> {
    Ok(())
}

fn reserve_operation(
    _: &str,
    _: &command::Selection,
    _: Option<store::JournalHeader>,
    _: bool,
) -> Result<(), Infallible> {
    Ok(())
}

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing private journal directory containing only store.sqlite; no repair or migration.
    root: PathBuf,
    /// Complete independently retained header; mismatch never silently rebinds to current state.
    #[arg(long, value_parser = super::journal::parse_header)]
    expected_header: JournalHeader,
    /// Commit one local generation increment; interruption/lost output may still leave it committed.
    #[arg(long)]
    advance_writer_generation: bool,
    /// Hold this local namespace until one stdin byte or EOF; not a cross-host fence.
    #[arg(long)]
    reserve_local_coordination: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    Acquired {
        inspection: store::JournalInspection,
    },
    Closed,
}

pub(super) fn run(args: Args) -> ExitCode {
    let kernel = Kernel::new();
    let opened = match kernel.execute(command::Open {
        handle: "cli-writer-session".to_owned(),
        selection: command::Selection::Writer {
            root: args.root,
            expected_header: args.expected_header,
            allow_native_reads: true,
            options: WriterOptions {
                allow_generation_advance: args.advance_writer_generation,
                session: SessionOptions {
                    allow_local_reservation: args.reserve_local_coordination,
                    read: args.read.options(),
                },
            },
        },
        reserve: reserve_open,
    }) {
        Ok(report) => report,
        Err(error) => return command_failure(error),
    };
    let inspection = match opened.outcome {
        command::Outcome::Acquired { inspection } => inspection,
        command::Outcome::Failed { failure, .. } => return native_failure(&failure),
        _ => return failure("writer_session_protocol"),
    };
    if emit(Event::Acquired { inspection }).is_err() {
        return failure("writer_session_ready_delivery");
    }
    let input = wait_for_close();
    let closed = match kernel.execute(command::Operate {
        handle: opened.session,
        close: true,
        reserve: reserve_operation,
    }) {
        Ok(report) => report,
        Err(error) => return command_failure(error),
    };
    if let Err(error) = input {
        return failure(error.code());
    }
    match closed.outcome {
        command::Outcome::Closed => match emit(Event::Closed) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(error.code()),
        },
        command::Outcome::Failed { failure, .. } => native_failure(&failure),
        _ => failure("writer_session_protocol"),
    }
}

fn command_failure(error: command::Failure<Infallible>) -> ExitCode {
    let code = match error {
        command::Failure::Busy => "writer_session_busy",
        command::Failure::Unavailable => "writer_session_unavailable",
        command::Failure::Capacity => "writer_session_capacity",
        command::Failure::Unknown => "writer_session_unknown",
        command::Failure::HandleCollision => "writer_session_handle_collision",
        command::Failure::Selection => "writer_session_selection",
        command::Failure::NotWriter => "writer_session_not_writer",
        command::Failure::Reservation(never) => match never {},
    };
    failure(code)
}

fn native_failure(error: &command::NativeFailure) -> ExitCode {
    let generation = error.generation.map_or_else(
        || "generation effect unavailable".to_owned(),
        |effect| format!("generation effect {effect:?}"),
    );
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {}: native {:?} failed; {generation}; generation may have advanced; no successful session-close evidence; preserve and inspect before retrying",
        error.code,
        error.phase
    );
    ExitCode::FAILURE
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; generation was advanced or remains uncertain; no successful session-close evidence; preserve and inspect before retrying"
    );
    ExitCode::FAILURE
}
