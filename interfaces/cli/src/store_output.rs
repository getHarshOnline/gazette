//! Shared bounded delivery for native store creation, never reconstruction or automatic retry.

use gazette_sdk::{
    render::LimitedWriter,
    store::{
        OutputLimits,
        output::{ContractError, Report, Reservation},
    },
};
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct OutputArgs {
    /// Maximum resolved output-directory Unix bytes, including a generated child; at most 1 MiB.
    #[arg(long, default_value_t = OutputLimits::default().directory_bytes)]
    max_output_directory_bytes: usize,
    /// Maximum complete compact JSON response including LF; at most 16 MiB. Recovery evidence must fit before native work.
    #[arg(long, default_value_t = 256 << 10)]
    max_output_bytes: usize,
}

impl OutputArgs {
    pub(super) fn limits(&self) -> OutputLimits {
        OutputLimits {
            directory_bytes: self.max_output_directory_bytes,
        }
    }

    pub(super) fn reserve(&self) -> Result<Reservation, &'static str> {
        let payload = self
            .max_output_bytes
            .checked_sub(1)
            .filter(|_| self.max_output_bytes <= 16 << 20)
            .ok_or("store_output_response_limit")?;
        Reservation::new(self.limits(), payload)
    }

    pub(super) fn deliver<T: Serialize>(
        &self,
        result: &Result<Report<'_, T>, ContractError<'_, T>>,
    ) -> ExitCode {
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                // A violated reservation cannot promise bounded normal delivery. Preserve the
                // SDK's original locator and evidence on the independent diagnostic carrier.
                let mut stderr = io::stderr().lock();
                let _ = serde_json::to_writer(&mut stderr, &error.delivery);
                let _ = stderr.write_all(b"\n");
                drop(stderr);
                return failure(error.delivery.code);
            }
        };
        let failed = report.is_error();
        let mut output = LimitedWriter::new(Vec::new(), self.max_output_bytes);
        if serde_json::to_writer(&mut output, report).is_err() || output.write_all(b"\n").is_err() {
            // This should be unreachable with the same native output limit and reservation.
            let mut stderr = LimitedWriter::new(io::stderr().lock(), self.max_output_bytes);
            let _ = serde_json::to_writer(&mut stderr, report);
            let _ = stderr.write_all(b"\n");
            return failure("store_output_response_unavailable");
        }
        let output = output.into_inner();
        if super::deliver_json(&output).is_err() {
            let mut stderr = io::stderr().lock();
            let _ = stderr.write_all(&output);
            let _ = writeln!(
                stderr,
                "gazette: receipt delivery failed; retain this exact native report and locator. Store effects may remain; no automatic retry, overwrite, cleanup or activation."
            );
            return ExitCode::FAILURE;
        }
        match report {
            Report::Complete(_) => (),
            Report::Failed(error) => {
                let disposition = if error.directory_unix_bytes.is_some() {
                    "unverified output retained at exact directory_unix_bytes in the report"
                } else {
                    "native result reports no new store output created; coordination effects may remain"
                };
                let _ = writeln!(
                    io::stderr().lock(),
                    "gazette: {}: {disposition}; no automatic retry, cleanup or activation",
                    error.code
                );
            }
            Report::Delivery(_) => {
                let _ = failure("store_output_response_unavailable");
            }
        }
        if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }
}

pub(super) fn failure(code: &str) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}; no complete native receipt delivered. Preserve reported evidence and any native output; do not automatically retry or activate it."
    );
    ExitCode::FAILURE
}
