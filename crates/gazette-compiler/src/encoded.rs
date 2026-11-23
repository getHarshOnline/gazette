//! One-layer disclosure decoding, not URL admission or decoded-container validation.
//!
//! Data bodies follow Fetch's data-URL and Infra's forgiving-base64 algorithms.
//! Header support is deliberately narrower: a MIME essence and at most one
//! token-valued charset. Other metadata is an explicit unsupported outcome.

use std::fmt;

use base64::{
    Engine as _, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig},
};
use percent_encoding::percent_decode;
use url::{Position, Url};

/// Qualified syntax profile; neither browser execution nor container certification.
pub const PROFILE: &str = "gazette-encoded/1";

/// Per-value bounds; native URL-parser scratch is bounded by input size, not body size.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Complete original input, including metadata, fragment and surrounding whitespace.
    pub input_bytes: usize,
    /// Exact returned body bytes, measured before output allocation.
    pub decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            decoded_bytes: 1 << 20,
        }
    }
}

/// The transformation performed; declarations do not establish actual media contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// At least one valid percent escape was decoded; plus signs remain literal.
    Percent,
    /// A complete data URL with an admitted metadata subset.
    Data {
        /// Lowercase declared MIME essence, or the Fetch default.
        media_type: String,
        /// Declared charset label, without transcoding or encoding validation.
        charset: Option<String>,
    },
}

/// Exact bytes after one decoding layer. Binary bytes and nested encodings remain opaque.
#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    /// Non-lossy payload, not necessarily UTF-8.
    pub bytes: Vec<u8>,
    /// Applied syntax profile category.
    pub kind: Kind,
}

/// Redacted refusal; errors never contain input or partially decoded payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// A caller-selected resource ceiling was exhausted.
    Limit(&'static str),
    /// Recognized data URL or Base64 body is malformed.
    Malformed,
    /// Header semantics lie outside the supported metadata subset.
    Unsupported,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(bound) => write!(f, "encoded value exceeds {bound}"),
            Self::Malformed => f.write_str("malformed encoded value"),
            Self::Unsupported => f.write_str("unsupported encoded metadata"),
        }
    }
}
impl std::error::Error for Error {}

/// Decode a complete data URL before considering generic percent escapes.
///
/// No bare Base64 guessing, recursive decoding, charset conversion, navigation or
/// IO occurs. Invalid generic percent escapes remain literal. An unchanged
/// generic value returns `None`; a recognized data URL missing its comma fails.
///
/// # Errors
/// Rejects input/output excess, malformed data syntax/Base64 and unsupported
/// metadata. No partial decoded value is returned.
pub fn decode(value: &[u8], limits: Limits) -> Result<Option<Decoded>, Error> {
    if value.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    // This allocation-free discriminator mirrors only native URL preprocessing
    // needed to recognize a scheme. Url remains the parser/serialization owner.
    let data = value
        .iter()
        .copied()
        .skip_while(|byte| *byte <= b' ')
        .filter(|byte| !matches!(byte, b'\t' | b'\r' | b'\n'))
        .take(5)
        .map(|byte| byte.to_ascii_lowercase())
        .eq(b"data:".iter().copied());
    if data {
        let value = std::str::from_utf8(value).map_err(|_| Error::Malformed)?;
        let url = Url::parse(value).map_err(|_| Error::Malformed)?;
        if url.scheme() != "data" {
            return Err(Error::Malformed);
        }
        let serialized = &url[..Position::AfterQuery];
        let (header, body) = serialized[5..].split_once(',').ok_or(Error::Malformed)?;
        let header = header.trim_matches(ascii_whitespace_char);
        let (header, base64) = header
            .rsplit_once(';')
            .filter(|(_, flag)| flag.trim_start_matches(' ').eq_ignore_ascii_case("base64"))
            .map_or((header, false), |(header, _)| (header, true));
        let kind = metadata(header)?;
        let bytes = if base64 {
            forgiving_base64(body.as_bytes(), limits)?
        } else {
            percent_bytes(body.as_bytes(), limits)?
        };
        return Ok(Some(Decoded { bytes, kind }));
    }
    let length = percent_decode(value).count();
    if length == value.len() {
        return Ok(None);
    }
    Ok(Some(Decoded {
        bytes: percent_bytes(value, limits)?,
        kind: Kind::Percent,
    }))
}

fn percent_bytes(value: &[u8], limits: Limits) -> Result<Vec<u8>, Error> {
    let length = percent_decode(value).count();
    if length > limits.decoded_bytes {
        return Err(Error::Limit("decoded bytes"));
    }
    let mut bytes = Vec::with_capacity(length);
    bytes.extend(percent_decode(value));
    Ok(bytes)
}

fn ascii_whitespace(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}
fn ascii_whitespace_char(character: char) -> bool {
    character.is_ascii() && ascii_whitespace(character as u8)
}

fn forgiving_base64(value: &[u8], limits: Limits) -> Result<Vec<u8>, Error> {
    let normalized = || percent_decode(value).filter(|byte| !ascii_whitespace(*byte));
    let mut length = 0usize;
    let mut padding = 0usize;
    for byte in normalized() {
        length += 1;
        if byte == b'=' {
            padding += 1;
        } else if padding != 0 || !(byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/')) {
            return Err(Error::Malformed);
        }
    }
    if padding > 2 || (padding != 0 && !length.is_multiple_of(4)) {
        return Err(Error::Malformed);
    }
    let symbols = length - padding;
    if symbols % 4 == 1 {
        return Err(Error::Malformed);
    }
    let decoded = symbols / 4 * 3 + symbols % 4 * 3 / 4;
    if decoded > limits.decoded_bytes {
        return Err(Error::Limit("decoded bytes"));
    }
    // Scratch is no larger than the URL body. Output is allocated at its exact
    // checked length; native Base64 decoding owns bit assembly and validation.
    let mut input = Vec::with_capacity(symbols);
    input.extend(normalized().take(symbols));
    let engine = GeneralPurpose::new(
        &alphabet::STANDARD,
        GeneralPurposeConfig::new()
            .with_decode_padding_mode(DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    let mut bytes = vec![0; decoded];
    let written = engine
        .decode_slice(input, &mut bytes)
        .map_err(|_| Error::Malformed)?;
    if written != decoded {
        return Err(Error::Malformed);
    }
    Ok(bytes)
}

fn metadata(header: &str) -> Result<Kind, Error> {
    if header.is_empty() {
        return Ok(Kind::Data {
            media_type: "text/plain".to_owned(),
            charset: Some("US-ASCII".to_owned()),
        });
    }
    let mut parts = header.split(';');
    let essence = parts
        .next()
        .ok_or(Error::Unsupported)?
        .trim_matches(ascii_whitespace_char);
    let essence = if essence.is_empty() {
        "text/plain"
    } else {
        essence
    };
    let (major, minor) = essence.split_once('/').ok_or(Error::Unsupported)?;
    if !token(major) || !token(minor) {
        return Err(Error::Unsupported);
    }
    let mut charset = None;
    for parameter in parts {
        let (name, value) = parameter
            .trim_matches(ascii_whitespace_char)
            .split_once('=')
            .ok_or(Error::Unsupported)?;
        if !name.eq_ignore_ascii_case("charset") || charset.is_some() {
            return Err(Error::Unsupported);
        }
        let value = if value.starts_with('"') {
            value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .ok_or(Error::Unsupported)?
        } else {
            value
        };
        if !token(value) {
            return Err(Error::Unsupported);
        }
        charset = Some(value.to_owned());
    }
    Ok(Kind::Data {
        media_type: essence.to_ascii_lowercase(),
        charset,
    })
}

fn token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}
