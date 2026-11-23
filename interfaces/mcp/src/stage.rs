//! Fresh retained staging through the SDK, with prebounded private failure evidence.

use gazette_cli::sdk::{render::LimitedWriter, source};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{os::unix::ffi::OsStrExt, path::Path};

use crate::{stage_check, tools};
use gazette_cli::command::{Kernel, rendered_output as command};

#[derive(Deserialize, JsonSchema)]
pub(super) struct Create {
    /// Explicit permission for fresh creation and retention; never replacement.
    create_stage: bool,
    /// Exact fresh child, if selected. A collision never chooses another name.
    name: Option<String>,
    /// Maximum complete canonical native directory bytes, checked before creation.
    directory_bytes: usize,
}

pub(super) struct Input(stage_check::Input<Create>);

pub(super) fn descriptor() -> Tool {
    let mut schema = schemars::schema_for!(stage_check::Request<Create>);
    Tool::new(
        "stage",
        "Use stage to retain a fresh private stage that composes the pinned project sources with the authored content. One call acquires exactly selected pinned project sources, captures current authored bytes, composes and retains one fresh private stage through Gazette's SDK. Requires independent allow_native_reads and create_stage permission, explicit placements (including []), absolute roots and acquisition/composition bounds. Optional name creates exactly that child with no collision fallback. directory_bytes bounds the complete canonical native path before creation; response capacity is reserved first. No fetch, checkout, source replacement, rendering, approval or publication. Retention survives caller disconnect. Failed delivery is not permission to retry or delete: retain the original inventory identity and lossless residue coordinates, then reconcile natively. Retaining a stage grants no approval or publication.",
        schema.ensure_object().clone(),
    )
    .with_raw_output_schema(tools::output_schema::<Report<'static, Retained<'static>>>())
    .with_annotations(ToolAnnotations::new().read_only(false).destructive(true).idempotent(false).open_world(true))
}

pub(super) fn decode(arguments: JsonObject) -> Result<Input, ErrorData> {
    let input = stage_check::decode_mode::<Create>(arguments)?;
    if !input.request.mode.create_stage
        || !(1..=1 << 20).contains(&input.request.mode.directory_bytes)
    {
        return Err(ErrorData::invalid_params(
            "stage requires create_stage: true and directory_bytes in 1..=1048576",
            None,
        ));
    }
    Ok(Input(input))
}

#[derive(Serialize, JsonSchema)]
struct Retained<'a> {
    status: RetainedStatus,
    workspace: &'a str,
    site: &'a str,
    inventory: &'a source::Inventory,
    directory: &'a Path,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RetainedStatus {
    Retained,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
enum FailureCode {
    #[serde(rename = "stage_preflight")]
    Preflight,
    #[serde(rename = "stage_operation")]
    Operation,
    #[serde(rename = "stage_residue")]
    Residue,
    #[serde(rename = "stage_receipt_delivery")]
    ReceiptDelivery,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Evidence {
    NoStage,
    UnverifiedResidue,
    Retained {
        /// Original complete native retention evidence, never recomputed from suspect output.
        inventory_identity: source::TreeIdentity,
    },
}

#[derive(Serialize, JsonSchema)]
pub(super) struct Incomplete<'a> {
    code: FailureCode,
    /// Native diagnostic when it fits; omitted in compact evidence delivery.
    message: Option<&'a str>,
    /// Actual Unix locator bytes; None means the native failure reports no new stage.
    directory_unix_bytes: Option<&'a [u8]>,
    evidence: Evidence,
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum Report<'a, T> {
    Complete(T),
    Incomplete(Incomplete<'a>),
}

/// Response storage is selected before acquisition or creation. The native writer
/// enforces its path bound on the actual canonical path, not the request spelling.
pub(super) struct Reservation {
    maximum: usize,
    buffer: Vec<u8>,
}

impl Reservation {
    pub(super) fn new(directory_bytes: usize, maximum: usize) -> Result<Self, &'static str> {
        if maximum == 0 || maximum > 16 << 20 || !(1..=1 << 20).contains(&directory_bytes) {
            return Err("stage_response_limit");
        }
        let mut directory = Vec::new();
        directory
            .try_reserve_exact(directory_bytes)
            .map_err(|_| "stage_response_allocation")?;
        directory.resize(directory_bytes, 255);
        let mut reservation = Self {
            maximum,
            buffer: Vec::new(),
        };
        reservation
            .buffer
            .try_reserve_exact(maximum)
            .map_err(|_| "stage_response_allocation")?;
        // TreeIdentity has a fixed-width digest projection, independent of entries.
        let identity = serde_json::from_value(serde_json::json!({
            "format": source::TreeIdentity::FORMAT,
            "sha256": source::Sha256Digest::of(&[]),
        }))
        .expect("fixed-width native inventory identity");
        for code in [
            FailureCode::Preflight,
            FailureCode::Operation,
            FailureCode::Residue,
            FailureCode::ReceiptDelivery,
        ] {
            for evidence in [
                Evidence::NoStage,
                Evidence::UnverifiedResidue,
                Evidence::Retained {
                    inventory_identity: identity,
                },
            ] {
                reservation
                    .write(&Incomplete {
                        code,
                        message: None,
                        directory_unix_bytes: Some(&directory),
                        evidence,
                    })
                    .map_err(|_| "stage_response_limit")?;
            }
        }
        reservation.buffer.clear();
        Ok(reservation)
    }

    fn write(&mut self, value: &impl Serialize) -> Result<(), serde_json::Error> {
        self.buffer.clear();
        serde_json::to_writer(
            &mut LimitedWriter::new(&mut self.buffer, self.maximum),
            value,
        )
    }

    fn finish<T: Serialize>(
        mut self,
        report: &Report<'_, T>,
        fallback: Incomplete<'_>,
    ) -> Result<CallToolResult, ErrorData> {
        let mut failed = matches!(report, Report::Incomplete(_));
        if self.write(report).is_err() {
            failed = true;
            // The writer's bound and the pre-effect reservation prove this fits.
            // No native reread, mutation, cleanup or retry occurs during delivery.
            self.write(&fallback).map_err(|_| ErrorData::internal_error(
                "Stage evidence violated its reserved delivery contract; native effects may remain; do not retry or delete",
                None,
            ))?;
        }
        tools::encoded(self.buffer, failed).map_err(|_| {
            ErrorData::internal_error(
                "Stage response encoding failed after native work; do not retry or delete",
                None,
            )
        })
    }
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let input = self.0;
        let request = input.request;
        let reservation = match Reservation::new(request.mode.directory_bytes, maximum) {
            Ok(reservation) => reservation,
            Err(code) => {
                return tools::failure(
                    code,
                    "No stage created: response capacity cannot retain native evidence",
                    maximum,
                );
            }
        };
        let context = input.context.clone();
        let result = Kernel::new().execute(command::CreateStage {
            input: command::StageInput {
                allow_read: request.allow_native_reads,
                context: input.context,
                placements: input.placements,
                preparation: request.preparation,
                authored: request.authored,
                parent: request.parent,
                limits: request.limits,
            },
            name: request.mode.name,
            directory_bytes: Some(request.mode.directory_bytes),
        });
        match result {
            Ok(outcome) => {
                let (_, retained) = outcome.into_parts();
                deliver(reservation, &context, &Ok(retained))
            }
            Err(error) => match error.into_stage_error() {
                Ok(error) => deliver(reservation, &context, &Err(error)),
                Err(error) => tools::failure(error.code(), error.message(), maximum),
            },
        }
    }
}

fn deliver(
    reservation: Reservation,
    context: &source::StageContext,
    result: &Result<source::RetainedStage, source::StageError>,
) -> Result<CallToolResult, ErrorData> {
    deliver_with(reservation, result, |retained| Retained {
        status: RetainedStatus::Retained,
        workspace: &context.workspace,
        site: &context.site,
        inventory: retained.inventory(),
        directory: retained.path(),
    })
}

// Both source composition and rendered publication retain the same native custody.
// Only their full success evidence differs; fallback never trusts caller-supplied pins.
pub(super) fn deliver_with<'a, T: Serialize>(
    reservation: Reservation,
    result: &'a Result<source::RetainedStage, source::StageError>,
    complete: impl FnOnce(&'a source::RetainedStage) -> T,
) -> Result<CallToolResult, ErrorData> {
    match result {
        Ok(retained) => reservation.finish(
            &Report::Complete(complete(retained)),
            Incomplete {
                code: FailureCode::ReceiptDelivery,
                message: None,
                directory_unix_bytes: Some(retained.path().as_os_str().as_bytes()),
                evidence: Evidence::Retained {
                    inventory_identity: retained.inventory().identity(),
                },
            },
        ),
        Err(error) => {
            let (code, path, evidence) = match error {
                source::StageError::Preflight(_) => {
                    (FailureCode::Preflight, None, Evidence::NoStage)
                }
                source::StageError::Operation(_) => {
                    (FailureCode::Operation, None, Evidence::NoStage)
                }
                source::StageError::Residue { path, .. } => (
                    FailureCode::Residue,
                    Some(path.as_os_str().as_bytes()),
                    Evidence::UnverifiedResidue,
                ),
            };
            let message = error.to_string();
            reservation.finish(
                &Report::<T>::Incomplete(Incomplete {
                    code,
                    message: Some(&message),
                    directory_unix_bytes: path,
                    evidence,
                }),
                Incomplete {
                    code,
                    message: None,
                    directory_unix_bytes: path,
                    evidence,
                },
            )
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/stage.rs"]
mod tests;
