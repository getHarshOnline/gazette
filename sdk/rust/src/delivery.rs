//! Served-output observations through the shared application and native HTTP adapter.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub use crate::http::Curl;
pub use gazette_application::delivery::{Body, Observation, Options, Outcome, Report, Selection};
use gazette_core::source::{CapturedSource, Limits, TreeIdentity};

/// Exact retained-output input shared by CLI, MCP and editor operation discovery.
/// A supplied tree pin is an expectation, not certification or publication authority.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Absolute existing retained-output root, never rebuilt or rewritten by this operation.
    pub tree: PathBuf,
    /// Independently retained identity of the complete selected output tree.
    pub tree_identity: TreeIdentity,
    /// Explicit native capture ceilings.
    pub source_limits: Limits,
    /// Explicitly pinned existing trusted transport; no executable discovery.
    pub curl: Curl,
    /// Configured serving base and exact ordered route subset.
    pub selection: Selection,
    /// Independent permissions and request ceilings.
    pub options: Options,
}

/// Redacted operation failure; route-level acquisition failures remain in the report.
#[derive(Debug)]
pub enum Error {
    /// Malformed, duplicate, positional or unsupported request fields.
    Request,
    /// Complete private input exceeds its selected byte ceiling.
    InputLimit,
    /// Native capture failed or differed from the independently supplied complete-tree pin.
    Capture(gazette_core::source::Error),
    /// Shared selection, permission or bound validation refused before HTTP requests.
    Observation(gazette_application::delivery::Error),
}

impl Error {
    /// Stable code, without private paths, response bytes or parser snippets.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Request => "delivery_input",
            Self::InputLimit => "delivery_input_limit",
            Self::Capture(_) => "delivery_capture",
            Self::Observation(error) => error.code(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for Error {}

impl Request {
    /// Decode one complete named request without native reads or network effects.
    ///
    /// # Errors
    /// Refuses oversized, duplicate, positional, malformed or unknown input fields.
    pub fn parse(bytes: &[u8], maximum: usize) -> Result<Self, Error> {
        if bytes.len() > maximum {
            return Err(Error::InputLimit);
        }
        let value = crate::json::parse(bytes).map_err(|_| Error::Request)?;
        if !value.is_object()
            || [
                "/tree_identity",
                "/source_limits",
                "/curl",
                "/curl/executable",
                "/selection",
                "/options",
                "/options/http",
            ]
            .into_iter()
            .any(|path| {
                !value
                    .pointer(path)
                    .is_some_and(serde_json::Value::is_object)
            })
        {
            return Err(Error::Request);
        }
        serde_json::from_value(value).map_err(|_| Error::Request)
    }
}

/// Inline discovery schema from the same request types as the native decoder.
/// Successful schema validation is not permission to perform the operation.
pub fn request_schema() -> schemars::Schema {
    schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<Request>()
}

/// Compare selected served routes directly against an already retained output.
/// No filesystem recapture, compilation, store mutation, retry or release occurs.
///
/// # Errors
/// Propagates shared whole-selection refusals; per-route failures remain explicit.
pub fn observe(
    output: &CapturedSource,
    curl: &Curl,
    selection: &Selection,
    options: Options,
) -> Result<Report, Error> {
    let curl = curl.native();
    let client = gazette_adapter_http::client::Client {
        runner: &gazette_adapter_process::Runner,
        curl: &curl,
    };
    gazette_application::delivery::observe(&client, output, selection, options)
        .map_err(Error::Observation)
}

/// Capture the exact selected output once, then observe the complete selected route set.
/// Permissions are checked before native capture. No pin is learned from the same tree,
/// and HTTP response bytes never replace the retained output or its identity.
///
/// # Errors
/// Refuses permissions, relative roots, capture mismatch and invalid selections/bounds.
pub fn observe_retained(request: &Request) -> Result<Report, Error> {
    request.options.http.validate().map_err(|error| {
        Error::Observation(gazette_application::delivery::Error::Request(error))
    })?;
    if !request.tree.is_absolute() {
        return Err(Error::Request);
    }
    let output =
        crate::source::capture_pinned(&request.tree, request.tree_identity, request.source_limits)
            .map_err(Error::Capture)?;
    observe(&output, &request.curl, &request.selection, request.options)
}
