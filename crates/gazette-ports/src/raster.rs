//! Decoder boundary over retained bytes; source selection and emission stay with the caller.

use gazette_core::raster::{Error, Format, Inspection, Limits, Metadata, MetadataLimits};

/// Decode actual pixels without native reads, network access or rewriting the original bytes.
pub trait RasterDecoder {
    /// Return exact source identity and stored dimensions after bounded pixel decoding.
    /// Reject animation instead of selecting an unrequested frame. Original metadata
    /// remains in the encoded source; success grants no sanitation or disclosure approval.
    ///
    /// # Errors
    /// Rejects invalid/unsupported encodings, exhausted ceilings and incomplete pixel output.
    fn inspect(&self, format: Format, bytes: &[u8], limits: Limits) -> Result<Inspection, Error>;
}

/// Bounded native metadata extraction, independently qualified from pixel decoding.
pub trait RasterMetadata {
    /// Inspect the complete supported container and return exact-byte textual evidence.
    /// Enforce chunk/count bounds before allocation and account for every logical
    /// decompression output plus retained field byte. Unsupported members fail closed.
    /// No source reads, rewriting, metadata removal or disclosure approval occurs.
    ///
    /// # Errors
    /// Refuses malformed or unsupported members and exhausted resource ceilings.
    fn metadata(
        &self,
        format: Format,
        bytes: &[u8],
        limits: MetadataLimits,
    ) -> Result<Metadata, Error>;
}
