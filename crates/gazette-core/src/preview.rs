//! Local listener policy and observations, never publication or release evidence.

use std::{collections::BTreeMap, fmt, io, net::SocketAddr, time::Duration};

use crate::{
    publication::routing::{PublicBaseUrl, RoutePath},
    render::MediaType,
    source::{CapturedSource, EntryPath},
};

/// Requested local socket and publication mount, before rendered output exists.
pub struct Binding<'a> {
    /// Explicit loopback endpoint; zero selects one native ephemeral port.
    pub address: SocketAddr,
    /// Configured public base supplies the mount path, not the local host identity.
    pub base_url: &'a PublicBaseUrl,
}

/// Exact output and its serving mount. The transport never rewrites rendered URLs.
pub struct Site<'a> {
    /// Immutable generated regular-file bytes, never a directory to reopen.
    pub output: &'a CapturedSource,
    /// Producer-declared generated formats; untyped static assets may be absent.
    pub media_types: &'a BTreeMap<EntryPath, MediaType>,
    /// Selected and emitted same-site Search page, when available.
    pub search_page: Option<&'a RoutePath>,
    /// Configured public base or explicit developer-local projection. Either
    /// retains the brand-owned path prefix; the transport consumes only that mount.
    pub base_url: &'a PublicBaseUrl,
}

/// Explicit local effects and finite per-connection work bounds.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Required acknowledgement that retained private output becomes locally reachable.
    pub allow_listeners: bool,
    /// Complete selected site/listener ceiling, not permission to shrink the set.
    pub max_sites: usize,
    /// Maximum request-line and header bytes retained per connection.
    pub max_header_bytes: usize,
    /// Maximum parsed request headers, including duplicates.
    pub max_headers: usize,
    /// Total request-header acquisition deadline; receiving bytes does not renew it.
    pub request_timeout: Duration,
    /// Total response-write deadline, not a fresh timeout per chunk.
    pub response_timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            allow_listeners: false,
            max_sites: 128,
            max_header_bytes: 16 << 10,
            max_headers: 64,
            request_timeout: Duration::from_secs(2),
            response_timeout: Duration::from_secs(2),
        }
    }
}

impl Options {
    /// Check effect acknowledgement and bounds before allocating or binding.
    ///
    /// # Errors
    /// Refuses missing permission, zero or unsupported bounds; no listener is opened.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_listeners {
            return Err(Error::Refused(
                "explicit loopback listener permission is required",
            ));
        }
        if !(1..=128).contains(&self.max_sites)
            || !(1..=1 << 20).contains(&self.max_header_bytes)
            || !(1..=1024).contains(&self.max_headers)
            || [self.request_timeout, self.response_timeout]
                .iter()
                .any(|timeout| timeout.is_zero() || *timeout > Duration::from_secs(30))
        {
            return Err(Error::Refused(
                "preview bounds are zero or exceed supported ceilings",
            ));
        }
        Ok(())
    }
}

/// One bounded transport step; response completion does not establish browser behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Poll {
    /// No pending client was accepted; the caller may wait before polling again.
    Idle,
    /// One complete HTTP response was written, including explicit error responses.
    Responded,
    /// A client disconnected, timed out or failed transport; no successful response claim.
    Disconnected,
}

/// Startup or listener failure, distinct from a malformed or disconnected client.
#[derive(Debug)]
pub enum Error {
    /// The exact requested effect, set, bindings or bounds cannot be accepted.
    Refused(&'static str),
    /// Native listener operation failed. No request/source bytes are included.
    Io(io::Error),
}

impl Error {
    /// Stable redacted class for SDK and interface diagnostics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused(_) => "preview_refused",
            Self::Io(_) => "preview_io",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(reason) => f.write_str(reason),
            Self::Io(error) => write!(f, "local preview listener failed: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Refused(_) => None,
        }
    }
}
