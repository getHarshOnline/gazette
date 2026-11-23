//! Retained raster decoding through the same qualified adapter used by publication builds.

pub use gazette_core::raster::{
    Encoding, Error, Format, Inspect, InspectMetadata, Inspection, Limits, Metadata,
    MetadataLimits, Resize, unavailable, unavailable_metadata,
};
use gazette_ports::raster::{RasterDecoder, RasterMetadata};

/// Exact retained-raster decoder profile used by publication builds.
pub const PROFILE: &str = gazette_adapter_raster::PROFILE;

/// Decode retained PNG/JPEG/WebP pixels and return exact original-byte evidence.
/// No native reads, source rewriting, metadata stripping or publication approval occurs.
///
/// # Errors
/// Rejects unsupported/invalid encoded data and exceeded logical limits.
pub fn inspect(format: Format, bytes: &[u8], limits: Limits) -> Result<Inspection, Error> {
    gazette_adapter_raster::Decoder.inspect(format, bytes, limits)
}

/// Extract private raster metadata through the native adapter selected by this SDK.
/// This does not substitute for pixel decoding or certify disclosure safety.
///
/// # Errors
/// Rejects unsupported or malformed containers and exceeded metadata bounds.
pub fn metadata(format: Format, bytes: &[u8], limits: MetadataLimits) -> Result<Metadata, Error> {
    gazette_adapter_raster::Decoder.metadata(format, bytes, limits)
}
