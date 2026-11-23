//! Validated retained webfont contracts, independent of source selection and presentation.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::source::Sha256Digest;

/// Bounded WOFF2 inspection ceilings; decoder allocation accounting is not a process sandbox.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum retained WOFF2 bytes accepted before decoder construction.
    #[schemars(range(max = usize::MAX))]
    pub encoded_bytes: usize,
    /// Maximum reconstructed single-font SFNT bytes.
    #[schemars(range(max = usize::MAX))]
    pub decoded_bytes: usize,
    /// Maximum OpenType table count recorded by the WOFF2 container.
    pub tables: u16,
    /// Maximum glyph count accepted from the decoded `maxp` table.
    pub glyphs: u16,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 4 << 20,
            decoded_bytes: 32 << 20,
            tables: 256,
            glyphs: u16::MAX,
        }
    }
}

/// Exact-byte evidence from complete WOFF2 decompression and required SFNT metadata parsing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact supplied WOFF2 byte count.
    #[schemars(range(max = usize::MAX))]
    pub encoded_bytes: usize,
    /// Reconstructed single-font SFNT byte count.
    #[schemars(range(max = usize::MAX))]
    pub decoded_bytes: usize,
    /// Exact WOFF2 directory table count.
    pub tables: u16,
    /// Decoded glyph count from the required `maxp` table.
    pub glyphs: u16,
    /// Exact retained WOFF2 byte identity, including optional metadata/private blocks.
    pub sha256: Sha256Digest,
    /// Reconstructed SFNT identity used by strict disclosure and decoder cross-checks.
    pub decoded_sha256: Sha256Digest,
    /// Qualified decoder/parser profile, without native paths or mutable observations.
    pub tool: &'static str,
}

impl Inspection {
    /// Bind adapter evidence to the exact WOFF2 bytes and selected ceilings.
    ///
    /// # Errors
    /// Rejects identity, size, count or profile mismatches.
    pub fn validate(&self, bytes: &[u8], decoded: &[u8], limits: Limits) -> Result<(), Error> {
        if bytes.len() > limits.encoded_bytes
            || self.decoded_bytes > limits.decoded_bytes
            || self.tables == 0
            || self.tables > limits.tables
            || self.glyphs == 0
            || self.glyphs > limits.glyphs
        {
            return Err(Error::Limit("font bytes, tables or glyphs"));
        }
        if self.encoded_bytes != bytes.len()
            || self.sha256 != Sha256Digest::of(bytes)
            || self.decoded_bytes != decoded.len()
            || self.decoded_sha256 != Sha256Digest::of(decoded)
            || self.tool.is_empty()
        {
            return Err(Error::Binding);
        }
        Ok(())
    }
}

/// Complete bounded decode; bytes remain private caller evidence, not a publishable artifact.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// Exact validation evidence safe to retain in reports.
    pub inspection: Inspection,
    /// Complete reconstructed SFNT used for strict local inspection.
    pub bytes: Vec<u8>,
}

impl Decoded {
    /// Revalidate evidence against both retained WOFF2 and reconstructed SFNT bytes.
    ///
    /// # Errors
    /// Rejects identity, size, count or profile mismatches.
    pub fn validate(&self, encoded: &[u8], limits: Limits) -> Result<(), Error> {
        self.inspection.validate(encoded, &self.bytes, limits)
    }
}

/// Pure caller-supplied WOFF2 inspector over retained bytes only.
pub type Inspect<'a> = dyn Fn(&[u8], Limits) -> Result<Decoded, Error> + 'a;

/// Explicit absence for callers that have not selected a qualified WOFF2 inspector.
///
/// # Errors
/// Always returns unavailable; it never accepts unvalidated font bytes.
pub fn unavailable(_: &[u8], _: Limits) -> Result<Decoded, Error> {
    Err(Error::Unsupported("no WOFF2 inspector selected"))
}

/// Bounded WOFF2 inspection failure; input bytes and native diagnostics are not echoed.
#[derive(Debug)]
pub enum Error {
    /// A selected logical resource ceiling was exceeded.
    Limit(&'static str),
    /// The selected inspector cannot faithfully inspect this representation.
    Unsupported(&'static str),
    /// The WOFF2 container or required decoded font metadata is invalid.
    Decode,
    /// Adapter evidence does not bind the exact requested bytes.
    Binding,
}

impl Error {
    /// Stable interface failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Limit(_) => "font_limit",
            Self::Unsupported(_) => "font_unsupported",
            Self::Decode => "font_decode",
            Self::Binding => "font_binding",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => {
                write!(
                    formatter,
                    "font exceeds {resource}; no complete font accepted"
                )
            }
            Self::Unsupported(feature) => {
                write!(
                    formatter,
                    "font unavailable: {feature}; no fallback accepted"
                )
            }
            Self::Decode => {
                formatter.write_str("WOFF2 decoding or required font metadata validation failed")
            }
            Self::Binding => formatter.write_str("font evidence does not bind the supplied bytes"),
        }
    }
}

impl std::error::Error for Error {}
