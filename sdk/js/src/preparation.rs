//! Local preparation delegates effects and bounded custody delivery to the Rust SDK.

use gazette_sdk::{build, process, release_commit, source, store, toml};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Deserialize;

use crate::{Limits, encode, integer, query, transport};

/// Explicit local permissions; omitted effect flags deny the corresponding operation.
#[napi(object)]
pub struct PrepareOptions {
    /// Acknowledge local preparation, not certification or publication.
    pub local: bool,
    /// Permit exact configured input reads.
    pub allow_read: bool,
    /// Permit fresh output staging.
    pub create_stage: Option<bool>,
    /// Permit durable preparation records.
    pub record_preparation: Option<bool>,
    /// Permit the native writer generation to advance.
    pub advance_writer_generation: Option<bool>,
    /// Permit a local coordinator reservation.
    pub reserve_local_coordination: Option<bool>,
    /// Permit a selected coordinator anchor write.
    pub write_coordinator_anchor: Option<bool>,
    /// Permit independent local Git worktree initialization.
    pub initialize_worktree: Option<bool>,
    /// Permit explicitly configured native policy checker execution.
    pub allow_policy_execution: Option<bool>,
    /// Complete recipe byte ceiling; native TOML source-byte default.
    pub input_bytes: Option<f64>,
    /// Complete native report or reserved reconciliation JSON ceiling; default 1 MiB.
    pub result_bytes: Option<f64>,
    /// Supplied limits JSON ceiling; default 16 KiB.
    pub limits_bytes: Option<f64>,
    /// Native syntax/acquisition/output/read options as exact named JSON.
    /// Coordination-file permission belongs to the native read options and defaults denied.
    pub limits_json: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeLimits {
    syntax: toml::Limits,
    acquisition: build::PublicationSetAcquisitionLimits,
    output: source::Limits,
    read: store::ReadOptions,
}

impl NativeLimits {
    fn parse(json: Option<&str>, maximum: usize) -> Result<Self> {
        let Some(json) = json else {
            return Ok(Self::default());
        };
        let value: serde_json::Value = query(
            json,
            &Limits {
                syntax: toml::Limits::default(),
                query: maximum,
                result: 0,
            },
        )?;
        for path in [
            "/syntax",
            "/acquisition",
            "/output",
            "/read",
            "/acquisition/sources",
            "/acquisition/sources/per_source",
            "/acquisition/sources/per_source/content",
        ] {
            if value.pointer(path).is_some_and(|v| !v.is_object()) {
                return Err(transport("node_query"));
            }
        }
        serde_json::from_value(value).map_err(|_| transport("node_query"))
    }
}

/// Prepare the complete requested local site set through the native SDK, once.
/// No journal initialization, signing, push or release authorization is performed.
/// Selected checkers/math commands are separate explicit effects, not sandboxed by Node.
///
/// # Errors
/// Refuses host permissions, malformed inputs and bounds before execution. Native
/// failure throws the complete report, including partial effects. If that report
/// cannot fit, throws pre-reserved original reconciliation coordinates instead.
/// A fallback that cannot fit prevents execution. Never infer rollback or retry
/// authority from an error, lost delivery or the original request coordinates.
#[napi]
pub fn prepare_sites(bytes: Uint8Array, options: PrepareOptions) -> Result<String> {
    if !options.local || !options.allow_read {
        return Err(transport("publication_permission"));
    }
    let input = integer(options.input_bytes, toml::Limits::default().source_bytes)?;
    let maximum = integer(options.result_bytes, 1 << 20)?;
    if bytes.len() > input {
        return Err(transport("node_input_limit"));
    }
    let limits = NativeLimits::parse(
        options.limits_json.as_deref(),
        integer(options.limits_bytes, 16 * 1024)?,
    )?;
    let recipe = build::LocalPreparationRecipe::parse_json(&bytes, input, limits.acquisition.sites)
        .map_err(|_| transport("local_preparation_input"))?;
    let native = build::LocalPreparationRunOptions {
        allow_read: options.allow_read,
        syntax: limits.syntax,
        acquisition: limits.acquisition,
        output: limits.output,
        preparation: build::LocalPreparationOptions {
            allow_anchor_write: options.write_coordinator_anchor.unwrap_or(false),
            stage: build::PreparationOptions {
                allow_stage: options.create_stage.unwrap_or(false),
                output: limits.output,
                record: store::PreparationWriteOptions {
                    allow_record: options.record_preparation.unwrap_or(false),
                },
                writer: store::WriterOptions {
                    allow_generation_advance: options.advance_writer_generation.unwrap_or(false),
                    session: store::SessionOptions {
                        allow_local_reservation: options
                            .reserve_local_coordination
                            .unwrap_or(false),
                        read: limits.read,
                    },
                },
                ..Default::default()
            },
            policy: process::Options {
                allow_execute: options.allow_policy_execution.unwrap_or(false),
                ..Default::default()
            },
            worktree: release_commit::WorktreeOptions {
                allow_initialize: options.initialize_worktree.unwrap_or(false),
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let (report, failed) = recipe.execute_with_delivery(native, |delivery, failed| {
        encode(delivery, maximum).map(|report| (report, failed))
    })?;
    if failed {
        Err(Error::new(Status::GenericFailure, report))
    } else {
        Ok(report)
    }
}
