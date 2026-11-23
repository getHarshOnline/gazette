//! Metadata extraction over retained bytes, distinct from pixel-decoding evidence.

use serde::Serialize;

use super::{Error, Format};
use crate::source::Sha256Digest;

/// Logical extraction bounds, not an operating-system allocation or execution sandbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct MetadataLimits {
    /// Entire encoded image, checked before native parser construction.
    pub encoded_bytes: usize,
    /// Maximum encoded chunk or marker payload; enforced by the provider before allocation.
    pub chunk_bytes: usize,
    /// Aggregate logical decompression output plus retained UTF-8 metadata field bytes.
    pub decoded_bytes: usize,
    /// Maximum retained metadata fields, including empty fields.
    pub values: usize,
    /// Maximum native chunks or markers visited, enforced by the provider.
    pub chunks: usize,
}

impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            encoded_bytes: 4 << 20,
            chunk_bytes: 1 << 20,
            decoded_bytes: 4 << 20,
            values: 4096,
            chunks: 4096,
        }
    }
}

/// Exact-byte extraction evidence; fields are private scan inputs, not public image output.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Metadata {
    /// Explicit format inspected by the selected provider.
    pub format: Format,
    /// Exact encoded input length.
    pub bytes: usize,
    /// Exact encoded input identity, including metadata.
    pub sha256: Sha256Digest,
    /// Qualified extraction profile, not binary attestation or certification.
    pub tool: &'static str,
    /// All logical decompression output plus retained UTF-8 field bytes, for shared charging.
    pub decoded_bytes: usize,
    /// Decoded textual metadata in native order, without guessing semantics or approval.
    pub fields: Vec<String>,
}

impl Metadata {
    /// Bind the report to exact supplied bytes and validate observable aggregate bounds.
    /// Native member closure, chunk/count ceilings and decompression accounting remain
    /// provider obligations; they cannot be reconstructed from the extracted fields.
    ///
    /// # Errors
    /// Refuses identity/profile mismatch, underreported field bytes and exceeded bounds.
    pub fn validate(
        &self,
        format: Format,
        bytes: &[u8],
        limits: MetadataLimits,
    ) -> Result<(), Error> {
        if bytes.len() > limits.encoded_bytes
            || self.fields.len() > limits.values
            || self.decoded_bytes > limits.decoded_bytes
        {
            return Err(Error::Limit("metadata bytes or values"));
        }
        if self.format != format
            || self.bytes != bytes.len()
            || self.sha256 != Sha256Digest::of(bytes)
            || self.tool.is_empty()
            || self.tool.len() > 256
            || !self.tool.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err(Error::Binding);
        }
        let fields = self.fields.iter().try_fold(0usize, |total, field| {
            total
                .checked_add(field.len())
                .ok_or(Error::Limit("metadata bytes"))
        })?;
        if fields > self.decoded_bytes {
            return Err(Error::Binding);
        }
        Ok(())
    }
}

/// Selected pure extractor; it consumes only supplied retained bytes, without native reads.
pub type InspectMetadata<'a> =
    dyn Fn(Format, &[u8], MetadataLimits) -> Result<Metadata, Error> + 'a;

/// Explicitly absent metadata capability; pixel decoding cannot substitute for it.
///
/// # Errors
/// Always returns unsupported, without accepting any metadata coverage.
pub fn unavailable_metadata(_: Format, _: &[u8], _: MetadataLimits) -> Result<Metadata, Error> {
    Err(Error::Unsupported("no raster metadata inspector selected"))
}
