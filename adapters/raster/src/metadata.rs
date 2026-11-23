//! Closed PNG metadata profile. Native PNG owns chunk decoding and CRC; native zlib
//! owns member decoding. Additional guards reject decoder-tolerated omissions/tails.
//! No pixels are reconstructed, metadata stripped, or original bytes rewritten.
//! XMP is unsupported rather than treated as plain text. Language tags have the
//! decoder's ASCII check, not BCP47 registry validation or language adjudication.

mod member;

use std::collections::BTreeSet;

use gazette_core::{
    raster::{Error, Format, Metadata, MetadataLimits},
    source::Sha256Digest,
};
use png::{BitDepth, ColorType, Decoded, StreamingDecoder, chunk::ChunkType};

use member::Member;

const PROFILE: &str = "gazette.raster-metadata/1;png/0.18.0;flate2/1.1.10;zlib-rs/0.6.7;closed-png";

struct Budget {
    maximum: usize,
    used: usize,
}

impl Budget {
    fn remaining(&self) -> usize {
        self.maximum - self.used
    }
    fn charge(&mut self, amount: usize) -> Result<(), Error> {
        self.used = self
            .used
            .checked_add(amount)
            .filter(|&used| used <= self.maximum)
            .ok_or(Error::Limit("metadata decoded bytes"))?;
        Ok(())
    }
    fn field(&mut self, fields: &mut Vec<String>, text: &str) -> Result<(), Error> {
        if text.contains('\0') {
            return Err(Error::Decode);
        }
        self.charge(text.len())?;
        fields.push(text.to_owned());
        Ok(())
    }
}

struct Order {
    seen: BTreeSet<[u8; 4]>,
    data: Data,
    palette: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Data {
    Before,
    During,
    After,
}

impl Order {
    fn begin(
        &mut self,
        kind: [u8; 4],
        length: usize,
        decoder: &StreamingDecoder,
    ) -> Result<usize, Error> {
        let text_fields = match &kind {
            b"tEXt" | b"zTXt" => 2,
            b"iTXt" => 4,
            _ => 0,
        };
        match &kind {
            b"IHDR" if length == 13 && self.seen.is_empty() => {}
            b"PLTE"
                if self.data == Data::Before
                    && length > 0
                    && length <= 768
                    && length.is_multiple_of(3) =>
            {
                let info = decoder.info().ok_or(Error::Decode)?;
                let color = info.color_type;
                if matches!(color, ColorType::Grayscale | ColorType::GrayscaleAlpha) {
                    return Err(Error::Decode);
                }
                if self.seen.contains(b"tRNS") {
                    return Err(Error::Decode);
                }
                if color == ColorType::Indexed {
                    let entries = match info.bit_depth {
                        BitDepth::One => 2,
                        BitDepth::Two => 4,
                        BitDepth::Four => 16,
                        BitDepth::Eight => 256,
                        _ => return Err(Error::Decode),
                    };
                    if length / 3 > entries {
                        return Err(Error::Decode);
                    }
                }
                self.palette = true;
            }
            b"IDAT" if self.data != Data::After => {
                let info = decoder.info().ok_or(Error::Decode)?;
                if info.color_type == ColorType::Indexed && !self.palette {
                    return Err(Error::Decode);
                }
                self.data = Data::During;
            }
            b"IEND" if length == 0 && self.data != Data::Before => {}
            b"tRNS" if self.data == Data::Before => {
                let info = decoder.info().ok_or(Error::Decode)?;
                let valid = match info.color_type {
                    ColorType::Grayscale => length == 2,
                    ColorType::Rgb => length == 6,
                    ColorType::Indexed => {
                        self.palette
                            && length > 0
                            && info
                                .palette
                                .as_ref()
                                .is_some_and(|palette| length <= palette.len() / 3)
                    }
                    _ => false,
                };
                if !valid {
                    return Err(Error::Decode);
                }
            }
            b"gAMA" if length == 4 && self.data == Data::Before && !self.palette => {}
            b"cHRM" if length == 32 && self.data == Data::Before && !self.palette => {}
            b"sRGB" if length == 1 && self.data == Data::Before && !self.palette => {}
            b"pHYs" if length == 9 && self.data == Data::Before => {}
            b"tEXt" | b"zTXt" | b"iTXt" => {}
            b"IHDR" | b"PLTE" | b"IDAT" | b"IEND" | b"tRNS" | b"gAMA" | b"cHRM" | b"sRGB"
            | b"pHYs" => return Err(Error::Decode),
            _ => return Err(Error::Unsupported("PNG chunk outside metadata profile")),
        }
        if kind != *b"IDAT" && self.data == Data::During {
            self.data = Data::After;
        }
        if text_fields == 0 && kind != *b"IDAT" && !self.seen.insert(kind) {
            return Err(Error::Decode);
        }
        Ok(text_fields)
    }
}

pub(super) fn inspect(bytes: &[u8], limits: MetadataLimits) -> Result<Metadata, Error> {
    let mut decoder = StreamingDecoder::new();
    decoder.set_ignore_crc(false);
    decoder.set_skip_ancillary_crc_failures(false);
    decoder.set_ignore_adler32(false);
    let mut budget = Budget {
        maximum: limits.decoded_bytes,
        used: 0,
    };
    let mut fields = Vec::new();
    let mut count = 0usize;
    let mut values = 0usize;
    let mut offset = 0usize;
    let mut payload = &bytes[..0];
    let mut image = Member::new();
    let mut order = Order {
        seen: BTreeSet::new(),
        data: Data::Before,
        palette: false,
    };
    loop {
        let (consumed, event) = decoder
            .update(bytes.get(offset..).ok_or(Error::Decode)?, None)
            .map_err(|_| Error::Decode)?;
        offset = offset.checked_add(consumed).ok_or(Error::Decode)?;
        match event {
            Decoded::ChunkBegin(length, ChunkType(kind)) => {
                count = count
                    .checked_add(1)
                    .filter(|&count| count <= limits.chunks)
                    .ok_or(Error::Limit("metadata chunks"))?;
                let length =
                    usize::try_from(length).map_err(|_| Error::Limit("metadata chunk bytes"))?;
                if length > limits.chunk_bytes {
                    return Err(Error::Limit("metadata chunk bytes"));
                }
                values = values
                    .checked_add(order.begin(kind, length, &decoder)?)
                    .filter(|&values| values <= limits.values)
                    .ok_or(Error::Limit("metadata values"))?;
                let end = offset.checked_add(length).ok_or(Error::Decode)?;
                payload = bytes.get(offset..end).ok_or(Error::Decode)?;
            }
            Decoded::ChunkComplete(ChunkType(kind)) => match &kind {
                b"IDAT" => image.feed(payload, &mut budget)?,
                b"tEXt" | b"zTXt" | b"iTXt" => {
                    text(&decoder, kind, payload, &mut budget, &mut fields)?
                }
                b"IEND" => {
                    image.finish(&mut budget)?;
                    if offset != bytes.len() {
                        return Err(Error::Decode);
                    }
                    break;
                }
                _ => {}
            },
            Decoded::BadAncillaryChunk(_) | Decoded::SkippedAncillaryChunk(_) => {
                return Err(Error::Decode);
            }
            Decoded::Nothing if consumed == 0 => return Err(Error::Decode),
            _ => {}
        }
        if offset == bytes.len() {
            return Err(Error::Decode);
        }
    }
    let metadata = Metadata {
        format: Format::Png,
        bytes: bytes.len(),
        sha256: Sha256Digest::of(bytes),
        tool: PROFILE,
        decoded_bytes: budget.used,
        fields,
    };
    metadata.validate(Format::Png, bytes, limits)?;
    Ok(metadata)
}

// Only recover compressed-member extents omitted by the decoder's public text API.
// Native PNG decoding above remains the owner of keyword/language/UTF-8 grammar.
fn after_zero(bytes: &[u8]) -> Result<&[u8], Error> {
    bytes
        .iter()
        .position(|&byte| byte == 0)
        .and_then(|at| bytes.get(at + 1..))
        .ok_or(Error::Decode)
}

fn text(
    decoder: &StreamingDecoder,
    kind: [u8; 4],
    payload: &[u8],
    budget: &mut Budget,
    fields: &mut Vec<String>,
) -> Result<(), Error> {
    let info = decoder.info().ok_or(Error::Decode)?;
    // PNG 3 §11.3.3.1: the decoder checks length but tolerates these malformed spellings.
    let keyword = payload
        .split(|&byte| byte == 0)
        .next()
        .ok_or(Error::Decode)?;
    if keyword == b"XML:com.adobe.xmp" {
        return Err(Error::Unsupported("PNG XMP metadata"));
    }
    if keyword.is_empty()
        || keyword.len() > 79
        || keyword.first() == Some(&b' ')
        || keyword.last() == Some(&b' ')
        || keyword.windows(2).any(|pair| pair == b"  ")
        || !keyword
            .iter()
            .all(|&byte| matches!(byte, 32..=126 | 161..=255))
    {
        return Err(Error::Decode);
    }
    let compressed = match &kind {
        b"zTXt" => Some(after_zero(payload)?.get(1..).ok_or(Error::Decode)?),
        b"iTXt" => {
            let rest = after_zero(payload)?;
            if rest.get(1) != Some(&0) {
                return Err(Error::Decode);
            }
            if rest.first() == Some(&1) {
                Some(after_zero(after_zero(
                    rest.get(2..).ok_or(Error::Decode)?,
                )?)?)
            } else {
                None
            }
        }
        _ => None,
    };
    let inflated = if let Some(compressed) = compressed {
        let mut member = Member::new();
        member.feed(compressed, budget)?;
        member.finish(budget)?;
        let decoded = member.decoded()?;
        // Account again for the decoder's separately materialized text inflation.
        budget.charge(decoded)?;
        Some((decoded, member.latin1_bytes()))
    } else {
        None
    };
    match &kind {
        b"tEXt" => {
            let text = info.uncompressed_latin1_text.last().ok_or(Error::Decode)?;
            budget.field(fields, &text.keyword)?;
            budget.field(fields, &text.text)?;
        }
        b"zTXt" => {
            let mut text = info
                .compressed_latin1_text
                .last()
                .ok_or(Error::Decode)?
                .clone();
            budget.field(fields, &text.keyword)?;
            let (inflated, field_bytes) = inflated.ok_or(Error::Decode)?;
            // Account exact Latin-1 expansion before the native decoder creates UTF-8.
            budget.charge(field_bytes)?;
            text.decompress_text_with_limit(inflated)
                .map_err(|_| Error::Decode)?;
            let value = text.get_text().map_err(|_| Error::Decode)?;
            if value.contains('\0') {
                return Err(Error::Decode);
            }
            if value.len() != field_bytes {
                return Err(Error::Binding);
            }
            fields.push(value);
        }
        b"iTXt" => {
            let mut text = info.utf8_text.last().ok_or(Error::Decode)?.clone();
            budget.field(fields, &text.keyword)?;
            budget.field(fields, &text.language_tag)?;
            budget.field(fields, &text.translated_keyword)?;
            if let Some((inflated, _)) = inflated {
                budget.charge(inflated)?;
                text.decompress_text_with_limit(inflated)
                    .map_err(|_| Error::Decode)?;
                let value = text.get_text().map_err(|_| Error::Decode)?;
                if value.contains('\0') {
                    return Err(Error::Decode);
                }
                if value.len() != inflated {
                    return Err(Error::Binding);
                }
                fields.push(value);
            } else {
                // Uncompressed UTF-8 was validated by the native chunk parser;
                // its exact source extent bounds the getter allocation beforehand.
                let rest = after_zero(payload)?.get(2..).ok_or(Error::Decode)?;
                let field_bytes = after_zero(after_zero(rest)?)?.len();
                budget.charge(field_bytes)?;
                let value = text.get_text().map_err(|_| Error::Decode)?;
                if value.contains('\0') {
                    return Err(Error::Decode);
                }
                if value.len() != field_bytes {
                    return Err(Error::Binding);
                }
                fields.push(value);
            }
        }
        _ => return Err(Error::Decode),
    }
    Ok(())
}
