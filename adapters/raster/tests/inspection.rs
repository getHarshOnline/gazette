//! Real retained-byte decoding, identity and rejection behavior at the raster port.

use base64::Engine;
use gazette_adapter_raster::Decoder;
use gazette_core::{
    raster::{Error, Format, Limits},
    source::Sha256Digest,
};
use gazette_ports::raster::RasterDecoder;

fn png(animated: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        if animated {
            encoder.set_animated(1, 0).expect("animation declaration");
        }
        let mut writer = encoder.write_header().expect("PNG header");
        writer
            .write_image_data(&[255, 0, 0, 0, 255, 0])
            .expect("PNG pixels");
        writer.finish().expect("PNG end");
    }
    bytes
}

fn jpeg() -> Vec<u8> {
    // Native image 0.25.10 encoder, quality 90, two RGB pixels: red and green.
    // Retain the generated fixture without carrying its unused decoder dependency.
    base64::engine::general_purpose::STANDARD
        .decode("/9j/4AAQSkZJRgABAgAAAQABAAD/wAARCAABAAIDAREAAhEBAxEB/9sAQwADAgIDAgIDAwMDBAMDBAUIBQUEBAUKBwcGCAwKDAwLCgsLDQ4SEA0OEQ4LCxAWEBETFBUVFQwPFxgWFBgSFBUU/9sAQwEDBAQFBAUJBQUJFA0LDRQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQU/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDovBn/ACJ+hf8AXhB/6LWv4tx3+91f8UvzZ/GHij/yXuf/APYZif8A09M//9k=")
        .expect("fixed JPEG fixture")
}

#[test]
fn actual_decoding_preserves_identity_and_is_deterministic() {
    for (format, bytes) in [(Format::Png, png(false)), (Format::Jpeg, jpeg())] {
        let before = bytes.clone();
        let result = Decoder
            .inspect(format, &bytes, Limits::default())
            .expect("decode pixels");
        assert_eq!((result.width, result.height), (2, 1));
        assert_eq!(
            result.display,
            Some(gazette_core::raster::Dimensions {
                width: 2,
                height: 1
            })
        );
        assert_eq!(result.format, format);
        assert_eq!(result.bytes, bytes.len());
        assert_eq!(result.sha256, Sha256Digest::of(&bytes));
        assert_eq!(
            result,
            Decoder
                .inspect(format, &bytes, Limits::default())
                .expect("repeat")
        );
        assert_eq!(bytes, before);
        assert!(result.tool.contains(match format {
            Format::Png => "png/0.18.0",
            Format::Jpeg => "zune-jpeg/0.5.16-rc2",
            Format::Webp => "image-webp/0.2.4",
        }));
    }
}

#[test]
fn explicit_format_is_not_guessed_or_replaced() {
    assert!(matches!(
        Decoder.inspect(Format::Jpeg, &png(false), Limits::default()),
        Err(Error::Decode)
    ));
    assert!(matches!(
        Decoder.inspect(Format::Png, &jpeg(), Limits::default()),
        Err(Error::Decode)
    ));
}

#[test]
fn encoded_dimensions_and_decoded_bytes_are_bounded() {
    for (format, bytes) in [(Format::Png, png(false)), (Format::Jpeg, jpeg())] {
        for limits in [
            Limits {
                encoded_bytes: bytes.len() - 1,
                ..Limits::default()
            },
            Limits {
                decoded_bytes: 5,
                ..Limits::default()
            },
        ] {
            assert!(
                matches!(
                    Decoder.inspect(format, &bytes, limits),
                    Err(Error::Limit(_))
                ),
                "{format:?} must reject exhausted limits"
            );
        }
        for limits in [
            Limits {
                width: 1,
                ..Limits::default()
            },
            Limits {
                height: 0,
                ..Limits::default()
            },
        ] {
            // zune's configured dimension ceiling is reported as a generic
            // format failure; do not classify by parsing decoder prose.
            assert!(Decoder.inspect(format, &bytes, limits).is_err());
        }
        let exact_encoded = Limits {
            encoded_bytes: bytes.len(),
            ..Limits::default()
        };
        assert!(Decoder.inspect(format, &bytes, exact_encoded).is_ok());
    }
    // The encoded ceiling is checked before an invalid header can be parsed.
    assert!(matches!(
        Decoder.inspect(
            Format::Png,
            b"not an image",
            Limits {
                encoded_bytes: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("encoded bytes"))
    ));
}

#[test]
fn valid_headers_cannot_hide_missing_or_corrupt_pixels() {
    let bytes = png(false);
    let idat = bytes
        .windows(4)
        .position(|window| window == b"IDAT")
        .expect("IDAT chunk");
    assert!(
        Decoder
            .inspect(Format::Png, &bytes[..idat + 4], Limits::default())
            .is_err()
    );
    let mut corrupted = bytes;
    corrupted[idat + 6] ^= 0xff;
    assert!(matches!(
        Decoder.inspect(Format::Png, &corrupted, Limits::default()),
        Err(Error::Decode)
    ));

    let bytes = jpeg();
    let scan = bytes
        .windows(2)
        .position(|window| window == [0xff, 0xda])
        .expect("start of scan");
    let header_length = usize::from(u16::from_be_bytes([bytes[scan + 2], bytes[scan + 3]]));
    let pixels = scan + 2 + header_length;
    assert!(
        Decoder
            .inspect(Format::Jpeg, &bytes[..pixels], Limits::default())
            .is_err()
    );
    assert!(
        Decoder
            .inspect(Format::Jpeg, &bytes[..pixels + 1], Limits::default())
            .is_err()
    );
}

#[test]
fn png_end_chunk_is_read_and_checksum_checked() {
    let bytes = png(false);
    assert!(
        Decoder
            .inspect(Format::Png, &bytes[..bytes.len() - 12], Limits::default())
            .is_err()
    );
    let mut corrupted = bytes;
    *corrupted.last_mut().expect("IEND checksum") ^= 1;
    assert!(matches!(
        Decoder.inspect(Format::Png, &corrupted, Limits::default()),
        Err(Error::Decode)
    ));
}

#[test]
fn animation_is_not_silently_reduced_to_default_frame() {
    assert!(matches!(
        Decoder.inspect(Format::Png, &png(true), Limits::default()),
        Err(Error::Unsupported("animated PNG"))
    ));
}

#[test]
fn decoder_errors_do_not_echo_raw_bytes_or_metadata() {
    let secret = b"private native source /Users/private.jpg and metadata";
    for format in [Format::Png, Format::Jpeg] {
        let error = Decoder
            .inspect(format, secret, Limits::default())
            .expect_err("invalid source");
        assert_eq!(error.code(), "raster_decode");
        assert!(!error.to_string().contains("private"));
        assert!(error.to_string().len() < 120);
    }
}

#[test]
fn display_extent_tracks_exif_without_changing_stored_dimensions() {
    for (orientation, expected) in [
        (1, Some((2, 1))),
        (2, Some((2, 1))),
        (5, Some((1, 2))),
        (6, Some((1, 2))),
        (7, Some((1, 2))),
        (8, Some((1, 2))),
        (9, None),
    ] {
        let mut exif = vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        ];
        exif[18] = orientation;
        let source = jpeg();
        let mut jpg = vec![0xff, 0xd8, 0xff, 0xe1, 0, 34];
        jpg.extend_from_slice(b"Exif\0\0");
        jpg.extend_from_slice(&exif);
        jpg.extend_from_slice(&source[2..]);
        let mut png_bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().expect("header");
            writer
                .write_chunk(png::chunk::ChunkType(*b"eXIf"), &exif)
                .expect("EXIF");
            writer
                .write_image_data(&[255, 0, 0, 0, 255, 0])
                .expect("pixels");
            writer.finish().expect("end");
        }
        for (format, bytes) in [(Format::Jpeg, jpg), (Format::Png, png_bytes)] {
            let result = Decoder
                .inspect(format, &bytes, Limits::default())
                .expect("decode");
            assert_eq!((result.width, result.height), (2, 1));
            assert_eq!(
                result.display.map(|size| (size.width, size.height)),
                expected,
                "{format:?} orientation {orientation}"
            );
            result
                .validate(format, &bytes, Limits::default())
                .expect("valid evidence");
        }
    }
}
