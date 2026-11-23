//! Bounded WOFF2 inspection over already retained bytes.

use gazette_core::font::{Decoded, Error, Limits};

/// Validate one complete WOFF2 container and its required decoded font metadata.
pub trait FontInspector {
    /// Inspect exact retained bytes without native reads, network access or rewriting.
    ///
    /// # Errors
    /// Refuses malformed, unsupported or over-limit WOFF2 and incomplete metadata.
    fn inspect(&self, bytes: &[u8], limits: Limits) -> Result<Decoded, Error>;
}
