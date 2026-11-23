//! Bounded explicit HTTP observations, separate from artifact identity and publication authority.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, net::IpAddr};

use crate::process;

/// Independent native permissions and complete response bounds.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    /// Permit executable, library and certificate reads.
    pub allow_read: bool,
    /// Permit the explicitly selected native executable.
    pub allow_execute: bool,
    /// Permit the explicit request, including loopback connections.
    pub allow_network: bool,
    /// Complete body ceiling, from one byte through 16 MiB.
    pub response_bytes: usize,
    /// Complete native lifetime in milliseconds, from one through 300000.
    pub timeout_ms: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            allow_read: false,
            allow_execute: false,
            allow_network: false,
            response_bytes: 1 << 20,
            timeout_ms: 30000,
        }
    }
}

impl Options {
    /// Check permission and finite bounds before native acquisition.
    ///
    /// # Errors
    /// Refuses any missing permission or an unsupported bound.
    pub fn validate(self) -> Result<(), Error> {
        if !(self.allow_read && self.allow_execute && self.allow_network) {
            return Err(Error::Permission);
        }
        if self.response_bytes == 0
            || self.response_bytes > 16 << 20
            || !(1..=300000).contains(&self.timeout_ms)
        {
            return Err(Error::Limit);
        }
        Ok(())
    }
}

/// One unauthenticated GET. Header values are data, never command arguments or file references.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// Exact selected HTTPS URL, or literal numeric loopback HTTP URL.
    pub url: &'a str,
    /// Explicit bounded headers without credential, host or framing overrides.
    pub headers: &'a [(&'a str, &'a str)],
    /// Independent native permissions and bounds.
    pub options: Options,
}

impl Request<'_> {
    /// Check syntax and scope without DNS, source reads or effects.
    ///
    /// # Errors
    /// Refuses credentials, fragments, ambiguous URLs, remote plaintext HTTP,
    /// duplicate/unsafe headers and exhausted bounds.
    pub fn validate(&self) -> Result<(), Error> {
        self.options.validate()?;
        if self.url.len() > 8192 || self.headers.len() > 32 {
            return Err(Error::Limit);
        }
        if self
            .url
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
            || self.url.contains(['\\', '#'])
        {
            return Err(Error::Selection);
        }
        let parsed = url::Url::parse(self.url).map_err(|_| Error::Selection)?;
        let authority = self
            .url
            .split_once("://")
            .ok_or(Error::Selection)?
            .1
            .split(['/', '?'])
            .next()
            .ok_or(Error::Selection)?;
        if authority.is_empty()
            || authority.contains('@')
            || parsed.host().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
        {
            return Err(Error::Selection);
        }
        match parsed.scheme() {
            "https" if self.url.starts_with("https://") => (),
            "http" if self.url.starts_with("http://") => {
                let host = if let Some(host) = authority.strip_prefix('[') {
                    host.split_once(']').ok_or(Error::Selection)?.0
                } else {
                    authority.split(':').next().ok_or(Error::Selection)?
                };
                if !host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
                    return Err(Error::Selection);
                }
            }
            _ => return Err(Error::Selection),
        }
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for (name, value) in self.headers {
            total = total
                .checked_add(name.len())
                .and_then(|n| n.checked_add(value.len()))
                .filter(|n| *n <= 8192)
                .ok_or(Error::Limit)?;
            let lower = name.to_ascii_lowercase();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
                || value.chars().any(|ch| ch.is_control())
                || matches!(
                    lower.as_str(),
                    "authorization"
                        | "proxy-authorization"
                        | "cookie"
                        | "host"
                        | "connection"
                        | "content-length"
                        | "transfer-encoding"
                        | "upgrade"
                        | "expect"
                        | "user-agent"
                )
                || lower.starts_with("proxy-")
                || !names.insert(lower)
            {
                return Err(Error::Selection);
            }
        }
        Ok(())
    }
}

/// Complete HTTP response; even a successful transport may report a failed HTTP status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    /// Final observed HTTP status without following redirects.
    pub status: u16,
    /// Complete bounded body; never a truncated prefix.
    pub body: Vec<u8>,
}

/// Redacted transport refusal, without URLs, credentials or provider body text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    /// Native permission is absent.
    Permission,
    /// Request or native executable selection is invalid.
    Selection,
    /// Requested or observed complete bounds were exceeded.
    Limit,
    /// Native failure, retaining direct-child custody evidence.
    Transport {
        /// Native failure class.
        failure: process::Failure,
        /// Direct-child custody; not a network-effect guarantee.
        cleanup: process::Cleanup,
    },
    /// Incomplete, failed or malformed native response.
    Response,
}

impl Error {
    /// Stable shared diagnostic code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Permission => "http_permission",
            Self::Selection => "http_selection",
            Self::Limit => "http_limit",
            Self::Transport { .. } => "http_transport",
            Self::Response => "http_response",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for Error {}
