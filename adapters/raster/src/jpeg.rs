use gazette_core::raster::{Error, Limits};
use zune_core::{bytestream::ZCursor, options::DecoderOptions};

pub(super) fn decode(
    bytes: &[u8],
    limits: Limits,
) -> Result<(u32, u32, Option<gazette_core::raster::Dimensions>), Error> {
    let decoded = pixels(bytes, limits)?;
    Ok((decoded.width, decoded.height, decoded.display))
}

pub(super) struct Pixels {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bytes: Vec<u8>,
    pub(super) display: Option<gazette_core::raster::Dimensions>,
    pub(super) orientation: image::metadata::Orientation,
}

pub(super) fn pixels(bytes: &[u8], limits: Limits) -> Result<Pixels, Error> {
    if bytes.len() > limits.encoded_bytes {
        return Err(Error::Limit("encoded bytes"));
    }
    // Limits and strict decoding apply before header parsing. Unlike the image
    // facade, this decoder borrows the supplied bytes instead of copying them.
    let options = DecoderOptions::new_safe()
        .set_strict_mode(true)
        .set_max_width(limits.width as usize)
        .set_max_height(limits.height as usize);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(failure)?;
    decoder.set_options(
        decoder
            .options()
            .jpeg_set_out_colorspace(zune_core::colorspace::ColorSpace::RGB),
    );
    let info = decoder.info().ok_or(Error::Decode)?;
    let dimensions = (u32::from(info.width), u32::from(info.height));
    super::dimensions(dimensions.0, dimensions.1, limits)?;
    let mut pixels = super::pixel_buffer(decoder.output_buffer_size(), limits)?;
    decoder.decode_into(&mut pixels).map_err(failure)?;
    if decoder.decoded_output_bytes() != Some(pixels.len()) {
        return Err(Error::Decode);
    }
    let orientation = match decoder.exif() {
        Some(bytes) => image::metadata::Orientation::from_exif_chunk(bytes),
        None => Some(image::metadata::Orientation::NoTransforms),
    };
    let display = orientation.map(|orientation| {
        let (width, height) = super::oriented(dimensions.0, dimensions.1, orientation);
        gazette_core::raster::Dimensions { width, height }
    });
    Ok(Pixels {
        width: dimensions.0,
        height: dimensions.1,
        bytes: pixels,
        display,
        orientation: orientation.unwrap_or(image::metadata::Orientation::NoTransforms),
    })
}

fn failure(error: zune_jpeg::errors::DecodeErrors) -> Error {
    match error {
        zune_jpeg::errors::DecodeErrors::LargeDimensions(_) => Error::Limit("JPEG dimensions"),
        zune_jpeg::errors::DecodeErrors::Unsupported(_) => Error::Unsupported("JPEG encoding"),
        _ => Error::Decode,
    }
}
