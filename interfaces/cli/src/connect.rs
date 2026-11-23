//! A directly operated local session, with explicit readiness and close events.

use super::session::{emit, wait_for_close};
use crate::command::{Kernel, store_session as command};
use gazette_sdk::store::{self, SessionOptions};
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
    /// Existing local `SQLite` store; never initialized, migrated or written.
    path: PathBuf,
    /// Existing private local coordination directory shared by cooperating callers.
    coordination_root: PathBuf,
    /// Reserve that directory until close; not a journal epoch or release grant.
    #[arg(long)]
    reserve_local_coordination: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    Connected { observation: store::Observation },
    Closed,
}

pub(super) fn run(args: Args) -> ExitCode {
    let kernel = Kernel::new();
    let opened = match kernel.execute(command::Open {
        handle: "cli-store-session".to_owned(),
        selection: command::Selection::Connection {
            path: args.path,
            coordination_root: args.coordination_root,
            allow_native_reads: true,
            options: SessionOptions {
                allow_local_reservation: args.reserve_local_coordination,
                read: args.read.options(),
            },
        },
        reserve: reserve_open,
    }) {
        Ok(report) => report,
        Err(error) => return command_failure(error),
    };
    let observation = match opened.outcome {
        command::Outcome::Connected { observation } => observation,
        command::Outcome::Failed { failure, .. } => return native_failure(&failure),
        _ => return failure("store_session_protocol"),
    };
    if emit(Event::Connected { observation }).is_err() {
        return failure("store_session_ready_delivery");
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
        _ => failure("store_session_protocol"),
    }
}

fn command_failure(error: command::Failure<Infallible>) -> ExitCode {
    let code = match error {
        command::Failure::Busy => "store_session_busy",
        command::Failure::Unavailable => "store_session_unavailable",
        command::Failure::Capacity => "store_session_capacity",
        command::Failure::Unknown => "store_session_unknown",
        command::Failure::HandleCollision => "store_session_handle_collision",
        command::Failure::Selection => "store_session_selection",
        command::Failure::NotWriter => "store_session_not_writer",
        command::Failure::Reservation(never) => match never {},
    };
    failure(code)
}

fn native_failure(error: &command::NativeFailure) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {}: native {:?} failed; local session ended without successful close evidence",
        error.code,
        error.phase
    );
    ExitCode::FAILURE
}

fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; local session ended without successful close evidence"
    );
    ExitCode::FAILURE
}
