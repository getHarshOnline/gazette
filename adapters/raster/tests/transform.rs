//! Real retained-tree image transforms through the resource port and native codecs.

use std::collections::{BTreeMap, BTreeSet};

use gazette_adapter_raster::{Compiler, Decoder};
use gazette_core::{
    raster::{Dimensions, Encoding, Format, Geometry, Resize},
    resource::{Artifact, Error, Limits, Recipe},
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest, TreeInput},
};
use gazette_ports::{raster::RasterDecoder, resource::ResourceCompiler};
use image::{DynamicImage, ImageEncoder, Rgba, RgbaImage};

fn path(value: &str) -> EntryPath {
    value.parse().expect("fixture path")
}

fn capture(bytes: &[u8]) -> CapturedSource {
    let files = BTreeMap::from([
        (path("image"), bytes.to_vec()),
        (path("companion"), b"retained, not consumed".to_vec()),
    ]);
    let mut builder = InventoryBuilder::new(Default::default());
    for (path, bytes) in &files {
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("inventory");
    }
    CapturedSource::from_parts(builder.finish(), files).expect("complete capture")
}

fn pixels() -> RgbaImage {
    RgbaImage::from_fn(4, 2, |x, y| {
        Rgba([
            u8::try_from(x * 60).expect("red"),
            u8::try_from(y * 200).expect("green"),
            40,
            255,
        ])
    })
}

fn png(pixels: &RgbaImage) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .expect("PNG");
    bytes
}

fn jpeg(pixels: &RgbaImage) -> Vec<u8> {
    let pixels = DynamicImage::ImageRgba8(pixels.clone()).into_rgb8();
    let mut bytes = Vec::new();
    jpeg_encoder::Encoder::new(&mut bytes, 100)
        .encode(
            pixels.as_raw(),
            u16::try_from(pixels.width()).expect("JPEG width"),
            u16::try_from(pixels.height()).expect("JPEG height"),
            jpeg_encoder::ColorType::Rgb,
        )
        .expect("JPEG");
    bytes
}

fn recipe(input_format: Format, resize: Resize, encoding: Encoding) -> Recipe {
    Recipe::RasterImage {
        entry: path("image"),
        input_format,
        resize,
        encoding,
        apply_orientation: true,
    }
}

fn compile(captured: &CapturedSource, recipe: &Recipe, limits: Limits) -> Result<Artifact, Error> {
    let files = captured
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| {
            captured
                .file(&entry.path)
                .map(|bytes| (entry.path.clone(), bytes))
        })
        .collect();
    let input = TreeInput::new(captured.inventory(), files).expect("verified tree");
    let compiler: &dyn ResourceCompiler = &Compiler;
    compiler.compile(&input, recipe, &Default::default(), limits)
}

fn output(bytes: &[u8]) -> RgbaImage {
    if bytes.starts_with(b"RIFF") {
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).unwrap();
        let (width, height) = decoder.dimensions();
        let alpha = decoder.has_alpha();
        let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
        decoder.read_image(&mut pixels).unwrap();
        return if alpha {
            RgbaImage::from_raw(width, height, pixels).unwrap()
        } else {
            DynamicImage::ImageRgb8(image::RgbImage::from_raw(width, height, pixels).unwrap())
                .into_rgba8()
        };
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        let options = zune_core::options::DecoderOptions::new_safe().set_strict_mode(true);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(
            zune_core::bytestream::ZCursor::new(bytes),
            options,
        );
        let pixels = decoder.decode().expect("qualified JPEG decoding");
        let info = decoder.info().expect("JPEG dimensions");
        return DynamicImage::ImageRgb8(
            image::RgbImage::from_raw(u32::from(info.width), u32::from(info.height), pixels)
                .expect("JPEG pixels"),
        )
        .into_rgba8();
    }
    image::load_from_memory(bytes)
        .expect("encoded output pixels")
        .into_rgba8()
}

#[test]
fn lossless_webp_preserves_every_channel_including_transparent_pixels() {
    let original = RgbaImage::from_fn(32, 24, |x, y| {
        Rgba([
            u8::try_from((x * 7 + y * 13) % 256).unwrap(),
            u8::try_from((x * 17 + y * 3) % 256).unwrap(),
            u8::try_from((x * 11 + y * 19) % 256).unwrap(),
            if y % 3 == 0 {
                0
            } else {
                u8::try_from((x * 5 + y * 11) % 256).unwrap()
            },
        ])
    });
    let captured = capture(&png(&original));
    let selected = recipe(
        Format::Png,
        Resize::Fit {
            width: 32,
            height: 24,
        },
        Encoding::Webp,
    );
    let result = compile(&captured, &selected, Limits::default()).unwrap();
    assert_eq!(output(&result.bytes), original);
    assert_eq!(selected.kind(), gazette_core::resource::Kind::WebpImage);
    assert_eq!(
        result.image.unwrap().output,
        Dimensions {
            width: 32,
            height: 24
        }
    );
    let repeated = compile(&captured, &selected, Limits::default()).unwrap();
    assert_eq!(result.bytes, repeated.bytes);
    assert!(matches!(
        compile(
            &captured,
            &selected,
            Limits {
                output_bytes: result.bytes.len() - 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit(_))
    ));
    let resized = compile(
        &captured,
        &recipe(Format::Png, Resize::FitWidth { width: 16 }, Encoding::Webp),
        Limits::default(),
    )
    .unwrap();
    assert_eq!(output(&resized.bytes).dimensions(), (16, 12));
}

#[test]
fn all_resize_modes_decode_both_formats_and_bind_complete_retained_tree() {
    let cases = [
        (
            Resize::Scale {
                width: 3,
                height: 5,
            },
            (3, 5),
        ),
        (Resize::FitWidth { width: 6 }, (6, 3)),
        (Resize::FitHeight { height: 3 }, (6, 3)),
        (
            Resize::Fit {
                width: 3,
                height: 3,
            },
            (3, 2),
        ),
        (
            Resize::Fit {
                width: 20,
                height: 20,
            },
            (4, 2),
        ),
        (
            Resize::Fill {
                width: 3,
                height: 3,
            },
            (3, 3),
        ),
    ];
    for (format, encoded) in [
        (Format::Png, png(&pixels())),
        (Format::Jpeg, jpeg(&pixels())),
    ] {
        let captured = capture(&encoded);
        for (resize, expected) in cases {
            let selected = recipe(format, resize, Encoding::Png);
            let artifact = compile(&captured, &selected, Limits::default()).expect("transform");
            assert_eq!(output(&artifact.bytes).dimensions(), expected);
            assert_eq!(
                artifact.image,
                Some(Geometry {
                    source: Dimensions {
                        width: 4,
                        height: 2
                    },
                    oriented: Dimensions {
                        width: 4,
                        height: 2
                    },
                    output: Dimensions {
                        width: expected.0,
                        height: expected.1
                    },
                })
            );
            assert_eq!(artifact.input, captured.inventory().identity());
            assert_eq!(artifact.dependencies, BTreeSet::from([path("image")]));
            assert!(artifact.tool.contains("image/0.25.10;Lanczos3"));
            assert_eq!(
                artifact.bytes,
                compile(&captured, &selected, Limits::default())
                    .expect("repeat")
                    .bytes
            );
            assert_eq!(captured.file(&path("image")), Some(encoded.as_slice()));
            let inspected = Decoder
                .inspect(Format::Png, &artifact.bytes, Default::default())
                .expect("complete output decoding");
            assert_eq!((inspected.width, inspected.height), expected);
        }
    }
}

#[test]
fn fill_is_centered_and_unchanged_scale_preserves_opaque_pixels() {
    let original = pixels();
    let captured = capture(&png(&original));
    let scale = compile(
        &captured,
        &recipe(
            Format::Png,
            Resize::Scale {
                width: 4,
                height: 2,
            },
            Encoding::Png,
        ),
        Limits::default(),
    )
    .expect("scale");
    assert_eq!(output(&scale.bytes), original);
    let fill = compile(
        &captured,
        &recipe(
            Format::Png,
            Resize::Fill {
                width: 2,
                height: 2,
            },
            Encoding::Png,
        ),
        Limits::default(),
    )
    .expect("fill");
    assert_eq!(
        fill.image.expect("transformed geometry").output,
        Dimensions {
            width: 2,
            height: 2
        }
    );
    assert_eq!(
        output(&fill.bytes),
        image::imageops::crop_imm(&original, 1, 0, 2, 2).to_image()
    );
}

#[test]
fn transparent_colors_do_not_bleed_and_jpeg_composites_explicit_background() {
    let original = RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 {
            Rgba([255, 0, 0, 255])
        } else {
            Rgba([0, 0, 255, 0])
        }
    });
    let captured = capture(&png(&original));
    let result = compile(
        &captured,
        &recipe(
            Format::Png,
            Resize::Scale {
                width: 8,
                height: 1,
            },
            Encoding::Png,
        ),
        Limits::default(),
    )
    .expect("alpha resize");
    let actual = output(&result.bytes);
    assert!(actual.pixels().any(|p| p[3] > 0 && p[3] < 255));
    for pixel in actual.pixels().filter(|pixel| pixel[3] != 0) {
        assert_eq!(pixel[2], 0, "hidden blue must not bleed");
    }
    let transparent = capture(&png(&RgbaImage::from_pixel(8, 8, Rgba([250, 0, 0, 0]))));
    let result = compile(
        &transparent,
        &recipe(
            Format::Png,
            Resize::Scale {
                width: 8,
                height: 8,
            },
            Encoding::Jpeg {
                quality: 100,
                background: [20, 180, 60],
            },
        ),
        Limits::default(),
    )
    .expect("JPEG alpha composition");
    assert_eq!(
        result.image,
        Some(Geometry {
            source: Dimensions {
                width: 8,
                height: 8
            },
            oriented: Dimensions {
                width: 8,
                height: 8
            },
            output: Dimensions {
                width: 8,
                height: 8
            },
        })
    );
    let inspected = Decoder
        .inspect(Format::Jpeg, &result.bytes, Default::default())
        .expect("complete JPEG");
    assert_eq!((inspected.width, inspected.height), (8, 8));
    for pixel in output(&result.bytes).pixels() {
        for (actual, expected) in pixel.0[..3].iter().zip([20u8, 180, 60]) {
            assert!(actual.abs_diff(expected) <= 3);
        }
    }
}

#[test]
fn jpeg_orientation_is_explicit_applied_before_resize_and_not_reemitted() {
    let original = jpeg(&pixels());
    // One little-endian TIFF SHORT orientation entry (6 = rotate clockwise).
    let tiff = [
        b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut tagged = original[..2].to_vec();
    tagged.extend_from_slice(&[0xff, 0xe1]);
    tagged.extend_from_slice(
        &u16::try_from(tiff.len() + 8)
            .expect("EXIF length")
            .to_be_bytes(),
    );
    tagged.extend_from_slice(b"Exif\0\0");
    tagged.extend_from_slice(&tiff);
    tagged.extend_from_slice(&original[2..]);
    let captured = capture(&tagged);
    let mut selected = recipe(
        Format::Jpeg,
        Resize::Fit {
            width: 10,
            height: 10,
        },
        Encoding::Png,
    );
    let result = compile(&captured, &selected, Limits::default()).expect("oriented");
    assert_eq!(
        result.image,
        Some(Geometry {
            source: Dimensions {
                width: 4,
                height: 2
            },
            oriented: Dimensions {
                width: 2,
                height: 4
            },
            output: Dimensions {
                width: 2,
                height: 4
            },
        })
    );
    let inspected = Decoder
        .inspect(Format::Png, &result.bytes, Default::default())
        .expect("oriented output decode");
    assert_eq!((inspected.width, inspected.height), (2, 4));
    assert_eq!(
        output(&result.bytes),
        image::imageops::rotate90(&output(&original))
    );
    let resized = compile(
        &captured,
        &recipe(Format::Jpeg, Resize::FitWidth { width: 3 }, Encoding::Png),
        Limits::default(),
    )
    .expect("resize follows selected orientation");
    assert_eq!(
        resized.image,
        Some(Geometry {
            source: Dimensions {
                width: 4,
                height: 2
            },
            oriented: Dimensions {
                width: 2,
                height: 4
            },
            output: Dimensions {
                width: 3,
                height: 6
            },
        })
    );
    assert_eq!(output(&resized.bytes).dimensions(), (3, 6));
    if let Recipe::RasterImage {
        apply_orientation, ..
    } = &mut selected
    {
        *apply_orientation = false;
    }
    let result = compile(&captured, &selected, Limits::default()).expect("stored orientation");
    assert_eq!(
        result.image,
        Some(Geometry {
            source: Dimensions {
                width: 4,
                height: 2
            },
            oriented: Dimensions {
                width: 4,
                height: 2
            },
            output: Dimensions {
                width: 4,
                height: 2
            },
        })
    );
    assert_eq!(output(&result.bytes), output(&original));
    let mut limited = Limits::default();
    limited.raster.height = 2;
    assert!(
        compile(
            &captured,
            &recipe(
                Format::Jpeg,
                Resize::Scale {
                    width: 1,
                    height: 1
                },
                Encoding::Png
            ),
            limited
        )
        .is_err(),
        "oriented intermediate exceeds height even when final output fits"
    );
}

#[test]
fn invalid_dimensions_quality_formats_and_missing_sources_are_refused() {
    let captured = capture(&png(&pixels()));
    for resize in [
        Resize::Scale {
            width: 0,
            height: 1,
        },
        Resize::FitWidth { width: 0 },
        Resize::FitHeight { height: 0 },
        Resize::Fit {
            width: 0,
            height: 1,
        },
        Resize::Fill {
            width: 0,
            height: 1,
        },
    ] {
        assert!(
            compile(
                &captured,
                &recipe(Format::Png, resize, Encoding::Png),
                Limits::default()
            )
            .is_err()
        );
    }
    for quality in [0, 101, 255] {
        assert!(
            compile(
                &captured,
                &recipe(
                    Format::Png,
                    Resize::Scale {
                        width: 1,
                        height: 1
                    },
                    Encoding::Jpeg {
                        quality,
                        background: [0; 3]
                    }
                ),
                Limits::default()
            )
            .is_err()
        );
    }
    assert!(
        compile(
            &captured,
            &recipe(
                Format::Jpeg,
                Resize::Scale {
                    width: 1,
                    height: 1
                },
                Encoding::Png
            ),
            Limits::default()
        )
        .is_err()
    );
    let mut missing = recipe(
        Format::Png,
        Resize::Scale {
            width: 1,
            height: 1,
        },
        Encoding::Png,
    );
    if let Recipe::RasterImage { entry, .. } = &mut missing {
        *entry = path("missing");
    }
    assert!(matches!(
        compile(&captured, &missing, Limits::default()),
        Err(Error::Input { .. })
    ));
    assert!(
        compile(
            &captured,
            &Recipe::Stylesheet {
                entry: path("image")
            },
            Limits::default()
        )
        .is_err()
    );
}

#[test]
fn malformed_and_truncated_inputs_never_return_partial_artifacts() {
    let mut corrupt = png(&pixels());
    let middle = corrupt.len() / 2;
    corrupt[middle] ^= 0x80;
    for (format, bytes) in [
        (Format::Png, corrupt),
        (Format::Png, png(&pixels())[..25].to_vec()),
        (Format::Jpeg, jpeg(&pixels())[..30].to_vec()),
    ] {
        assert!(
            compile(
                &capture(&bytes),
                &recipe(
                    format,
                    Resize::Scale {
                        width: 1,
                        height: 1
                    },
                    Encoding::Png
                ),
                Limits::default()
            )
            .is_err()
        );
    }
}

#[test]
fn encoded_output_limit_is_exact_and_complete_unused_input_is_bounded() {
    let captured = capture(&png(&pixels()));
    for encoding in [
        Encoding::Png,
        Encoding::Jpeg {
            quality: 90,
            background: [255; 3],
        },
    ] {
        let selected = recipe(
            Format::Png,
            Resize::Scale {
                width: 2,
                height: 1,
            },
            encoding,
        );
        let expected = compile(&captured, &selected, Limits::default())
            .expect("reference")
            .bytes;
        let mut limits = Limits {
            output_bytes: expected.len(),
            ..Limits::default()
        };
        limits.raster.encoded_bytes = captured.file(&path("image")).expect("source").len();
        if matches!(encoding, Encoding::Jpeg { .. }) {
            assert!(
                expected.len() > limits.raster.encoded_bytes,
                "output verification must not reuse the smaller source transport ceiling"
            );
        }
        assert_eq!(
            compile(&captured, &selected, limits)
                .expect("exact limit")
                .bytes,
            expected
        );
        limits.output_bytes -= 1;
        assert!(matches!(
            compile(&captured, &selected, limits),
            Err(Error::Limit(_))
        ));
        limits = Limits::default();
        limits.inputs.entries = 1;
        assert!(matches!(
            compile(&captured, &selected, limits),
            Err(Error::Limit(_))
        ));
        limits = Limits::default();
        limits.raster.encoded_bytes = 1;
        assert!(matches!(
            compile(&captured, &selected, limits),
            Err(Error::Limit(_))
        ));
    }
}

#[test]
fn source_output_fill_and_native_vertical_scratch_have_independent_bounds() {
    let captured = capture(&png(&pixels()));
    let mut limits = Limits::default();
    limits.raster.decoded_bytes = 127;
    assert!(matches!(
        compile(
            &captured,
            &recipe(
                Format::Png,
                Resize::Scale {
                    width: 1,
                    height: 1
                },
                Encoding::Png
            ),
            limits
        ),
        Err(Error::Limit(_))
    ));
    limits.raster.decoded_bytes = 128;
    compile(
        &captured,
        &recipe(
            Format::Png,
            Resize::Scale {
                width: 4,
                height: 2,
            },
            Encoding::Png,
        ),
        limits,
    )
    .expect("exact RGBA32F source limit");
    limits.raster.decoded_bytes = 256;
    // Input 4x2 and output 1x8 fit 256, but native vertical scratch 4x8 needs 512.
    assert!(matches!(
        compile(
            &captured,
            &recipe(
                Format::Png,
                Resize::Scale {
                    width: 1,
                    height: 8
                },
                Encoding::Png
            ),
            limits
        ),
        Err(Error::Limit(_))
    ));
    limits.raster.decoded_bytes = 512;
    compile(
        &captured,
        &recipe(
            Format::Png,
            Resize::Scale {
                width: 1,
                height: 8,
            },
            Encoding::Png,
        ),
        limits,
    )
    .expect("exact scratch bound");
    limits = Limits::default();
    limits.raster.width = 5;
    // Center-fill would first scale 4x2 to 6x3, although final 3x3 fits.
    assert!(matches!(
        compile(
            &captured,
            &recipe(
                Format::Png,
                Resize::Fill {
                    width: 3,
                    height: 3
                },
                Encoding::Png
            ),
            limits
        ),
        Err(Error::Limit(_))
    ));
    limits.raster.width = 4;
    assert!(matches!(
        compile(
            &captured,
            &recipe(
                Format::Png,
                Resize::Scale {
                    width: 5,
                    height: 1
                },
                Encoding::Png
            ),
            limits
        ),
        Err(Error::Limit(_))
    ));
}
