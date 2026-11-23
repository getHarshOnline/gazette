//! Complete-set qualification is a native SDK operation, not a JavaScript policy engine.

use gazette_sdk::{build, process, source::Sha256Digest};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Serialize;

use crate::{encode, integer, transport};

/// Independent native permissions and complete transport bounds.
#[napi(object)]
pub struct QualificationOptions {
    /// Permit exact configured source reads; this does not permit checker execution.
    pub allow_read: bool,
    /// Permit the explicitly configured unsandboxed policy checker; default denied.
    pub allow_policy_execution: Option<bool>,
    /// Complete original JSON input ceiling; default 4 MiB.
    pub input_bytes: Option<f64>,
    /// Complete report or effect-aware fallback JSON ceiling; default 4 MiB.
    pub result_bytes: Option<f64>,
}

#[derive(Serialize)]
struct Unavailable {
    code: &'static str,
    message: &'static str,
    request_sha256: Sha256Digest,
}

/// Qualify every selected site through the shared native recipe, once.
/// Native reads and policy execution have independent permissions; generation
/// requests retain their own explicit permissions. No stage or release is created.
///
/// # Errors
/// Invalid bytes, recipes and impossible delivery bounds fail before execution.
/// Incomplete native outcomes throw the complete report. If it cannot be encoded,
/// a pre-reserved failure binds the original request and warns that configured
/// programs may have run. Missing delivery never establishes rollback or retry safety.
#[napi]
pub fn qualify_sites(bytes: Uint8Array, options: QualificationOptions) -> Result<String> {
    let input = integer(options.input_bytes, 4 << 20)?;
    let maximum = integer(options.result_bytes, 4 << 20)?;
    if bytes.len() > input {
        return Err(transport("node_input_limit"));
    }
    let native = build::QualificationOptions {
        allow_read: options.allow_read,
        policy: process::Options {
            allow_execute: options.allow_policy_execution.unwrap_or(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let recipe = build::QualificationRecipe::parse_json(&bytes, input, native.acquisition.sites)
        .map_err(|_| transport("qualification_input"))?;
    let unavailable = encode(
        &Unavailable {
            code: "qualification_response_unavailable",
            message: "No complete qualification report delivered; configured checkers or generators may have run. Preserve the original recipe and inspect native outcomes before further work; do not infer rollback or retry. No release preparation performed.",
            request_sha256: Sha256Digest::of(&bytes),
        },
        maximum,
    )?;
    let report = recipe.execute(native);
    match encode(&report, maximum) {
        Ok(encoded) if report.is_complete() => Ok(encoded),
        Ok(encoded) => Err(Error::new(Status::GenericFailure, encoded)),
        Err(_) => Err(Error::new(Status::GenericFailure, unavailable)),
    }
}
