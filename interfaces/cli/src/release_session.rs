//! Bounded line transport over one SDK-owned release writer lifetime.

use crate::command::{Kernel, release_session as command};
use gazette_sdk::{release::session, render::LimitedWriter, store};
use serde::Serialize;
use std::{
    convert::Infallible,
    io::{self, BufRead, Read, Write},
    process::ExitCode,
};

fn reserve_noop(_: &str, _: &session::OpenRequest) -> Result<(), Infallible> {
    Ok(())
}

#[derive(clap::Args)]
pub(super) struct Args {
    /// Complete newline-terminated input-line ceiling, including LF; at most 16 MiB.
    #[arg(long, default_value_t = 4 << 20)]
    pub(super) max_input_bytes: usize,
    /// Complete output-line ceiling, including LF; recovery coordinates must fit before effects.
    #[arg(long, default_value_t = 4 << 20)]
    pub(super) max_output_bytes: usize,
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event<'a> {
    Ready {
        inspection: &'a store::JournalInspection,
    },
    OpenFailed {
        error: &'a session::OpenError,
    },
    ReadyDeliveryFailed {
        close: &'a session::Outcome,
    },
}

pub(super) fn run(args: Args) -> ExitCode {
    if !(1..=16 << 20).contains(&args.max_input_bytes)
        || !(1..=16 << 20).contains(&args.max_output_bytes)
    {
        return failure("release_session_transport_limit");
    }
    let mut input = io::stdin().lock();
    let first = match line(&mut input, args.max_input_bytes) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return failure("release_session_open_required"),
        Err(code) => return failure(code),
    };
    let selection = match session::parse_open(&first) {
        Ok(selection) => selection,
        Err(code) => return failure(code),
    };
    let delivery = selection.coordinator.clone();
    let fallback = match encode(
        &session::Delivery::new(&selection.coordinator, &first),
        args.max_output_bytes,
    ) {
        Ok(bytes) => bytes,
        Err(code) => return failure(code),
    };
    let kernel = Kernel::new();
    let opened = match kernel.execute(command::Open {
        handle: "cli-release-session".to_owned(),
        request: selection,
        reserve: reserve_noop,
    }) {
        Ok(opened) => opened,
        Err(error) => return command_failure(error),
    };
    let inspection = match opened.outcome {
        command::OpenOutcome::Ready { inspection } => inspection,
        command::OpenOutcome::Failed { error } => {
            respond_or_fallback(
                &Event::OpenFailed { error: &error },
                &fallback,
                args.max_output_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    let ready = match encode(
        &Event::Ready {
            inspection: &inspection,
        },
        args.max_output_bytes,
    ) {
        Ok(bytes) => bytes,
        Err(_) => {
            // No ready response was attempted: explicitly close, preserving its independent result.
            let close = match kernel.execute(command::Operate {
                handle: opened.session,
                operation: session::Operation::Close,
                reserve: reserve_noop,
            }) {
                Ok(report) => report.outcome,
                Err(error) => return command_failure(error),
            };
            respond_or_fallback(
                &Event::ReadyDeliveryFailed { close: &close },
                &fallback,
                args.max_output_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    if emit(&ready).is_err() {
        lost(&fallback);
        return ExitCode::FAILURE;
    }
    let mut failed = false;
    loop {
        let bytes = match line(&mut input, args.max_input_bytes) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return interrupted("release_session_eof_without_close"),
            Err(code) => return interrupted(code),
        };
        let operation = match session::parse_operation(&bytes) {
            Ok(operation) => operation,
            Err(code) => return interrupted(code),
        };
        let fallback = match encode(
            &session::Delivery::new(&delivery, &bytes),
            args.max_output_bytes,
        ) {
            Ok(bytes) => bytes,
            Err(code) => return interrupted(code),
        };
        let report = match kernel.execute(command::Operate {
            handle: "cli-release-session".to_owned(),
            operation,
            reserve: reserve_noop,
        }) {
            Ok(report) => report,
            Err(error) => return command_failure(error),
        };
        let outcome = report.outcome;
        failed |= outcome.is_error();
        if !respond_or_fallback(&outcome, &fallback, args.max_output_bytes) {
            return ExitCode::FAILURE;
        }
        if !report.retained {
            return if failed {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            };
        }
    }
}

fn command_failure(error: command::Failure<Infallible>) -> ExitCode {
    let code = match error {
        command::Failure::Busy => "release_session_busy",
        command::Failure::Unavailable => "release_session_unavailable",
        command::Failure::Capacity => "release_session_capacity",
        command::Failure::Unknown => "release_session_unknown",
        command::Failure::HandleCollision => "release_session_handle_collision",
        command::Failure::Reservation(never) => match never {},
    };
    failure(code)
}

pub(super) fn line(
    input: &mut impl BufRead,
    maximum: usize,
) -> Result<Option<Vec<u8>>, &'static str> {
    let mut bytes = Vec::new();
    input
        .take(maximum as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_| "release_session_input_io")?;
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > maximum {
        return Err("release_session_input_limit");
    }
    if bytes.last() != Some(&b'\n') {
        return Err("release_session_incomplete_line");
    }
    Ok(Some(bytes))
}

pub(super) fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, &'static str> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    serde_json::to_writer(&mut output, value).map_err(|_| "release_session_response_budget")?;
    output
        .write_all(b"\n")
        .map_err(|_| "release_session_response_budget")?;
    Ok(output.into_inner())
}

pub(super) fn emit(bytes: &[u8]) -> io::Result<()> {
    let mut output = io::stdout().lock();
    output.write_all(bytes)?;
    output.flush()
}

pub(super) fn respond_or_fallback(value: &impl Serialize, fallback: &[u8], maximum: usize) -> bool {
    match encode(value, maximum) {
        Ok(bytes) => {
            if emit(&bytes).is_ok() {
                return true;
            }
            lost(fallback);
        }
        Err(_) => {
            if emit(fallback).is_err() {
                lost(fallback);
            }
        }
    }
    false
}

pub(super) fn lost(fallback: &[u8]) {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(fallback);
    let _ = writeln!(
        stderr,
        "gazette: response delivery failed; no close receipt delivered. Preserve exact input and native custody; do not automatically reopen or retry."
    );
}

pub(super) fn interrupted(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; no explicit close receipt. Custody remains interrupted; preserve native journal and anchor, do not reopen or retry automatically."
    );
    ExitCode::FAILURE
}

pub(super) fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; no session readiness returned"
    );
    ExitCode::FAILURE
}
