//! Retained WOFF2 decoding through the same qualified adapter used by publication builds.

pub use gazette_core::font::{Decoded, Error, Inspect, Inspection, Limits, unavailable};
use gazette_ports::font::FontInspector;

/// Exact WOFF2 validation profile used by publication builds.
pub const PROFILE: &str = gazette_adapter_fonts::PROFILE;

/// Decode one retained WOFF2 and return exact encoded/reconstructed evidence.
/// No native reads, source rewriting, font discovery or publication approval occurs.
///
/// # Errors
/// Rejects unsupported/invalid WOFF2 and exceeded logical limits.
pub fn inspect(bytes: &[u8], limits: Limits) -> Result<Decoded, Error> {
    gazette_adapter_fonts::Inspector.inspect(bytes, limits)
}
