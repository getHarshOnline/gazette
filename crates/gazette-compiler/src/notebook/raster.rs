//! Notebook MIME transport decoding; pixel decoding belongs to the supplied raster port.

use base64::{Engine, engine::general_purpose::STANDARD};
use gazette_core::raster::{Error, Format, Inspect, Inspection, Limits};

pub(super) fn decode(
    encoded: &str,
    format: Format,
    source_bytes: usize,
    inspect: &Inspect<'_>,
) -> Result<(Vec<u8>, Inspection), Error> {
    let limits = Limits {
        encoded_bytes: Limits::default().encoded_bytes.min(source_bytes),
        ..Limits::default()
    };
    // nbformat permits multiline Base64 image strings. Remove line endings only;
    // arbitrary whitespace, a data-URI prefix, URL-safe alphabet and malformed
    // padding remain errors in the pinned strict standard decoder.
    let encoded: Vec<u8> = encoded
        .bytes()
        .filter(|byte| !matches!(byte, b'\r' | b'\n'))
        .collect();
    if encoded.is_empty() || !encoded.len().is_multiple_of(4) {
        return Err(Error::Decode);
    }
    let padding = encoded
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2 {
        return Err(Error::Decode);
    }
    let length = (encoded.len() / 4)
        .checked_mul(3)
        .and_then(|length| length.checked_sub(padding))
        .ok_or(Error::Decode)?;
    if length > limits.encoded_bytes {
        return Err(Error::Limit("encoded bytes"));
    }
    let mut bytes = vec![0; length];
    let decoded = STANDARD
        .decode_slice(&encoded, &mut bytes)
        .map_err(|_| Error::Decode)?;
    if decoded != length {
        return Err(Error::Decode);
    }
    let image = inspect(format, &bytes, limits)?;
    image.validate(format, &bytes, limits)?;
    Ok((bytes, image))
}
