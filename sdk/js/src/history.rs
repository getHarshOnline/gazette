//! Immutable release-history projection; the Rust SDK owns storage and verification.

use std::path::PathBuf;

use gazette_sdk::{json, source::Sha256Digest, store};
use napi::{Error, Result, Status, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::{Deserialize, Serialize};

use crate::{encode, integer, transport};

/// Explicit database-read permission and independent host transport ceilings.
#[napi(object)]
pub struct HistoryOptions {
    /// Permit the selected database read; journal coordinates never grant permission.
    pub allow_read: bool,
    /// Maximum exact request JSON bytes before decoding; default 16 KiB.
    pub input_bytes: Option<f64>,
    /// Maximum complete report or reserved failure envelope bytes; default 1 MiB.
    pub result_bytes: Option<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Authorization,
    Leases,
    Consumption,
    Attempts,
    Reconciliations,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    kind: Kind,
    database: PathBuf,
    record_id: String,
    expected_header: Option<store::JournalHeader>,
    #[serde(default)]
    read: store::ReadOptions,
}

#[derive(Serialize)]
struct Unavailable {
    code: &'static str,
    message: &'static str,
    request_sha256: Sha256Digest,
}

/// Inspect one exact record's native authorization, leases, consumption, attempts
/// or reconciliation history. This never renews authority, acquires a writer,
/// resamples destinations, retries delivery or repairs state.
///
/// # Errors
/// Refuses malformed requests, missing read/coordination permission, native limits
/// and independently expected journal mismatch. Native errors retain their codes.
/// A complete-report delivery failure identifies the exact request and possible
/// coordination-file effects without returning partial history.
#[napi]
pub fn inspect_release_history(bytes: Uint8Array, options: HistoryOptions) -> Result<String> {
    if !options.allow_read {
        return Err(transport("storage_permission"));
    }
    let input = integer(options.input_bytes, 16 * 1024)?;
    let maximum = integer(options.result_bytes, 1 << 20)?;
    if bytes.len() > input {
        return Err(transport("node_query_limit"));
    }
    // Inspect map shape with the duplicate-safe parser, then deserialize original
    // bytes so journal coordinates and native bounds never round through a JS value.
    let shape = json::parse(&bytes).map_err(|_| transport("node_query"))?;
    if !shape.is_object()
        || shape.get("read").is_some_and(|value| !value.is_object())
        || shape
            .get("expected_header")
            .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(transport("node_query"));
    }
    let request: Request = serde_json::from_slice(&bytes).map_err(|_| transport("node_query"))?;
    if !request.database.is_absolute() {
        return Err(transport("store_unsafe_path"));
    }
    let unavailable = encode(
        &Unavailable {
            code: "history_response_unavailable",
            message: "No complete history report delivered; database reads and explicitly permitted coordination-file effects may have occurred. No release, renewal, repair or retry performed.",
            request_sha256: Sha256Digest::of(&bytes),
        },
        maximum,
    )?;
    let expected = request.expected_header.as_ref();
    let encoded = match request.kind {
        Kind::Authorization => encode(
            &store::inspect_release_authorization(
                &request.database,
                &request.record_id,
                expected,
                request.read,
            )
            .map_err(|error| transport(error.code()))?,
            maximum,
        ),
        Kind::Leases => encode(
            &store::inspect_release_leases(
                &request.database,
                &request.record_id,
                expected,
                request.read,
            )
            .map_err(|error| transport(error.code()))?,
            maximum,
        ),
        Kind::Consumption => encode(
            &store::inspect_release_consumption(
                &request.database,
                &request.record_id,
                expected,
                request.read,
            )
            .map_err(|error| transport(error.code()))?,
            maximum,
        ),
        Kind::Attempts => encode(
            &store::inspect_release_attempts(
                &request.database,
                &request.record_id,
                expected,
                request.read,
            )
            .map_err(|error| transport(error.code()))?,
            maximum,
        ),
        Kind::Reconciliations => encode(
            &store::inspect_release_reconciliations(
                &request.database,
                &request.record_id,
                expected,
                request.read,
            )
            .map_err(|error| transport(error.code()))?,
            maximum,
        ),
    };
    encoded.map_err(|_| Error::new(Status::GenericFailure, unavailable))
}
