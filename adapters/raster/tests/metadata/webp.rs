//! Complete native WebP pixels with a closed metadata/container profile.

use gazette_adapter_raster::Decoder;
use gazette_core::raster::{Error, Format, Limits, MetadataLimits};
use gazette_ports::raster::{RasterDecoder, RasterMetadata};

fn image(alpha: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    let (pixels, color) = if alpha {
        (vec![40, 90, 170, 120], image_webp::ColorType::Rgba8)
    } else {
        (vec![40, 90, 170], image_webp::ColorType::Rgb8)
    };
    image_webp::WebPEncoder::new(&mut bytes)
        .encode(&pixels, 1, 1, color)
        .unwrap();
    bytes
}

fn fix_length(bytes: &mut [u8]) {
    let n = u32::try_from(bytes.len() - 8).unwrap();
    bytes[4..8].copy_from_slice(&n.to_le_bytes());
}

fn extended(alpha: bool) -> Vec<u8> {
    let plain = image(alpha);
    let mut bytes = plain[..12].to_vec();
    bytes.extend_from_slice(b"VP8X\x0a\0\0\0");
    bytes.extend_from_slice(&[if alpha { 0x10 } else { 0 }, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&plain[12..]);
    fix_length(&mut bytes);
    bytes
}

#[test]
fn webp_plain_and_extended_alpha_decode_and_bind_without_sanitizing() {
    for bytes in [image(false), image(true), extended(false), extended(true)] {
        let pixels = Decoder
            .inspect(Format::Webp, &bytes, Limits::default())
            .unwrap();
        assert_eq!((pixels.width, pixels.height), (1, 1));
        let metadata = Decoder
            .metadata(Format::Webp, &bytes, MetadataLimits::default())
            .unwrap();
        metadata
            .validate(Format::Webp, &bytes, MetadataLimits::default())
            .unwrap();
        assert!(metadata.fields.is_empty());
        assert!(matches!(metadata.decoded_bytes, 3 | 4));
        assert_eq!(metadata.sha256, pixels.sha256);
    }
}

#[test]
fn webp_metadata_animation_unknown_chunks_and_trailing_data_never_pass() {
    for tag in [*b"EXIF", *b"XMP ", *b"ICCP", *b"ANIM", *b"ANMF", *b"TEST"] {
        let mut bytes = image(false);
        bytes.extend_from_slice(&tag);
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.extend_from_slice(b"SECRET");
        fix_length(&mut bytes);
        assert!(matches!(
            Decoder.metadata(Format::Webp, &bytes, MetadataLimits::default()),
            Err(Error::Unsupported(_))
        ));
    }
    for flag in [0x20, 0x08, 0x04, 0x02] {
        let mut bytes = extended(false);
        bytes[20] |= flag;
        assert!(matches!(
            Decoder.metadata(Format::Webp, &bytes, MetadataLimits::default()),
            Err(Error::Unsupported(_))
        ));
    }
    let mut trailing = image(false);
    trailing.push(0);
    assert!(matches!(
        Decoder.metadata(Format::Webp, &trailing, MetadataLimits::default()),
        Err(Error::Decode)
    ));
}

#[test]
fn webp_closed_structure_and_decoded_work_have_independent_bounds() {
    let bytes = extended(true);
    for limits in [
        MetadataLimits {
            chunks: 0,
            ..Default::default()
        },
        MetadataLimits {
            chunk_bytes: 1,
            ..Default::default()
        },
        MetadataLimits {
            encoded_bytes: bytes.len() - 1,
            ..Default::default()
        },
        MetadataLimits {
            decoded_bytes: 0,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            Decoder.metadata(Format::Webp, &bytes, limits),
            Err(Error::Limit(_))
        ));
    }
    for length in [0, 11, 19, bytes.len() - 1] {
        assert!(
            Decoder
                .metadata(Format::Webp, &bytes[..length], MetadataLimits::default())
                .is_err()
        );
    }
    let mut repeated = image(false);
    let chunk = repeated[12..].to_vec();
    repeated.extend_from_slice(&chunk);
    fix_length(&mut repeated);
    assert!(matches!(
        Decoder.metadata(Format::Webp, &repeated, MetadataLimits::default()),
        Err(Error::Decode)
    ));
    let mut inconsistent = extended(false);
    inconsistent[24] = 1;
    assert!(matches!(
        Decoder.metadata(Format::Webp, &inconsistent, MetadataLimits::default()),
        Err(Error::Decode)
    ));
    let mut reserved = extended(false);
    reserved[21] = 1;
    assert!(matches!(
        Decoder.metadata(Format::Webp, &reserved, MetadataLimits::default()),
        Err(Error::Decode)
    ));
}
