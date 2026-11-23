//! Direct publication delivery keeps generator receipts even when rendering or stdout fails.

use crate::command::{publication, rendered_output};
use gazette_sdk::{build, source};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    os::unix::ffi::OsStrExt,
    process::ExitCode,
};

#[derive(Serialize)]
struct Report<'a> {
    #[serde(flatten)]
    publication: &'a Value,
    #[serde(skip_serializing_if = "no_receipts")]
    documentation: &'a [build::DocumentationReceipt],
}

fn no_receipts(receipts: &[build::DocumentationReceipt]) -> bool {
    receipts.is_empty()
}

/// Consume exceptional native custody while the SDK still owns the original operation.
pub(super) fn deliver(
    execution: rendered_output::DocumentationRenderSiteExecution<'_>,
) -> ExitCode {
    use rendered_output::DocumentationRenderSiteExecution as E;
    match execution {
        E::Rendered {
            outcome: Ok(outcome),
            receipts,
        } => {
            if receipts.is_empty() {
                return crate::write_stage_json(outcome.report(), outcome.retained());
            }
            emit(
                &Report {
                    publication: outcome.report(),
                    documentation: receipts,
                },
                true,
            )
        }
        E::Rendered {
            outcome: Err(error),
            receipts,
        } => {
            let mut report = json!({
                "status": "documentation-publication-failed",
                "code": error.code(),
                "error": error.message(),
            });
            if let Some(stage) = error.stage_error() {
                report["stage"] = stage_failure(stage);
            }
            emit(
                &Report {
                    publication: &report,
                    documentation: receipts,
                },
                false,
            )
        }
        E::Refused { receipts, error } => {
            let report = json!({
                "status": "documentation-publication-failed",
                "code": error.code(),
                "error": error.to_string(),
            });
            emit(
                &Report {
                    publication: &report,
                    documentation: receipts,
                },
                false,
            )
        }
        E::Native {
            site,
            receipts,
            execution,
        } => match publication::documentation_failure(site, receipts, execution) {
            Ok(report) => emit(&report, false),
            Err(error) => super::failure(error.code(), &error),
        },
    }
}

// Exact native residue bytes remain usable even when the path is not UTF-8.
fn stage_failure(error: &source::StageError) -> Value {
    match error {
        source::StageError::Preflight(cause) => {
            json!({"phase":"preflight", "cause":cause.to_string()})
        }
        source::StageError::Operation(cause) => {
            json!({"phase":"operation", "cause":cause.to_string()})
        }
        source::StageError::Residue {
            cause,
            path,
            cleanup,
        } => json!({
            "phase":"residue", "cause":cause.to_string(),
            "path_unix_bytes":path.as_os_str().as_bytes(),
            "cleanup":cleanup.as_ref().map(ToString::to_string),
        }),
    }
}

fn emit(report: &impl Serialize, complete: bool) -> ExitCode {
    let mut bytes = match serde_json::to_vec_pretty(report) {
        Ok(bytes) => bytes,
        Err(error) => return super::failure("documentation_publication_response", &error),
    };
    bytes.push(b'\n');
    if let Err(error) = crate::deliver_json(&bytes) {
        let mut diagnostic = io::stderr().lock();
        let _ = writeln!(
            diagnostic,
            "gazette: output_io: {error}; original documentation and publication evidence follows; inspect before retrying"
        );
        let _ = diagnostic.write_all(&bytes);
        return ExitCode::FAILURE;
    }
    if complete {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
