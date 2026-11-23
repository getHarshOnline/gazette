use gazette_core::source::{CommitId, Sha256Digest};
use serde::{Deserialize, Serialize};

use crate::Error;

/// Exact existing build and expected output commit selected by the caller.
/// The remote comes from publication configuration; no second repository name is inferred.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PagesBuild {
    /// Literal HTTPS github.com repository remote, with optional `.git` suffix.
    pub remote: String,
    /// Positive provider build identifier; never `latest` or a moving branch.
    pub build_id: u64,
    /// Exact prepared commit to compare with the provider's reported commit.
    pub expected_commit: CommitId,
}

impl PagesBuild {
    pub(crate) fn endpoint(&self) -> Result<String, Error> {
        let repository = self
            .remote
            .strip_prefix("https://github.com/")
            .ok_or(Error::Selection)?;
        let repository = repository.strip_suffix(".git").unwrap_or(repository);
        let parts: Vec<_> = repository.split('/').collect();
        if self.remote.len() > 512
            || self.build_id == 0
            || parts.len() != 2
            || parts.iter().any(|part| {
                part.is_empty()
                    || *part == "."
                    || *part == ".."
                    || !part
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
            })
        {
            return Err(Error::Selection);
        }
        Ok(format!(
            "https://api.github.com/repos/{repository}/pages/builds/{}",
            self.build_id
        ))
    }
}

/// Provider state, not publication permission or served-site verification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BuildState {
    /// GitHub reports a completed build.
    Built,
    /// GitHub reports a build error.
    Errored,
    /// Queued or building; not a terminal success.
    Pending,
    /// A new or unsupported native state; its exact spelling remains in the observation.
    Unknown,
}

/// Exact bound provider fields, without pusher identity or arbitrary error messages.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Build {
    /// Provider's exact record URL, checked against the requested repository and build ID.
    pub url: String,
    /// Native full commit coordinate, not a Git advertisement.
    pub commit: CommitId,
    /// Exact commit comparison only; no current branch or artifact-byte claim.
    pub matches_expected_commit: bool,
    /// Native status spelling, retained even when unknown.
    pub native_status: String,
    /// Bounded classification of the native status.
    pub state: BuildState,
}

/// HTTP acquisition outcome. In particular, 404 does not prove absence or an unborn repository.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HttpOutcome {
    /// Complete successful response bound to the selected build.
    Observed {
        /// Provider-native observation; matching success is not release settlement.
        build: Build,
    },
    /// The provider denied access or the public-only transport requires authentication.
    AccessDenied,
    /// HTTP 403 alone cannot distinguish a permission failure from GitHub rate limiting.
    ForbiddenOrRateLimited,
    /// Missing or concealed evidence, never a successful empty observation.
    NotFoundOrHidden,
    /// Rate limit response; no automatic retry or cached fallback.
    RateLimited,
    /// Other HTTP status, including a redirect that was deliberately not followed.
    Unavailable,
}

/// Complete point-in-time response evidence, excluded from deterministic artifact identity.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PagesObservation {
    /// Original explicit selection, including exact remote spelling and expected commit.
    pub selection: PagesBuild,
    /// Fixed API request endpoint derived from that selection.
    pub endpoint: String,
    /// Native HTTP status; not replaced by the process exit status.
    pub http_status: u16,
    /// Identity of the complete response body, including failures; body contents stay private.
    pub response_sha256: Sha256Digest,
    /// Complete response byte count.
    pub response_bytes: usize,
    /// Native evidence or explicit unavailability.
    pub outcome: HttpOutcome,
}

pub(crate) fn decode(
    selection: &PagesBuild,
    endpoint: String,
    status: u16,
    body: &[u8],
) -> Result<PagesObservation, Error> {
    let outcome = match status {
        200 => {
            let value = gazette_core::json::parse(body).map_err(|_| Error::Binding)?;
            let text = |key| {
                value
                    .get(key)
                    .and_then(serde_json::Value::as_str)
                    .ok_or(Error::Binding)
            };
            let native_url = text("url")?;
            // GitHub repository names are case-insensitive. Every other URL component
            // must match: no redirects, encoded aliases, query strings or other hosts.
            let coordinate = |value: &str| {
                value
                    .strip_prefix("https://api.github.com/repos/")
                    .and_then(|path| path.split_once("/pages/builds/"))
                    .map(|(repository, build)| (repository.to_ascii_lowercase(), build.to_owned()))
            };
            if coordinate(native_url) != coordinate(&endpoint) {
                return Err(Error::Binding);
            }
            let commit: CommitId = text("commit")?.parse().map_err(|_| Error::Binding)?;
            let native_status = text("status")?;
            if native_status.is_empty()
                || native_status.len() > 64
                || native_status.chars().any(char::is_control)
            {
                return Err(Error::Binding);
            }
            let state = match native_status {
                "built" => BuildState::Built,
                "errored" => BuildState::Errored,
                "queued" | "building" => BuildState::Pending,
                _ => BuildState::Unknown,
            };
            HttpOutcome::Observed {
                build: Build {
                    url: native_url.to_owned(),
                    matches_expected_commit: commit == selection.expected_commit,
                    commit,
                    native_status: native_status.to_owned(),
                    state,
                },
            }
        }
        401 => HttpOutcome::AccessDenied,
        403 => HttpOutcome::ForbiddenOrRateLimited,
        404 => HttpOutcome::NotFoundOrHidden,
        429 => HttpOutcome::RateLimited,
        _ => HttpOutcome::Unavailable,
    };
    Ok(PagesObservation {
        selection: selection.clone(),
        endpoint,
        http_status: status,
        response_sha256: Sha256Digest::of(body),
        response_bytes: body.len(),
        outcome,
    })
}
