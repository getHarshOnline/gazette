//! Retained raster decoding and transformation contracts; no source access or disclosure grant.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::source::Sha256Digest;

mod metadata;
pub use metadata::{InspectMetadata, Metadata, MetadataLimits, unavailable_metadata};
mod transform;
pub use transform::{Dimensions, Encoding, Geometry, Resize};

/// Explicit encoded representation, never guessed from a filename or unrelated MIME alternative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// A single-frame PNG image.
    Png,
    /// A JPEG image.
    Jpeg,
    /// A single-image WebP container; animation remains a separate capability.
    Webp,
}

impl Format {
    /// Fixed media type suitable for transporting these exact encoded bytes.
    pub fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
        }
    }
}

/// Per-image logical limits; decoder allocation accounting is not a process sandbox.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Limits {
    /// Maximum encoded input bytes, checked before decoder construction.
    pub encoded_bytes: usize,
    /// Maximum decoded pixel buffer in bytes, also supplied to decoder allocation
    /// limiting where supported. This does not bound all decoder-internal allocations.
    pub decoded_bytes: u64,
    /// Maximum pixel width.
    pub width: u32,
    /// Maximum pixel height.
    pub height: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            encoded_bytes: 4 << 20,
            decoded_bytes: 64 << 20,
            width: 8192,
            height: 8192,
        }
    }
}

/// Decoder evidence of actual pixel decoding, not sanitation or publication certification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Verified browser display extent after recognized orientation; absent when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<Dimensions>,
    /// Explicit format that was decoded.
    pub format: Format,
    /// Stored pixel width, before any orientation or display transformation.
    #[schemars(range(max = u32::MAX))]
    pub width: u32,
    /// Stored pixel height, before any orientation or display transformation.
    #[schemars(range(max = u32::MAX))]
    pub height: u32,
    /// Exact supplied encoded-byte length.
    #[schemars(range(max = usize::MAX))]
    pub bytes: usize,
    /// Exact supplied encoded-byte identity, including retained metadata.
    pub sha256: Sha256Digest,
    /// Qualified decoder/profile identity, without native paths or mutable observations.
    pub tool: &'static str,
}

impl Inspection {
    /// Bind decoder observations to the supplied bytes and explicit dimensional ceilings.
    /// Pixel decoding itself remains the decoder's proof obligation.
    ///
    /// # Errors
    /// Rejects mismatched identity, format, empty dimensions/profile or exceeded bounds.
    pub fn validate(&self, format: Format, bytes: &[u8], limits: Limits) -> Result<(), Error> {
        if bytes.len() > limits.encoded_bytes
            || self.width > limits.width
            || self.height > limits.height
        {
            return Err(Error::Limit("encoded bytes or dimensions"));
        }
        if self.display.is_some_and(|size| {
            size.width == 0
                || size.height == 0
                || !((size.width == self.width && size.height == self.height)
                    || (size.width == self.height && size.height == self.width))
        }) {
            return Err(Error::Binding);
        }
        if self.format != format
            || self.bytes != bytes.len()
            || self.sha256 != Sha256Digest::of(bytes)
            || self.width == 0
            || self.height == 0
            || self.tool.is_empty()
        {
            return Err(Error::Binding);
        }
        Ok(())
    }
}

/// Pure caller-supplied decoder function. It may consume only the supplied retained bytes.
pub type Inspect<'a> = dyn Fn(Format, &[u8], Limits) -> Result<Inspection, Error> + 'a;

/// Explicit absence for pure inspection callers that have not selected a raster decoder.
///
/// # Errors
/// Always returns unavailable; it never accepts unvalidated image bytes.
pub fn unavailable(_: Format, _: &[u8], _: Limits) -> Result<Inspection, Error> {
    Err(Error::Unsupported("no raster decoder selected"))
}

/// Bounded raster failure; input metadata and decoder error strings are not echoed.
#[derive(Debug)]
pub enum Error {
    /// A selected logical resource ceiling was exceeded.
    Limit(&'static str),
    /// A format or feature cannot be faithfully decoded by the selected profile.
    Unsupported(&'static str),
    /// Encoded data could not be completely decoded under the selected pixel or metadata profile.
    Decode,
    /// Decoder evidence did not describe the exact requested bytes and format.
    Binding,
}

impl Error {
    /// Stable interface failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Limit(_) => "raster_limit",
            Self::Unsupported(_) => "raster_unsupported",
            Self::Decode => "raster_decode",
            Self::Binding => "raster_binding",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => {
                write!(f, "raster exceeds {resource}; no complete image accepted")
            }
            Self::Unsupported(feature) => {
                write!(f, "raster unavailable: {feature}; no fallback accepted")
            }
            Self::Decode => f.write_str("raster decoding failed; no complete image accepted"),
            Self::Binding => {
                f.write_str("raster decoder evidence does not bind the supplied image")
            }
        }
    }
}

impl std::error::Error for Error {}
