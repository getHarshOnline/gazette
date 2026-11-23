//! Separate restricted recovery custody over the existing bounded LF transport.

use gazette_sdk::{
    release::{recovery_session, session::Delivery},
    store,
};
use serde::Serialize;
use std::{convert::Infallible, io, process::ExitCode};

use super::release_session as transport;
use crate::command::{Kernel, recovery_session as command};

fn reserve_noop(_: &str, _: &recovery_session::OpenRequest) -> Result<(), Infallible> {
    Ok(())
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event<'a> {
    Ready {
        inspection: &'a store::JournalInspection,
    },
    OpenFailed {
        error: &'a recovery_session::OpenError,
    },
    ReadyDeliveryFailed {
        close: &'a recovery_session::Outcome,
    },
}

pub(super) fn run(args: transport::Args) -> ExitCode {
    if !(1..=16 << 20).contains(&args.max_input_bytes)
        || !(1..=16 << 20).contains(&args.max_output_bytes)
    {
        return transport::failure("recovery_session_transport_limit");
    }
    let mut input = io::stdin().lock();
    let first = match transport::line(&mut input, args.max_input_bytes) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return transport::failure("recovery_session_open_required"),
        Err(code) => return transport::failure(code),
    };
    let selection = match recovery_session::parse_open(&first) {
        Ok(selection) => selection,
        Err(code) => return transport::failure(code),
    };
    let delivery = selection.coordinator.clone();
    let fallback = match transport::encode(
        &Delivery::new(&selection.coordinator, &first),
        args.max_output_bytes,
    ) {
        Ok(bytes) => bytes,
        Err(code) => return transport::failure(code),
    };
    let kernel = Kernel::new();
    let opened = match kernel.execute(command::Open {
        handle: "cli-recovery-session".to_owned(),
        request: selection,
        reserve: reserve_noop,
    }) {
        Ok(opened) => opened,
        Err(error) => return command_failure(error),
    };
    let inspection = match opened.outcome {
        command::OpenOutcome::Ready { inspection } => inspection,
        command::OpenOutcome::Failed { error } => {
            transport::respond_or_fallback(
                &Event::OpenFailed { error: &error },
                &fallback,
                args.max_output_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    let ready = match transport::encode(
        &Event::Ready {
            inspection: &inspection,
        },
        args.max_output_bytes,
    ) {
        Ok(bytes) => bytes,
        Err(_) => {
            // Explicit native close releases reservations, never seals dirty recovery clean.
            let close = match kernel.execute(command::Operate {
                handle: opened.session,
                operation: recovery_session::Operation::Close,
                reserve: reserve_noop,
            }) {
                Ok(report) => report.outcome,
                Err(error) => return command_failure(error),
            };
            transport::respond_or_fallback(
                &Event::ReadyDeliveryFailed { close: &close },
                &fallback,
                args.max_output_bytes,
            );
            return ExitCode::FAILURE;
        }
    };
    if transport::emit(&ready).is_err() {
        transport::lost(&fallback);
        return ExitCode::FAILURE;
    }
    let mut failed = false;
    loop {
        let bytes = match transport::line(&mut input, args.max_input_bytes) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return transport::interrupted("recovery_session_eof_without_close"),
            Err(code) => return transport::interrupted(code),
        };
        let operation = match recovery_session::parse_operation(&bytes) {
            Ok(operation) => operation,
            Err(code) => return transport::interrupted(code),
        };
        let fallback =
            match transport::encode(&Delivery::new(&delivery, &bytes), args.max_output_bytes) {
                Ok(bytes) => bytes,
                Err(code) => return transport::interrupted(code),
            };
        let report = match kernel.execute(command::Operate {
            handle: "cli-recovery-session".to_owned(),
            operation,
            reserve: reserve_noop,
        }) {
            Ok(report) => report,
            Err(error) => return command_failure(error),
        };
        let outcome = report.outcome;
        failed |= outcome.is_error();
        if !transport::respond_or_fallback(&outcome, &fallback, args.max_output_bytes) {
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
        command::Failure::Busy => "recovery_session_busy",
        command::Failure::Unavailable => "recovery_session_unavailable",
        command::Failure::Capacity => "recovery_session_capacity",
        command::Failure::Unknown => "recovery_session_unknown",
        command::Failure::HandleCollision => "recovery_session_handle_collision",
        command::Failure::Reservation(never) => match never {},
    };
    transport::failure(code)
}
