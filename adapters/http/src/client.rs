//! Explicit pinned curl GET over the existing bounded native process owner.

use gazette_core::{
    http::{Error, Request, Response},
    process,
};
use gazette_ports::{http::Reader, process::CaptureRunner};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

/// Existing trusted curl selection, not executable discovery or dependency attestation.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Curl {
    /// Absolute selected executable path.
    pub program: String,
    /// Complete executable byte pin, checked by the native process owner.
    pub executable: process::ExecutablePin,
    /// Absolute existing working directory; not a filesystem sandbox.
    pub directory: String,
}

/// Caller-selected composition; no process adapter dependency or hidden runtime.
pub struct Client<'a, R> {
    /// Existing native output-capture owner.
    pub runner: &'a R,
    /// Explicit trusted executable selection.
    pub curl: &'a Curl,
}

impl<R: CaptureRunner> Reader for Client<'_, R> {
    fn get(&self, request: Request<'_>) -> Result<Response, Error> {
        request.validate()?;
        let mut arguments = [
            "--disable",
            "--silent",
            "--globoff",
            "--proto",
            if request.url.starts_with("https://") {
                "=https"
            } else {
                "=http"
            },
            "--proxy",
            "",
            "--request",
            "GET",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        for (name, value) in request.headers {
            arguments.push("--header".into());
            arguments.push(format!("{name}: {value}"));
        }
        arguments.extend(
            [
                "--user-agent",
                "Gazette/0.1",
                "--write-out",
                "\n%{http_code}",
                "--url",
                request.url,
            ]
            .into_iter()
            .map(str::to_owned),
        );
        let command = process::Command {
            program: self.curl.program.clone(),
            executable: Some(self.curl.executable),
            directory: self.curl.directory.clone(),
            environment: BTreeMap::new(),
            arguments,
        };
        command.validate().map_err(|_| Error::Selection)?;
        let capture = process::CaptureRequest {
            request: process::Request {
                command: &command,
                input: &[],
                options: process::Options {
                    allow_execute: true,
                    max_input_bytes: 0,
                    timeout: Duration::from_millis(request.options.timeout_ms),
                },
            },
            max_output_bytes: request.options.response_bytes + 4,
            max_stderr_bytes: None,
        };
        capture.validate().map_err(|_| Error::Selection)?;
        let mut captured = self
            .runner
            .capture(capture)
            .map_err(|error| Error::Transport {
                failure: error.failure,
                cleanup: error.cleanup,
            })?;
        if captured.stdout.len() > request.options.response_bytes + 4 {
            return Err(Error::Limit);
        }
        if captured.exit.code != Some(0) || captured.stdout.len() < 4 {
            return Err(Error::Response);
        }
        let body_end = captured.stdout.len() - 4;
        let trailer = &captured.stdout[body_end..];
        if trailer[0] != b'\n' || !trailer[1..].iter().all(u8::is_ascii_digit) {
            return Err(Error::Response);
        }
        let status = u16::from(trailer[1] - b'0') * 100
            + u16::from(trailer[2] - b'0') * 10
            + u16::from(trailer[3] - b'0');
        if !(100..=599).contains(&status) {
            return Err(Error::Response);
        }
        captured.stdout.truncate(body_end);
        Ok(Response {
            status,
            body: captured.stdout,
        })
    }
}
