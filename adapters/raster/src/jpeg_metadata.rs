//! Closed JPEG container coverage plus the existing strict native pixel decoder.
//!
//! The decoder does not expose every ignored marker or trailing byte. This bounded
//! marker walk closes that gap; it does not implement entropy or pixel decoding.
//! Only thumbnail-free JFIF and UTF-8 comments are admitted metadata. Other APP
//! containers (including Exif, XMP and ICC) remain explicitly unsupported.

use gazette_core::{
    raster::{Error, Format, Limits, Metadata, MetadataLimits},
    source::Sha256Digest,
};

pub(super) fn inspect(bytes: &[u8], limits: MetadataLimits) -> Result<Metadata, Error> {
    if bytes.len() > limits.encoded_bytes {
        return Err(Error::Limit("metadata encoded bytes"));
    }
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return Err(Error::Decode);
    }
    let mut cursor = 2;
    let mut markers = 1usize;
    let mut entropy = false;
    let mut fields = Vec::new();
    let mut field_bytes = 0usize;
    let mut jfif = false;
    loop {
        let marker = marker(bytes, &mut cursor, entropy)?;
        markers = markers
            .checked_add(1)
            .ok_or(Error::Limit("metadata markers"))?;
        if markers > limits.chunks {
            return Err(Error::Limit("metadata markers"));
        }
        if marker == 0xd9 {
            if cursor != bytes.len() {
                return Err(Error::Decode);
            }
            break;
        }
        if matches!(marker, 0xd0..=0xd7) && entropy {
            continue;
        }
        if matches!(marker, 0x00 | 0xd0..=0xd8) {
            return Err(Error::Decode);
        }
        entropy = false;
        let length = take(bytes, &mut cursor, 2)?;
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]))
            .checked_sub(2)
            .ok_or(Error::Decode)?;
        if length > limits.chunk_bytes {
            return Err(Error::Limit("metadata marker bytes"));
        }
        let payload = take(bytes, &mut cursor, length)?;
        match marker {
            // Huffman-coded frames/tables/scans. Native decoding validates their
            // contents and order after complete metadata bounds have passed.
            0xc0 | 0xc2 | 0xc4 | 0xdb | 0xdd => {}
            0xda => entropy = true,
            0xe0 => {
                if jfif {
                    return Err(Error::Decode);
                }
                if payload.len() != 14
                    || &payload[..5] != b"JFIF\0"
                    || payload[5] != 1
                    || payload[6] > 2
                    || payload[7] > 2
                    || payload[12..] != [0, 0]
                {
                    return Err(Error::Unsupported("JPEG APP0 or thumbnail metadata"));
                }
                if payload[8..10] == [0, 0] || payload[10..12] == [0, 0] {
                    return Err(Error::Decode);
                }
                jfif = true;
            }
            0xfe => {
                if fields.len() >= limits.values {
                    return Err(Error::Limit("metadata values"));
                }
                let text = std::str::from_utf8(payload)
                    .map_err(|_| Error::Unsupported("JPEG comment encoding"))?;
                if text
                    .chars()
                    .any(|ch| ch.is_control() && !matches!(ch, '\t' | '\r' | '\n'))
                {
                    return Err(Error::Unsupported("JPEG comment control characters"));
                }
                field_bytes = field_bytes
                    .checked_add(text.len())
                    .filter(|&size| size <= limits.decoded_bytes)
                    .ok_or(Error::Limit("metadata decoded bytes"))?;
                fields.push(text.to_owned());
            }
            _ => return Err(Error::Unsupported("JPEG marker outside metadata profile")),
        }
    }
    // Do not bless a marker-shaped but undecodable image. Reuse the same native
    // decoder as transformations, charging its RGB output plus retained fields.
    let pixels = super::jpeg::pixels(
        bytes,
        Limits {
            encoded_bytes: limits.encoded_bytes,
            decoded_bytes: u64::try_from(limits.decoded_bytes - field_bytes)
                .map_err(|_| Error::Limit("metadata decoded bytes"))?,
            width: u32::from(u16::MAX),
            height: u32::from(u16::MAX),
        },
    )?;
    let report = Metadata {
        format: Format::Jpeg,
        bytes: bytes.len(),
        sha256: Sha256Digest::of(bytes),
        tool: "gazette.raster-metadata/1;zune-jpeg/0.5.16-rc2;strict;closed-jpeg-jfif-utf8-com",
        decoded_bytes: field_bytes
            .checked_add(pixels.bytes.len())
            .ok_or(Error::Limit("metadata decoded bytes"))?,
        fields,
    };
    report.validate(Format::Jpeg, bytes, limits)?;
    Ok(report)
}

fn take<'a>(bytes: &'a [u8], cursor: &mut usize, length: usize) -> Result<&'a [u8], Error> {
    let end = cursor.checked_add(length).ok_or(Error::Decode)?;
    let value = bytes.get(*cursor..end).ok_or(Error::Decode)?;
    *cursor = end;
    Ok(value)
}

// T.81 B.1.1.2/F.1.2.3: entropy FF00 is data; restart markers retain scan state.
// Metadata payload bytes are length-delimited and never searched for marker codes.
fn marker(bytes: &[u8], cursor: &mut usize, entropy: bool) -> Result<u8, Error> {
    loop {
        let byte = take(bytes, cursor, 1)?[0];
        if byte != 0xff {
            if entropy {
                continue;
            }
            return Err(Error::Decode);
        }
        let mut fill = 0usize;
        let code = loop {
            let code = take(bytes, cursor, 1)?[0];
            if code != 0xff {
                break code;
            }
            fill += 1;
        };
        if code == 0 && entropy && fill == 0 {
            continue;
        }
        return Ok(code);
    }
}
