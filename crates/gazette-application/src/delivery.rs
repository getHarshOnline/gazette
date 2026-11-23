//! Selected served-byte observations, separate from compilation and release settlement.

use std::collections::BTreeSet;

use gazette_core::{
    http,
    publication::routing::{PublicBaseUrl, RoutePath},
    source::{CapturedSource, EntryPath, Sha256Digest, TreeIdentity},
};
use gazette_ports::http::Reader;
use serde::{Deserialize, Serialize};

/// Explicit addresses to compare with one retained output tree. A subset never
/// implies that unselected routes, a provider build or an entire site were verified.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Configured serving base, including its deployment prefix.
    pub base_url: PublicBaseUrl,
    /// Exact ordered site-relative routes; directory routes select their index file.
    pub routes: Vec<RoutePath>,
}

/// Per-response acquisition bounds and a complete-set request ceiling.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Explicit read, execution and network permissions for every GET.
    pub http: http::Options,
    /// Maximum selected routes, from 1 through 256. Responses are acquired sequentially.
    pub max_routes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            http: http::Options::default(),
            max_routes: 32,
        }
    }
}

/// Exact complete body identity, not an HTML interpretation or a source claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Body {
    /// Number of body bytes, excluding HTTP framing.
    pub bytes: u64,
    /// SHA-256 of those exact bytes.
    pub sha256: Sha256Digest,
}

impl Body {
    fn of(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len() as u64,
            sha256: Sha256Digest::of(bytes),
        }
    }
}

/// One route's outcome. No error, redirect or missing response is a successful match.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    /// A complete HTTP 200 body matches the retained file exactly.
    Matches {
        /// Complete observed body identity.
        observed: Body,
    },
    /// A complete HTTP 200 body differs; no normalization hides the difference.
    Differs {
        /// Complete observed body identity.
        observed: Body,
    },
    /// Non-200 response, including a deliberately unfollowed redirect.
    HttpFailure {
        /// Actual HTTP status, distinct from native process exit.
        http_status: u16,
        /// Complete error-response identity; its contents are not returned.
        observed: Body,
    },
    /// No complete bounded response is available. Partial bytes are not evidence.
    Unavailable {
        /// Stable acquisition refusal without private response bodies.
        reason: http::Error,
    },
}

/// Exact selected address and its retained expectation, even when acquisition fails.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Observation {
    /// Original normalized route; not replaced by a provider redirect.
    pub route: RoutePath,
    /// Exact requested URL, retaining the selected deployment prefix.
    pub url: String,
    /// Exact file in the retained output tree.
    pub output: EntryPath,
    /// Expected body identity derived from retained bytes, not newly fetched content.
    pub expected: Body,
    /// Complete comparison or explicit unavailability.
    pub outcome: Outcome,
}

/// Point-in-time delivery evidence; never included in deterministic artifact identity.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Report {
    /// Complete retained tree identity, independent from mutable HTTP responses.
    pub output: TreeIdentity,
    /// Original configured serving base.
    pub base_url: PublicBaseUrl,
    /// Every selected route in original order. No failed route is silently removed.
    pub observations: Vec<Observation>,
}

impl Report {
    /// Whether every explicitly selected route matched. This says nothing about
    /// unselected output, publisher approval, provider readiness or settlement.
    pub fn matches_selected_routes(&self) -> bool {
        !self.observations.is_empty()
            && self
                .observations
                .iter()
                .all(|observation| matches!(observation.outcome, Outcome::Matches { .. }))
    }
}

/// Whole-selection refusal before any request. Per-route acquisition failures
/// instead remain in the complete report and never change the selection.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    /// Empty, duplicate, missing-file or otherwise invalid selected route.
    Selection,
    /// Invalid configured bounds or a selected file too large for the response ceiling.
    Limit,
    /// Acquisition permissions or complete request validation failed before execution.
    Request(http::Error),
}

impl Error {
    /// Stable refusal shared by SDK and interfaces, without source paths.
    pub fn code(self) -> &'static str {
        match self {
            Self::Selection => "delivery_selection",
            Self::Limit => "delivery_limit",
            Self::Request(_) => "delivery_request",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for Error {}

/// Compare explicitly selected served routes against the supplied immutable capture.
/// Validates the entire selection before the first GET. Each route is acquired once;
/// failures do not trigger retries, redirects, source rereads or journal changes.
///
/// # Errors
/// Refuses invalid selections, permissions and bounds before network acquisition.
pub fn observe(
    reader: &impl Reader,
    output: &CapturedSource,
    selection: &Selection,
    options: Options,
) -> Result<Report, Error> {
    options.http.validate().map_err(Error::Request)?;
    if !(1..=256).contains(&options.max_routes) || selection.routes.len() > options.max_routes {
        return Err(Error::Limit);
    }
    if selection.routes.is_empty() {
        return Err(Error::Selection);
    }
    let headers = [
        ("Accept-Encoding", "identity"),
        ("Cache-Control", "no-cache"),
    ];
    let mut seen = BTreeSet::new();
    let mut planned = Vec::with_capacity(selection.routes.len());
    for route in &selection.routes {
        if !seen.insert(route) {
            return Err(Error::Selection);
        }
        let bytes = output.file(route.output()).ok_or(Error::Selection)?;
        if bytes.len() > options.http.response_bytes {
            return Err(Error::Limit);
        }
        let url = selection.base_url.resolve(route);
        http::Request {
            url: &url,
            headers: &headers,
            options: options.http,
        }
        .validate()
        .map_err(Error::Request)?;
        planned.push((route, url, Body::of(bytes)));
    }
    let observations = planned
        .into_iter()
        .map(|(route, url, expected)| {
            let response = reader.get(http::Request {
                url: &url,
                headers: &headers,
                options: options.http,
            });
            let outcome = match response {
                Ok(response) if response.body.len() > options.http.response_bytes => {
                    Outcome::Unavailable {
                        reason: http::Error::Limit,
                    }
                }
                Ok(response) if !(100..=599).contains(&response.status) => Outcome::Unavailable {
                    reason: http::Error::Response,
                },
                Ok(response) => {
                    let observed = Body::of(&response.body);
                    if response.status != 200 {
                        Outcome::HttpFailure {
                            http_status: response.status,
                            observed,
                        }
                    } else if observed == expected {
                        Outcome::Matches { observed }
                    } else {
                        Outcome::Differs { observed }
                    }
                }
                Err(reason) => Outcome::Unavailable { reason },
            };
            Observation {
                route: route.clone(),
                url,
                output: route.output().clone(),
                expected,
                outcome,
            }
        })
        .collect();
    Ok(Report {
        output: output.inventory().identity(),
        base_url: selection.base_url.clone(),
        observations,
    })
}
