//! Served-output observations through the native SDK request and complete report.

use gazette_sdk::{delivery, source::Sha256Digest};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Serialize;

use crate::{encode, integer, transport};

/// Host bounds only; all permissions remain in the native request's `options.http`.
#[derive(Default)]
#[napi(object)]
pub struct DeliveryOptions {
    /// Complete original request JSON byte ceiling; default 1 MiB.
    pub input_bytes: Option<f64>,
    /// Complete report or pre-reserved unavailable envelope ceiling; default 1 MiB.
    pub result_bytes: Option<f64>,
}

#[derive(Serialize)]
struct Unavailable {
    code: &'static str,
    message: &'static str,
    request_sha256: Sha256Digest,
}

/// Compare every selected served route with the independently pinned retained output.
/// The shared request owns read, native execution and network permissions. This
/// operation neither rebuilds output nor performs publication or release settlement.
///
/// # Errors
/// Native whole-request failures retain their stable codes. Any nonmatching route
/// throws the complete report, not a reduced successful set. A response envelope is
/// reserved before native effects; if full delivery fails, it identifies the exact
/// request and warns that observations may already have occurred. No retry is inferred.
#[napi]
pub fn observe_delivery(bytes: Uint8Array, options: Option<DeliveryOptions>) -> Result<String> {
    let options = options.unwrap_or_default();
    let input = integer(options.input_bytes, 1 << 20)?;
    let maximum = integer(options.result_bytes, 1 << 20)?;
    let request =
        delivery::Request::parse(&bytes, input).map_err(|error| transport(error.code()))?;
    let unavailable = encode(
        &Unavailable {
            code: "delivery_response_unavailable",
            message: "No complete served-output report delivered; native reads and HTTP requests may have occurred. Preserve the original request and inspect native outcomes; do not infer rollback or retry. No publication or release settlement performed.",
            request_sha256: Sha256Digest::of(&bytes),
        },
        maximum,
    )?;
    let report = delivery::observe_retained(&request).map_err(|error| transport(error.code()))?;
    match encode(&report, maximum) {
        Ok(encoded) if report.matches_selected_routes() => Ok(encoded),
        Ok(encoded) => Err(Error::new(Status::GenericFailure, encoded)),
        Err(_) => Err(Error::new(Status::GenericFailure, unavailable)),
    }
}
