//! Caller-selected lexical disclosure matching, without source, policy or effect authority.
//!
//! These matchers consume already selected bytes. Decoding belongs to the containing
//! format's owner; no matcher evaluates URLs, resolves paths, queries DNS or guesses rules.

use std::net::IpAddr;

use serde::Serialize;

mod decode;
mod matching;

/// Lexical algorithm identity, not binary attestation or complete disclosure certification.
pub const PROFILE: &str = "gazette-disclosure-patterns/1";

/// Explicit caller-owned selections. Decoding establishes shape, not semantic validity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Pattern {
    /// Exact case-sensitive UTF-8 byte substring; no normalization or interpretation.
    Literal {
        /// Nonempty selected bytes.
        value: String,
    },
    /// Exact absolute POSIX, drive-rooted Windows or UNC spelling with path boundaries.
    /// Roots use nonempty segments without `.` or `..`; only volume roots end in a separator.
    /// Case, separator spelling and encoded bytes are not canonicalized.
    PathTree {
        /// Selected root; matching includes descendants but never resolves them.
        root: String,
    },
    /// Exact ASCII DNS-style hostname, matched case-insensitively as one lexical token.
    /// One terminal DNS dot is accepted in input; subdomains are not implicit matches.
    /// URL authority, email and port delimiters are recognized as lexical boundaries only.
    Host {
        /// Unpadded hostname without a terminal dot, port or wildcard.
        name: String,
    },
    /// Native textual IP addresses and socket coordinates within the selected network.
    /// IPv4 and IPv6 remain distinct, including IPv4-mapped IPv6. Zone IDs are not supported.
    IpNetwork {
        /// Exact network base; host bits must be zero.
        address: IpAddr,
        /// Prefix length in the selected address family.
        prefix: u8,
    },
}

/// Invalid caller selection, with no selected private values retained in the error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error;

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid disclosure pattern")
    }
}

impl std::error::Error for Error {}

impl Pattern {
    /// Validate the declaration, not its truth, authority or applicability.
    ///
    /// # Errors
    /// Refuses empty literals, unsupported root/hostname spellings and non-network CIDRs.
    pub fn validate(&self) -> Result<(), Error> {
        let valid = match self {
            Self::Literal { value } => !value.is_empty(),
            Self::PathTree { root } => matching::path_kind(root).is_some(),
            Self::Host { name } => matching::hostname(name),
            Self::IpNetwork { address, prefix } => matching::network(*address, *prefix),
        };
        valid.then_some(()).ok_or(Error)
    }

    /// Conservative byte-work multiplier. Charge `input.len() * work_factor()` with
    /// checked arithmetic before matching. This is not a wall-clock or CPU sandbox.
    /// Literal/path search accounts for every candidate comparison; host scan is linear.
    /// IP scan allows two native parses of at most 53 bytes per token, without allocation.
    pub fn work_factor(&self) -> usize {
        match self {
            Self::Literal { value } => value.len().saturating_add(1),
            Self::PathTree { root } => root.len().saturating_add(16),
            Self::Host { .. } => 8,
            Self::IpNetwork { .. } => 512,
        }
    }

    /// Search supplied bytes after declaration validation and shared work-budget charging.
    /// Malformed or oversized lexical IP candidates are not interpreted as shorter addresses.
    /// This reports lexical matches only; it never classifies an entire output as safe.
    pub fn matches(&self, bytes: &[u8]) -> bool {
        match self {
            Self::Literal { value } => contains_literal(bytes, value),
            Self::PathTree { root } => matching::path(bytes, root),
            Self::Host { name } => matching::host(bytes, name),
            Self::IpNetwork { address, prefix } => matching::ip(bytes, *address, *prefix),
        }
    }
}

/// One exact substring owner for typed policies and existing borrowed literal rules.
/// Empty selections never match; callers charge their whole scan before invoking this.
pub fn contains_literal(bytes: &[u8], value: &str) -> bool {
    !value.is_empty()
        && bytes
            .windows(value.len())
            .any(|part| part == value.as_bytes())
}
