//! Real native JPEG generation reaches the metadata port without rewriting input.

use gazette_adapter_raster::Decoder;
use gazette_core::{
    raster::{Error, Format, MetadataLimits},
    source::Sha256Digest,
};
use gazette_ports::raster::{RasterDecoder, RasterMetadata};

fn image(progressive: bool) -> Vec<u8> {
    let pixels: Vec<u8> = (0..32 * 16 * 3).map(|value| (value % 251) as u8).collect();
    let mut output = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut output, 90);
    encoder.set_progressive(progressive);
    encoder.set_restart_interval(1);
    encoder
        .encode(&pixels, 32, 16, jpeg_encoder::ColorType::Rgb)
        .expect("native JPEG encoding");
    output
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut value = vec![0xff, marker];
    value.extend_from_slice(
        &u16::try_from(payload.len() + 2)
            .expect("fixture length")
            .to_be_bytes(),
    );
    value.extend_from_slice(payload);
    value
}

fn insert(bytes: &mut Vec<u8>, at: usize, marker: u8, payload: &[u8]) {
    bytes.splice(at..at, segment(marker, payload));
}

#[test]
fn baseline_progressive_and_restart_streams_preserve_comments_in_native_order() {
    for progressive in [false, true] {
        let mut bytes = image(progressive);
        insert(&mut bytes, 2, 0xfe, "café\n".as_bytes());
        let end = bytes.len() - 2;
        insert(&mut bytes, end, 0xfe, b"after scan");
        let original = bytes.clone();
        let pixels = Decoder
            .inspect(Format::Jpeg, &bytes, Default::default())
            .expect("real pixels");
        assert_eq!((pixels.width, pixels.height), (32, 16));
        assert!(
            bytes
                .windows(2)
                .any(|pair| pair[0] == 0xff && (0xd0..=0xd7).contains(&pair[1])),
            "native restart markers"
        );
        let report = Decoder
            .metadata(Format::Jpeg, &bytes, Default::default())
            .expect("complete metadata");
        assert_eq!(report.fields, ["café\n", "after scan"]);
        assert_eq!(report.decoded_bytes, 32 * 16 * 3 + "café\nafter scan".len());
        assert_eq!(report.bytes, bytes.len());
        assert_eq!(report.sha256, Sha256Digest::of(&bytes));
        assert_eq!(
            report,
            Decoder
                .metadata(Format::Jpeg, &bytes, Default::default())
                .expect("deterministic repeat")
        );
        assert_eq!(bytes, original);
    }
}

#[test]
fn metadata_limits_cover_markers_values_comments_and_native_pixel_output() {
    let mut bytes = image(false);
    insert(&mut bytes, 2, 0xfe, b"a");
    let report = Decoder
        .metadata(Format::Jpeg, &bytes, Default::default())
        .expect("baseline");
    let exact = MetadataLimits {
        encoded_bytes: bytes.len(),
        decoded_bytes: report.decoded_bytes,
        values: 1,
        ..Default::default()
    };
    Decoder
        .metadata(Format::Jpeg, &bytes, exact)
        .expect("exact work and field bounds");
    for limits in [
        MetadataLimits {
            encoded_bytes: bytes.len() - 1,
            ..exact
        },
        MetadataLimits {
            decoded_bytes: report.decoded_bytes - 1,
            ..exact
        },
        MetadataLimits { values: 0, ..exact },
        MetadataLimits { chunks: 1, ..exact },
        MetadataLimits {
            chunk_bytes: 1,
            ..exact
        },
    ] {
        assert!(
            matches!(
                Decoder.metadata(Format::Jpeg, &bytes, limits),
                Err(Error::Limit(_))
            ),
            "{limits:?}"
        );
    }
}

#[test]
fn ignored_app_containers_and_thumbnails_are_not_empty_metadata_success() {
    for marker in 0xe1..=0xef {
        let mut bytes = image(false);
        insert(&mut bytes, 2, marker, b"private metadata");
        assert!(
            matches!(
                Decoder.metadata(Format::Jpeg, &bytes, Default::default()),
                Err(Error::Unsupported(_))
            ),
            "APP {}",
            marker - 0xe0
        );
    }
    let mut bytes = image(false);
    insert(
        &mut bytes,
        2,
        0xe0,
        b"JFIF\0\x01\x02\0\0\x01\0\x01\x01\x01\x01\x02\x03",
    );
    assert!(matches!(
        Decoder.metadata(Format::Jpeg, &bytes, Default::default()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn closure_and_native_decode_failures_cannot_produce_partial_evidence() {
    let bytes = image(false);
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(b"private trailing bytes");
    let mut second = bytes.clone();
    second.extend_from_slice(&bytes);
    let mut malformed = bytes.clone();
    malformed.splice(2..2, [0xff, 0xfe, 0, 1]);
    for bytes in [
        trailing,
        second,
        malformed,
        bytes[..bytes.len() - 1].to_vec(),
        vec![0xff, 0xd8, 0xff, 0xd9],
    ] {
        assert!(
            Decoder
                .metadata(Format::Jpeg, &bytes, Default::default())
                .is_err()
        );
    }
    // A metadata-only marker stream cannot pass as a decoded publication image.
    let mut no_pixels = vec![0xff, 0xd8];
    no_pixels.extend(segment(0xfe, b"valid comment"));
    no_pixels.extend([0xff, 0xd9]);
    assert!(matches!(
        Decoder.metadata(Format::Jpeg, &no_pixels, Default::default()),
        Err(Error::Decode)
    ));
}

#[test]
fn unsupported_comment_encoding_fails_instead_of_replacing_characters() {
    for payload in [b"private\0value".as_slice(), b"\xff", b"\x1bsecret"] {
        let mut bytes = image(false);
        insert(&mut bytes, 2, 0xfe, payload);
        assert!(matches!(
            Decoder.metadata(Format::Jpeg, &bytes, Default::default()),
            Err(Error::Unsupported(_))
        ));
    }
}
