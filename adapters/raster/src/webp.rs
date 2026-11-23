//! Closed single-image RIFF coverage plus bounded native WebP pixel decoding.
//! Metadata-bearing, animated and unknown chunks refuse rather than disappearing.

use std::io::Cursor;

use gazette_core::{
    raster::{Error, Format, Limits, Metadata, MetadataLimits},
    source::Sha256Digest,
};

pub(super) const PROFILE: &str = "gazette.raster/1;image-webp/0.2.4;closed-single-image";
const METADATA_PROFILE: &str = "gazette.raster-metadata/1;image-webp/0.2.4;closed-webp-no-metadata";

pub(super) struct Pixels {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) alpha: bool,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn pixels(bytes: &[u8], limits: Limits) -> Result<Pixels, Error> {
    if bytes.len() > limits.encoded_bytes {
        return Err(Error::Limit("encoded bytes"));
    }
    let dimensions = container(bytes, limits.encoded_bytes, 4096)?;
    super::dimensions(dimensions.0, dimensions.1, limits)?;
    decode(bytes, dimensions, limits)
}

fn decode(bytes: &[u8], expected: (u32, u32), limits: Limits) -> Result<Pixels, Error> {
    // The closed header walk bounds dimensions before native decoder construction.
    // The provider's logical memory setting is additional to our pixel-buffer cap;
    // its internal allocations are not an operating-system memory guarantee.
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).map_err(failure)?;
    decoder.set_memory_limit(
        usize::try_from(limits.decoded_bytes).map_err(|_| Error::Limit("WebP allocation"))?,
    );
    if decoder.is_animated() || decoder.dimensions() != expected {
        return Err(Error::Decode);
    }
    let alpha = decoder.has_alpha();
    let mut pixels = super::pixel_buffer(decoder.output_buffer_size(), limits)?;
    decoder.read_image(&mut pixels).map_err(failure)?;
    Ok(Pixels {
        width: expected.0,
        height: expected.1,
        alpha,
        bytes: pixels,
    })
}

pub(super) fn metadata(bytes: &[u8], limits: MetadataLimits) -> Result<Metadata, Error> {
    if bytes.len() > limits.encoded_bytes {
        return Err(Error::Limit("metadata encoded bytes"));
    }
    let dimensions = container(bytes, limits.chunk_bytes, limits.chunks)?;
    let pixels = decode(
        bytes,
        dimensions,
        Limits {
            encoded_bytes: limits.encoded_bytes,
            decoded_bytes: limits.decoded_bytes as u64,
            width: 16_384,
            height: 16_384,
        },
    )?;
    let result = Metadata {
        format: Format::Webp,
        bytes: bytes.len(),
        sha256: Sha256Digest::of(bytes),
        tool: METADATA_PROFILE,
        decoded_bytes: pixels.bytes.len(),
        fields: Vec::new(),
    };
    result.validate(Format::Webp, bytes, limits)?;
    Ok(result)
}

fn container(bytes: &[u8], chunk_limit: usize, chunks_limit: usize) -> Result<(u32, u32), Error> {
    if bytes.len() < 12
        || &bytes[..4] != b"RIFF"
        || &bytes[8..12] != b"WEBP"
        || u64::from(u32::from_le_bytes(
            bytes[4..8].try_into().map_err(|_| Error::Decode)?,
        )) + 8
            != bytes.len() as u64
    {
        return Err(Error::Decode);
    }
    let mut cursor = 12usize;
    let mut count = 0usize;
    let mut extended = None;
    let mut alpha = false;
    let mut image = None;
    while cursor < bytes.len() {
        count += 1;
        if count > chunks_limit {
            return Err(Error::Limit("metadata chunks"));
        }
        let header = take(bytes, &mut cursor, 8)?;
        let length =
            u32::from_le_bytes(header[4..8].try_into().map_err(|_| Error::Decode)?) as usize;
        if length > chunk_limit {
            return Err(Error::Limit("metadata chunk bytes"));
        }
        let payload = take(bytes, &mut cursor, length)?;
        if length & 1 != 0 && take(bytes, &mut cursor, 1)? != [0] {
            return Err(Error::Decode);
        }
        match &header[..4] {
            b"VP8X" => {
                if count != 1
                    || length != 10
                    || payload[0] & 0xc1 != 0
                    || payload[1..4] != [0, 0, 0]
                {
                    return Err(Error::Decode);
                }
                if payload[0] & 0x2e != 0 {
                    return Err(Error::Unsupported("WebP animation or metadata"));
                }
                let width = u24(&payload[4..7]) + 1;
                let height = u24(&payload[7..10]) + 1;
                extended = Some((width, height, payload[0] & 0x10 != 0));
            }
            b"ALPH" => {
                if image.is_some()
                    || alpha
                    || !extended.is_some_and(|(_, _, a)| a)
                    || payload.is_empty()
                {
                    return Err(Error::Decode);
                }
                alpha = true;
            }
            b"VP8 " | b"VP8L" => {
                if image.is_some() {
                    return Err(Error::Decode);
                }
                let dimensions = if &header[..4] == b"VP8 " {
                    if payload.len() < 10
                        || payload[0] & 1 != 0
                        || payload[3..6] != [0x9d, 1, 0x2a]
                        || extended.is_some_and(|(_, _, a)| a != alpha)
                    {
                        return Err(Error::Decode);
                    }
                    (
                        u32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff),
                        u32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff),
                    )
                } else {
                    if payload.len() < 5 || payload[0] != 0x2f || payload[4] & 0xe0 != 0 || alpha {
                        return Err(Error::Decode);
                    }
                    let bits =
                        u32::from_le_bytes(payload[1..5].try_into().map_err(|_| Error::Decode)?);
                    if extended.is_some_and(|(_, _, a)| a != (bits & (1 << 28) != 0)) {
                        return Err(Error::Decode);
                    }
                    ((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1)
                };
                if dimensions.0 == 0
                    || dimensions.1 == 0
                    || extended.is_some_and(|(w, h, _)| (w, h) != dimensions)
                {
                    return Err(Error::Decode);
                }
                image = Some(dimensions);
            }
            _ => return Err(Error::Unsupported("WebP chunk outside metadata profile")),
        }
    }
    image.ok_or(Error::Decode)
}

fn u24(bytes: &[u8]) -> u32 {
    u32::from(bytes[0]) | (u32::from(bytes[1]) << 8) | (u32::from(bytes[2]) << 16)
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, count: usize) -> Result<&'a [u8], Error> {
    let end = cursor.checked_add(count).ok_or(Error::Decode)?;
    let result = bytes.get(*cursor..end).ok_or(Error::Decode)?;
    *cursor = end;
    Ok(result)
}

fn failure(error: image_webp::DecodingError) -> Error {
    match error {
        image_webp::DecodingError::MemoryLimitExceeded => Error::Limit("WebP decoder allocation"),
        _ => Error::Decode,
    }
}
