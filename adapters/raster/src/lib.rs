//! Decodes retained PNG and JPEG bytes into pixel evidence, with no native source
//! access.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. PNG uses checksum-checked frame decoding and reads
//! through the end chunk. JPEG uses the decoder's strict mode. Neither result is a
//! sanitation certificate: original metadata and any decoder-tolerated trailing bytes
//! remain unchanged. Logical output limits are not a process-wide memory or execution
//! sandbox.

mod jpeg;
mod jpeg_metadata;
mod metadata;
mod png;
mod transform;
mod webp;

pub use transform::Compiler;

use gazette_core::{
    raster::{Dimensions, Error, Format, Inspection, Limits, Metadata, MetadataLimits},
    source::Sha256Digest,
};
use gazette_ports::raster::{RasterDecoder, RasterMetadata};

/// Complete qualified decoder behavior used for pre-render dependency identity.
pub const PROFILE: &str = "gazette.raster/2;display-orientation;png/0.18.0;single-frame;finish;jpeg=zune-jpeg/0.5.16-rc2+zune-core/0.5.3;strict;webp=image-webp/0.2.4;closed-single-image";

/// Stateless decoder of explicitly selected, single-image raster representations.
#[derive(Clone, Copy, Debug, Default)]
pub struct Decoder;

impl RasterMetadata for Decoder {
    fn metadata(
        &self,
        format: Format,
        bytes: &[u8],
        limits: MetadataLimits,
    ) -> Result<Metadata, Error> {
        if bytes.len() > limits.encoded_bytes {
            return Err(Error::Limit("metadata encoded bytes"));
        }
        match format {
            Format::Png => metadata::inspect(bytes, limits),
            Format::Jpeg => jpeg_metadata::inspect(bytes, limits),
            Format::Webp => webp::metadata(bytes, limits),
        }
    }
}

impl RasterDecoder for Decoder {
    fn inspect(&self, format: Format, bytes: &[u8], limits: Limits) -> Result<Inspection, Error> {
        if bytes.len() > limits.encoded_bytes {
            return Err(Error::Limit("encoded bytes"));
        }
        let (width, height, display, tool) = match format {
            Format::Webp => {
                let decoded = webp::pixels(bytes, limits)?;
                (
                    decoded.width,
                    decoded.height,
                    Some(Dimensions {
                        width: decoded.width,
                        height: decoded.height,
                    }),
                    webp::PROFILE,
                )
            }
            Format::Png => {
                let (width, height, display) = png::decode(bytes, limits)?;
                (
                    width,
                    height,
                    display,
                    "gazette.raster/2;display-orientation;png/0.18.0;single-frame;finish",
                )
            }
            Format::Jpeg => {
                let (width, height, display) = jpeg::decode(bytes, limits)?;
                (
                    width,
                    height,
                    display,
                    "gazette.raster/2;display-orientation;zune-jpeg/0.5.16-rc2;zune-core/0.5.3;strict",
                )
            }
        };
        let inspection = Inspection {
            display,
            format,
            width,
            height,
            bytes: bytes.len(),
            sha256: Sha256Digest::of(bytes),
            tool,
        };
        inspection.validate(format, bytes, limits)?;
        Ok(inspection)
    }
}

fn dimensions(width: u32, height: u32, limits: Limits) -> Result<(), Error> {
    if width == 0 || height == 0 {
        return Err(Error::Decode);
    }
    if width > limits.width || height > limits.height {
        return Err(Error::Limit("dimensions"));
    }
    Ok(())
}

fn pixel_buffer(size: Option<usize>, limits: Limits) -> Result<Vec<u8>, Error> {
    let size = size.ok_or(Error::Limit("decoded bytes"))?;
    if u64::try_from(size).map_err(|_| Error::Limit("decoded bytes"))? > limits.decoded_bytes {
        return Err(Error::Limit("decoded bytes"));
    }
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(size)
        .map_err(|_| Error::Limit("pixel allocation"))?;
    buffer.resize(size, 0);
    Ok(buffer)
}

fn oriented(width: u32, height: u32, orientation: image::metadata::Orientation) -> (u32, u32) {
    use image::metadata::Orientation;
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => (height, width),
        _ => (width, height),
    }
}
