use std::io::Cursor;

use gazette_core::raster::{Error, Limits};

pub(super) fn decode(
    bytes: &[u8],
    limits: Limits,
) -> Result<(u32, u32, Option<gazette_core::raster::Dimensions>), Error> {
    let allocation_limit = usize::try_from(limits.decoded_bytes).unwrap_or(usize::MAX);
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: allocation_limit,
        },
    );
    // Expand packed/indexed pixels without reducing sixteen-bit samples. The
    // original bytes, not this temporary decoded representation, are emitted.
    decoder.set_transformations(png::Transformations::EXPAND);
    decoder.ignore_checksums(false);
    let header = decoder.read_header_info().map_err(failure)?;
    let dimensions = (header.width, header.height);
    super::dimensions(dimensions.0, dimensions.1, limits)?;
    let mut reader = decoder.read_info().map_err(failure)?;
    if reader.info().animation_control.is_some() {
        return Err(Error::Unsupported("animated PNG"));
    }
    let mut pixels = super::pixel_buffer(reader.output_buffer_size(), limits)?;
    let output = reader.next_frame(&mut pixels).map_err(failure)?;
    if (output.width, output.height) != dimensions || output.buffer_size() != pixels.len() {
        return Err(Error::Decode);
    }
    reader.finish().map_err(failure)?;
    let orientation = match reader.info().exif_metadata.as_deref() {
        Some(bytes) => image::metadata::Orientation::from_exif_chunk(bytes),
        None => Some(image::metadata::Orientation::NoTransforms),
    };
    let display = orientation.map(|orientation| {
        let (width, height) = super::oriented(dimensions.0, dimensions.1, orientation);
        gazette_core::raster::Dimensions { width, height }
    });
    Ok((dimensions.0, dimensions.1, display))
}

fn failure(error: png::DecodingError) -> Error {
    match error {
        png::DecodingError::LimitsExceeded => Error::Limit("PNG decoder allocation"),
        _ => Error::Decode,
    }
}
