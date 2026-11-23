//! Fixed Lanczos3 transforms over checked retained bytes. Pixel and intermediate-buffer
//! ceilings are logical bounds, not a process-memory sandbox. Sampling uses encoded
//! channel values with premultiplied alpha; this profile is not color-managed rendering.

use std::{collections::BTreeSet, io::Cursor};

use gazette_core::{
    raster::{self, Dimensions, Encoding, Format, Geometry, Resize},
    render::LimitedWriter,
    resource::{Artifact, Error, Limits, Recipe},
    source::TreeInput,
};
use gazette_ports::{raster::RasterDecoder, resource::ResourceCompiler};
use image::{
    DynamicImage, ImageDecoder, ImageEncoder, ImageReader, imageops, metadata::Orientation,
};

/// Deterministic PNG/JPEG/WebP compilation without filesystem access or metadata preservation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Compiler;

impl ResourceCompiler for Compiler {
    fn compile(
        &self,
        source: &TreeInput<'_>,
        recipe: &Recipe,
        links: &gazette_core::resource::ModuleLinks,
        limits: Limits,
    ) -> Result<Artifact, Error> {
        let Recipe::RasterImage {
            entry,
            input_format,
            resize,
            encoding,
            apply_orientation,
        } = recipe
        else {
            return Err(Error::Input {
                path: recipe.entry().clone(),
                reason: "raster compiler requires an image recipe",
            });
        };
        if !links.is_empty() {
            return Err(Error::Input {
                path: recipe.entry().clone(),
                reason: "image recipes do not accept module links",
            });
        }
        limits.validate_input(source.inventory())?;
        if let Encoding::Jpeg { quality, .. } = encoding
            && !(1..=100).contains(quality)
        {
            return Err(Error::Compile(
                "JPEG quality must be between 1 and 100".into(),
            ));
        }
        let bytes = source.file(entry).ok_or_else(|| Error::Input {
            path: entry.clone(),
            reason: "image entry must be a retained regular file",
        })?;
        let (mut decoded, stored_orientation) = decode(*input_format, bytes, limits.raster)?;
        let source_dimensions = Dimensions {
            width: decoded.width(),
            height: decoded.height(),
        };
        buffer(decoded.width(), decoded.height(), limits.raster)?;
        let orientation = if *apply_orientation {
            stored_orientation
        } else {
            Orientation::NoTransforms
        };
        let (width, height) = super::oriented(decoded.width(), decoded.height(), orientation);
        buffer(width, height, limits.raster)?;
        let size = dimensions(width, height, *resize)?;
        buffer(size.0, size.1, limits.raster)?;
        // image 0.25.10 Lanczos3 first creates a width-by-new_height RGBA32F
        // vertical sample buffer, before its horizontal pass. Bound that exact shape.
        if size != (width, height) {
            buffer(width, size.1, limits.raster)?;
        }
        if let Resize::Fill { width, height } = resize {
            buffer(*width, *height, limits.raster)?;
        }
        decoded.apply_orientation(orientation);
        let oriented_dimensions = Dimensions {
            width: decoded.width(),
            height: decoded.height(),
        };
        if (oriented_dimensions.width, oriented_dimensions.height) != (width, height) {
            return Err(Error::Compile(
                "orientation dimensions differ from bounded plan".into(),
            ));
        }
        // An unchanged lossless rendition preserves every eight-bit channel,
        // including RGB beneath zero alpha. Sampling would discard that information.
        let transformed = if *encoding == Encoding::Webp
            && size == (width, height)
            && !matches!(resize, Resize::Fill { width: w, height: h } if (*w, *h) != size)
        {
            DynamicImage::ImageRgba8(decoded.into_rgba8())
        } else {
            let mut pixels = decoded.into_rgba32f();
            for pixel in pixels.pixels_mut() {
                for channel in 0..3 {
                    pixel[channel] *= pixel[3];
                }
            }
            let mut pixels = if size == (width, height) {
                pixels
            } else {
                imageops::resize(&pixels, size.0, size.1, imageops::FilterType::Lanczos3)
            };
            if let Resize::Fill { width, height } = resize {
                pixels = imageops::crop_imm(
                    &pixels,
                    (size.0 - width) / 2,
                    (size.1 - height) / 2,
                    *width,
                    *height,
                )
                .to_image();
            }
            for pixel in pixels.pixels_mut() {
                let alpha = pixel[3].clamp(0.0, 1.0);
                for channel in 0..3 {
                    let straight = if alpha > 0.0 {
                        (pixel[channel] / alpha).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    pixel[channel] = match encoding {
                        Encoding::Png | Encoding::Webp => straight,
                        Encoding::Jpeg { background, .. } => {
                            straight * alpha
                                + f32::from(background[channel]) / 255.0 * (1.0 - alpha)
                        }
                    };
                }
                pixel[3] = alpha;
            }
            DynamicImage::ImageRgba32F(pixels)
        };
        let output_dimensions = Dimensions {
            width: transformed.width(),
            height: transformed.height(),
        };
        let bytes = encode(transformed, *encoding, limits.output_bytes)?;
        let format = match encoding {
            Encoding::Png => Format::Png,
            Encoding::Webp => Format::Webp,
            Encoding::Jpeg { .. } => Format::Jpeg,
        };
        // Output encoding has its own byte allowance; the same pixel ceilings
        // still apply when independently decoding the complete emitted artifact.
        let output_limits = raster::Limits {
            encoded_bytes: limits.output_bytes,
            ..limits.raster
        };
        let inspected = super::Decoder
            .inspect(format, &bytes, output_limits)
            .map_err(raster_error)?;
        inspected
            .validate(format, &bytes, output_limits)
            .map_err(raster_error)?;
        if (inspected.width, inspected.height)
            != (output_dimensions.width, output_dimensions.height)
        {
            return Err(Error::Compile(
                "encoded image dimensions differ from transformed pixels".into(),
            ));
        }
        Ok(Artifact {
            input: source.inventory().identity(),
            bytes,
            dependencies: BTreeSet::from([entry.clone()]),
            references: BTreeSet::new(),
            deferred_references: Default::default(),
            svg: None,
            image: Some(Geometry {
                source: source_dimensions,
                oriented: oriented_dimensions,
                output: output_dimensions,
            }),
            tool: "gazette.raster-transform/2;image/0.25.10;Lanczos3;zune-jpeg/0.5.16-rc2;strict;jpeg-encoder/0.7.1;image-webp/0.2.4;premultiplied-encoded-rgba32f;png-rgba8/jpeg-rgb8/webp-lossless-rgba8",
        })
    }
}

fn decode(
    format: Format,
    bytes: &[u8],
    limits: raster::Limits,
) -> Result<(DynamicImage, Orientation), Error> {
    if format == Format::Webp {
        let decoded = super::webp::pixels(bytes, limits).map_err(raster_error)?;
        let image = if decoded.alpha {
            DynamicImage::ImageRgba8(
                image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.bytes)
                    .ok_or_else(|| Error::Compile("WebP pixel shape mismatch".into()))?,
            )
        } else {
            DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(decoded.width, decoded.height, decoded.bytes)
                    .ok_or_else(|| Error::Compile("WebP pixel shape mismatch".into()))?,
            )
        };
        return Ok((image, Orientation::NoTransforms));
    }
    if format == Format::Jpeg {
        // One qualified strict decoder owns both inspection and transformed pixels.
        let decoded = super::jpeg::pixels(bytes, limits).map_err(raster_error)?;
        let pixels = image::RgbImage::from_raw(decoded.width, decoded.height, decoded.bytes)
            .ok_or_else(|| Error::Compile("JPEG pixel shape mismatch".into()))?;
        return Ok((DynamicImage::ImageRgb8(pixels), decoded.orientation));
    }
    // Existing PNG admission checks the complete end chunk/checksums and rejects APNG.
    let inspected = super::Decoder
        .inspect(format, bytes, limits)
        .map_err(raster_error)?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let mut decode_limits = image::Limits::default();
    decode_limits.max_image_width = Some(limits.width);
    decode_limits.max_image_height = Some(limits.height);
    decode_limits.max_alloc = Some(limits.decoded_bytes);
    reader.limits(decode_limits);
    let mut decoder = reader.into_decoder().map_err(image_error)?;
    if decoder.dimensions() != (inspected.width, inspected.height) {
        return Err(Error::Compile("PNG decoders disagree on dimensions".into()));
    }
    let orientation = decoder.orientation().map_err(image_error)?;
    Ok((
        DynamicImage::from_decoder(decoder).map_err(image_error)?,
        orientation,
    ))
}

fn buffer(width: u32, height: u32, limits: raster::Limits) -> Result<(), Error> {
    if width == 0 || height == 0 || width > limits.width || height > limits.height {
        return Err(Error::Limit("raster dimensions"));
    }
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(16));
    if bytes.is_none_or(|bytes| bytes > limits.decoded_bytes || usize::try_from(bytes).is_err()) {
        return Err(Error::Limit("raster transform buffer bytes"));
    }
    Ok(())
}

fn proportional(side: u32, selected: u32, original: u32) -> Result<u32, Error> {
    let scaled =
        (u64::from(side) * u64::from(selected) + u64::from(original) / 2) / u64::from(original);
    u32::try_from(scaled.max(1)).map_err(|_| Error::Limit("raster proportional dimensions"))
}

fn dimensions(width: u32, height: u32, resize: Resize) -> Result<(u32, u32), Error> {
    let valid = match resize {
        Resize::Scale { width, height }
        | Resize::Fit { width, height }
        | Resize::Fill { width, height } => width != 0 && height != 0,
        Resize::FitWidth { width } => width != 0,
        Resize::FitHeight { height } => height != 0,
    };
    if !valid {
        return Err(Error::Limit("raster zero dimensions"));
    }
    let result = match resize {
        Resize::Scale { width, height } => (width, height),
        Resize::FitWidth { width: selected } => (selected, proportional(height, selected, width)?),
        Resize::FitHeight { height: selected } => {
            (proportional(width, selected, height)?, selected)
        }
        Resize::Fit {
            width: selected_width,
            height: selected_height,
        } => {
            let selected_width = selected_width.min(width);
            let selected_height = selected_height.min(height);
            if u64::from(selected_width) * u64::from(height)
                <= u64::from(selected_height) * u64::from(width)
            {
                (selected_width, proportional(height, selected_width, width)?)
            } else {
                (
                    proportional(width, selected_height, height)?,
                    selected_height,
                )
            }
        }
        Resize::Fill {
            width: selected_width,
            height: selected_height,
        } => {
            if u64::from(selected_width) * u64::from(height)
                >= u64::from(selected_height) * u64::from(width)
            {
                (selected_width, proportional(height, selected_width, width)?)
            } else {
                (
                    proportional(width, selected_height, height)?,
                    selected_height,
                )
            }
        }
    };
    if result.0 == 0 || result.1 == 0 {
        return Err(Error::Limit("raster zero dimensions"));
    }
    Ok(result)
}

fn encode(image: DynamicImage, encoding: Encoding, maximum: usize) -> Result<Vec<u8>, Error> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    let result = match encoding {
        Encoding::Webp => {
            let pixels = image.into_rgba8();
            image_webp::WebPEncoder::new(&mut output)
                .encode(
                    pixels.as_raw(),
                    pixels.width(),
                    pixels.height(),
                    image_webp::ColorType::Rgba8,
                )
                .map_err(|_| Error::Compile("WebP encoder rejected the raster".into()))
        }
        Encoding::Png => {
            let pixels = image.into_rgba8();
            image::codecs::png::PngEncoder::new(&mut output)
                .write_image(
                    pixels.as_raw(),
                    pixels.width(),
                    pixels.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(image_error)
        }
        Encoding::Jpeg { quality, .. } => {
            let pixels = image.into_rgb8();
            jpeg_encoder::Encoder::new(&mut output, quality)
                .encode(
                    pixels.as_raw(),
                    u16::try_from(pixels.width()).map_err(|_| Error::Limit("JPEG width"))?,
                    u16::try_from(pixels.height()).map_err(|_| Error::Limit("JPEG height"))?,
                    jpeg_encoder::ColorType::Rgb,
                )
                .map_err(|_| Error::Compile("JPEG encoder rejected the raster".into()))
        }
    };
    if output.exceeded() {
        return Err(Error::Limit("raster encoded output bytes"));
    }
    result?;
    Ok(output.into_inner())
}

fn raster_error(error: raster::Error) -> Error {
    match error {
        raster::Error::Limit(reason) => Error::Limit(reason),
        _ => Error::Compile(error.to_string()),
    }
}

fn image_error(error: image::ImageError) -> Error {
    match error {
        image::ImageError::Limits(_) => Error::Limit("image decoder allocation"),
        _ => Error::Compile("image decoder rejected the selected raster".into()),
    }
}
