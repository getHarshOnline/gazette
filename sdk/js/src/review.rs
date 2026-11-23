//! Private publication review through the shared Rust acquisition and projection owners.

use gazette_sdk::{build, corpus, publication::content, source, toml, workspace};
use napi::{Result, bindgen_prelude::Uint8Array};
use napi_derive::napi;
use serde::Deserialize;

use crate::{Limits, encode, integer, query, transport};

/// Explicit source-read permission and independent transport ceilings.
#[napi(object)]
pub struct ReviewOptions {
    /// Permit exact configured source reads; never policy execution or publication.
    pub allow_read: bool,
    /// Complete workspace bytes before UTF-8 decoding; defaults to native TOML source bytes.
    pub input_bytes: Option<f64>,
    /// Complete private review JSON ceiling; defaults to 1 MiB.
    pub result_bytes: Option<f64>,
    /// Exact native limits JSON ceiling before decoding; defaults to 16 KiB.
    pub limits_bytes: Option<f64>,
    /// Named syntax, acquisition, corpus, graph and review limits as exact JSON.
    pub limits_json: Option<String>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct NativeLimits {
    syntax: toml::Limits,
    acquisition: source::PreparationLimits,
    corpus: corpus::Limits,
    graph: content::Limits,
    review: build::ReviewLimits,
}

impl Default for NativeLimits {
    fn default() -> Self {
        Self {
            syntax: toml::Limits::default(),
            acquisition: build::PublicationSetAcquisitionLimits::default().sources,
            corpus: corpus::Limits::default(),
            graph: content::Limits::default(),
            review: build::ReviewLimits::default(),
        }
    }
}

impl NativeLimits {
    fn parse(json: Option<&str>, maximum: usize) -> Result<Self> {
        let Some(json) = json else {
            return Ok(Self::default());
        };
        let bounds = Limits {
            syntax: toml::Limits::default(),
            query: maximum,
            result: 0,
        };
        let value: serde_json::Value = query(json, &bounds)?;
        for path in [
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/corpus",
            "/corpus/document",
            "/corpus/document/metadata",
            "/corpus/document/markdown",
            "/corpus/notebook",
            "/graph",
            "/review",
            "/review/graph",
        ] {
            if value.pointer(path).is_some_and(|value| !value.is_object()) {
                return Err(transport("node_query"));
            }
        }
        serde_json::from_value(value).map_err(|_| transport("node_query"))
    }
}

/// Review one exact site's publication from retained sources and one resolved model.
/// Returns the complete private native review, including drafts, nested uses and
/// uncompiled obligations. No Theme/resource program, policy process or store runs.
///
/// # Errors
/// Refuses missing read permission, invalid inputs, complete-pipeline failures and
/// independent native/transport bounds without a successful subset. Native errors
/// expose bounded classifications; a review digest grants no publication authority.
#[napi]
pub fn review_publication(
    bytes: Uint8Array,
    site: String,
    options: ReviewOptions,
) -> Result<String> {
    if !options.allow_read {
        return Err(transport("publication_permission"));
    }
    let maximum = integer(options.result_bytes, 1 << 20)?;
    let input = integer(options.input_bytes, toml::Limits::default().source_bytes)?;
    if bytes.len() > input {
        return Err(transport("node_input_limit"));
    }
    let limits = NativeLimits::parse(
        options.limits_json.as_deref(),
        integer(options.limits_bytes, 16 * 1024)?,
    )?;
    let site: workspace::Key = site.parse().map_err(|_| transport("publication_site"))?;
    let source = std::str::from_utf8(&bytes).map_err(|_| transport("node_source_utf8"))?;
    let inputs = build::acquire_publication(source, &site, limits.syntax, limits.acquisition)
        .map_err(|error| transport(error.code()))?;
    let parsed = build::parse_publication(&inputs, limits.corpus)
        .map_err(|error| transport(error.code()))?;
    let resolved = parsed
        .resolve(limits.graph)
        .map_err(|error| transport(error.code()))?;
    let review =
        build::review(&resolved, limits.review).map_err(|error| transport(error.code()))?;
    encode(&review, maximum)
}
