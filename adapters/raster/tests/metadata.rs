//! Real PNG container and native zlib fixtures; inspection never rewrites the source.

use std::io::Write;

use flate2::{Compression, Crc, write::ZlibEncoder};
use gazette_adapter_raster::Decoder;
use gazette_core::raster::{Error, Format, Metadata, MetadataLimits};
use gazette_ports::raster::{RasterDecoder, RasterMetadata};

const HEADER: &[u8] = &[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0];

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(bytes)
        .expect("native zlib fixture encoding");
    encoder.finish().expect("complete native zlib member")
}

fn chunk(output: &mut Vec<u8>, kind: [u8; 4], bytes: &[u8]) {
    output.extend_from_slice(
        &u32::try_from(bytes.len())
            .expect("bounded fixture chunk")
            .to_be_bytes(),
    );
    output.extend_from_slice(&kind);
    output.extend_from_slice(bytes);
    let mut crc = Crc::new();
    crc.update(&kind);
    crc.update(bytes);
    output.extend_from_slice(&crc.sum().to_be_bytes());
}

fn image(before: &[([u8; 4], Vec<u8>)], after: &[([u8; 4], Vec<u8>)]) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut bytes, *b"IHDR", HEADER);
    for (kind, payload) in before {
        chunk(&mut bytes, *kind, payload);
    }
    chunk(&mut bytes, *b"IDAT", &compressed(&[0, 10, 20, 30, 255]));
    for (kind, payload) in after {
        chunk(&mut bytes, *kind, payload);
    }
    chunk(&mut bytes, *b"IEND", &[]);
    bytes
}

fn inspect(bytes: &[u8]) -> Result<Metadata, Error> {
    Decoder.metadata(Format::Png, bytes, MetadataLimits::default())
}

fn ztext(text: &[u8]) -> Vec<u8> {
    let mut payload = b"Comment\0\0".to_vec();
    payload.extend_from_slice(&compressed(text));
    payload
}

fn itext(compress: bool) -> Vec<u8> {
    let mut payload = b"Title\0".to_vec();
    payload.extend_from_slice(&[u8::from(compress), 0]);
    payload.extend_from_slice("en\0Títle\0".as_bytes());
    payload.extend_from_slice(&if compress {
        compressed("private π".as_bytes())
    } else {
        "private π".as_bytes().to_vec()
    });
    payload
}

#[test]
fn actual_png_text_variants_preserve_every_field_order_duplicates_and_exact_bytes() {
    let source = image(
        &[
            (*b"tEXt", b"Comment\0caf\xe9".to_vec()),
            (*b"zTXt", ztext(b"private-host.invalid")),
        ],
        &[
            (*b"iTXt", itext(false)),
            (*b"iTXt", itext(true)),
            (*b"tEXt", b"Comment\0again".to_vec()),
        ],
    );
    Decoder
        .inspect(Format::Png, &source, Default::default())
        .expect("fixture has actual decodable pixels");
    let original = source.clone();
    let result = inspect(&source).expect("complete bounded metadata");
    assert_eq!(
        result.fields,
        [
            "Comment",
            "café",
            "Comment",
            "private-host.invalid",
            "Title",
            "en",
            "Títle",
            "private π",
            "Title",
            "en",
            "Títle",
            "private π",
            "Comment",
            "again"
        ]
    );
    assert_eq!(
        result,
        inspect(&source).expect("deterministic native inspection")
    );
    assert_eq!(
        result.sha256,
        gazette_core::source::Sha256Digest::of(&source)
    );
    assert_eq!(result.bytes, source.len());
    assert_eq!(source, original);
    let fields: usize = result.fields.iter().map(String::len).sum();
    assert_eq!(
        result.decoded_bytes,
        5 + fields + 2 * (b"private-host.invalid".len() + "private π".len())
    );
}

#[test]
fn exact_limits_count_inflation_fields_and_predecode_chunks() {
    let bytes = image(&[(*b"zTXt", ztext(b"a"))], &[]);
    let report = inspect(&bytes).expect("native baseline");
    let limits = MetadataLimits {
        encoded_bytes: bytes.len(),
        chunk_bytes: 64,
        decoded_bytes: report.decoded_bytes,
        values: 2,
        chunks: 4,
    };
    Decoder
        .metadata(Format::Png, &bytes, limits)
        .expect("exact decoded/value/chunk/encoded limits");
    for selected in [
        MetadataLimits {
            encoded_bytes: bytes.len() - 1,
            ..limits
        },
        MetadataLimits {
            decoded_bytes: report.decoded_bytes - 1,
            ..limits
        },
        MetadataLimits {
            values: 1,
            ..limits
        },
        MetadataLimits {
            chunks: 3,
            ..limits
        },
        MetadataLimits {
            chunk_bytes: 12,
            ..limits
        },
    ] {
        assert!(
            matches!(
                Decoder.metadata(Format::Png, &bytes, selected),
                Err(Error::Limit(_))
            ),
            "{selected:?}"
        );
    }
    let bomb = image(&[(*b"zTXt", ztext(&vec![b'x'; 100_000]))], &[]);
    assert!(matches!(
        Decoder.metadata(
            Format::Png,
            &bomb,
            MetadataLimits {
                decoded_bytes: 100,
                ..Default::default()
            }
        ),
        Err(Error::Limit(_))
    ));
}

#[test]
fn unknown_animation_exif_icc_and_empty_jpeg_never_become_metadata_success() {
    for kind in [*b"zzZZ", *b"acTL", *b"fcTL", *b"eXIf", *b"iCCP"] {
        let bytes = image(&[(kind, vec![0; 8])], &[]);
        assert!(
            matches!(inspect(&bytes), Err(Error::Unsupported(_))),
            "{kind:?}"
        );
    }
    // The native parser rejects an out-of-order fdAT before exposing its chunk kind.
    let invalid_frame_data = image(&[(*b"fdAT", vec![0; 8])], &[]);
    assert!(matches!(inspect(&invalid_frame_data), Err(Error::Decode)));
    assert!(matches!(
        Decoder.metadata(Format::Jpeg, b"\xff\xd8\xff\xd9", Default::default()),
        Err(Error::Decode)
    ));
}

#[test]
fn every_truncation_crc_failure_and_outer_tail_is_refused() {
    let bytes = image(&[], &[(*b"tEXt", b"Comment\0after pixels".to_vec())]);
    for length in 0..bytes.len() {
        assert!(
            inspect(&bytes[..length]).is_err(),
            "accepted prefix {length}"
        );
    }
    let mut crc = bytes.clone();
    crc[bytes.len() - 13] ^= 1; // Text CRC immediately before the IEND chunk.
    assert!(matches!(inspect(&crc), Err(Error::Decode)));
    for suffix in [b"hidden".as_slice(), bytes.as_slice()] {
        let mut extra = bytes.clone();
        extra.extend_from_slice(suffix);
        assert!(matches!(inspect(&extra), Err(Error::Decode)));
    }
}

#[test]
fn compressed_text_and_idat_require_one_complete_member_without_tails() {
    for tail in [vec![42], compressed(b"second member")] {
        let mut text = ztext(b"first");
        text.extend_from_slice(&tail);
        assert!(matches!(
            inspect(&image(&[(*b"zTXt", text)], &[])),
            Err(Error::Decode)
        ));
        let mut international = itext(true);
        international.extend_from_slice(&tail);
        assert!(matches!(
            inspect(&image(&[(*b"iTXt", international)], &[])),
            Err(Error::Decode)
        ));
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut bytes, *b"IHDR", HEADER);
        let mut pixels = compressed(&[0, 10, 20, 30, 255]);
        pixels.extend_from_slice(&tail);
        chunk(&mut bytes, *b"IDAT", &pixels);
        chunk(&mut bytes, *b"IEND", &[]);
        assert!(matches!(inspect(&bytes), Err(Error::Decode)));
    }
    let mut truncated = ztext(b"first");
    truncated.pop();
    assert!(matches!(
        inspect(&image(&[(*b"zTXt", truncated)], &[])),
        Err(Error::Decode)
    ));
}

#[test]
fn ordering_lengths_duplicates_and_interrupted_idat_are_not_repaired() {
    for chunks in [
        vec![(*b"IHDR", HEADER.to_vec())],
        vec![(*b"gAMA", vec![0; 5])],
        vec![(*b"sRGB", vec![0]), (*b"sRGB", vec![0])],
        vec![(*b"iTXt", b"Title\0\0\x01en\0Title\0text".to_vec())],
    ] {
        assert!(matches!(inspect(&image(&chunks, &[])), Err(Error::Decode)));
    }
    let bytes = image(&[], &[(*b"gAMA", 100u32.to_be_bytes().to_vec())]);
    assert!(matches!(inspect(&bytes), Err(Error::Decode)));
    let bytes = image(&[], &[(*b"IEND", vec![0])]);
    assert!(matches!(inspect(&bytes), Err(Error::Decode)));
    let bytes = image(&[], &[(*b"tEXt", b"K\0V".to_vec()), (*b"IDAT", Vec::new())]);
    assert!(matches!(inspect(&bytes), Err(Error::Decode)));
}

#[test]
fn permissive_native_text_and_scalar_cases_are_refused_without_repair() {
    for payload in [
        b" leading\0text".as_slice(),
        b"trailing \0text",
        b"two  spaces\0text",
        b"bad\x7fkey\0text",
        b"Comment\0hidden\0tail",
    ] {
        assert!(matches!(
            inspect(&image(&[(*b"tEXt", payload.to_vec())], &[])),
            Err(Error::Decode)
        ));
    }
    assert!(matches!(
        inspect(&image(&[(*b"zTXt", ztext(b"hidden\0tail"))], &[])),
        Err(Error::Decode)
    ));
    for (kind, payload) in [
        (*b"gAMA", vec![0; 4]),
        (*b"sRGB", vec![4]),
        (*b"pHYs", vec![0, 0, 0, 1, 0, 0, 0, 1, 2]),
    ] {
        assert!(matches!(
            inspect(&image(&[(kind, payload)], &[])),
            Err(Error::Decode)
        ));
    }
}

#[test]
fn registered_xmp_is_not_misrepresented_as_decoded_plain_text() {
    let mut ztxt = b"XML:com.adobe.xmp\0\0".to_vec();
    ztxt.extend_from_slice(&compressed(b"<x>hidden</x>"));
    for (kind, payload) in [
        (*b"tEXt", b"XML:com.adobe.xmp\0<x>hidden</x>".to_vec()),
        (*b"zTXt", ztxt),
        (
            *b"iTXt",
            b"XML:com.adobe.xmp\0\0\0\0\0<x>hidden</x>".to_vec(),
        ),
    ] {
        assert!(matches!(
            inspect(&image(&[(kind, payload)], &[])),
            Err(Error::Unsupported(_))
        ));
    }
}

#[test]
fn one_member_can_span_consecutive_idat_chunks_without_decoding_pixels_twice() {
    let pixels = compressed(&[0, 10, 20, 30, 255]);
    for split in 0..=pixels.len() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        chunk(&mut bytes, *b"IHDR", HEADER);
        chunk(&mut bytes, *b"IDAT", &pixels[..split]);
        chunk(&mut bytes, *b"IDAT", &pixels[split..]);
        chunk(&mut bytes, *b"IEND", &[]);
        let report = inspect(&bytes).expect("one native zlib member across chunk boundaries");
        assert!(report.fields.is_empty());
        assert_eq!(report.decoded_bytes, 5);
    }
}

#[path = "metadata/webp.rs"]
mod webp;
