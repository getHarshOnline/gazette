//! Bounded CSS token text, not stylesheet validity, URL decoding or browser execution.
//!
//! Callbacks are speculative: discard their findings if any later token fails.
//! Native token allocations are bounded by input size, not by the callback byte budget.
//! For stylesheet bytes, only an exact leading `@charset "UTF-8";` is supported (encoding
//! label ASCII-case insensitive), optionally after one UTF-8 BOM. No conversion
//! or external transport/environment encoding is applied. Already-decoded CSS
//! preserves leading U+FEFF and cannot declare another decoding operation.

use std::fmt;

use cssparser::{ParseErrorKind, Parser, ParserInput, SourcePosition, Token};

/// Native tokenizer plus Gazette's explicit-termination inspection profile.
pub const PROFILE: &str = "cssparser@0.33.0/gazette-token-text-v1";

/// Distinguishes an external byte stream from CSS text already decoded by its owner.
#[derive(Clone, Copy, Debug)]
pub enum Input<'a> {
    /// Original stylesheet bytes: UTF-8 only, with an optional leading BOM signature.
    Stylesheet(&'a [u8]),
    /// Decoded style body/attribute text: no BOM stripping or encoding declarations.
    Decoded(&'a str),
}

/// Invocation ceilings. Nesting also has an implementation ceiling of 256 levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Exact UTF-8 source bytes, checked before native parsing.
    pub input_bytes: usize,
    /// All tokens, including whitespace, comments and closing delimiters.
    pub tokens: usize,
    /// Simultaneously open functions or blocks.
    pub depth: usize,
    /// Sum of decoded text bytes delivered to the consumer.
    pub decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            tokens: 100_000,
            depth: 64,
            decoded_bytes: 2 << 20,
        }
    }
}

/// Native lexical text category; adjacent tokens are never concatenated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// Identifiers, function names, at-keywords, hashes and dimension units.
    Identifier,
    /// Quoted strings, including strings inside a quoted `url(...)` function.
    String,
    /// The native unquoted URL token; no percent/base64/URL normalization.
    Url,
}

/// Redacted failure. A consumer payload remains available without being displayed.
#[derive(Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// An invocation ceiling was exhausted.
    Limit(&'static str),
    /// Source was not UTF-8.
    Utf8,
    /// Invalid tokens, missing terminators or unmatched delimiters.
    Malformed,
    /// Input requires behavior outside this token profile.
    Unsupported,
    /// Caller rejected a decoded token.
    Consumer(E),
}

impl<E> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(kind) => write!(f, "CSS inspection limit: {kind}"),
            Self::Utf8 => f.write_str("CSS input is not UTF-8"),
            Self::Malformed => f.write_str("CSS token stream is malformed or unterminated"),
            Self::Unsupported => f.write_str("CSS syntax is outside the inspection profile"),
            Self::Consumer(_) => f.write_str("CSS token consumer rejected input"),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for Error<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Consumer(error) => Some(error),
            _ => None,
        }
    }
}

/// Inspect each native decoded token without retaining a second syntax tree.
///
/// # Errors
/// Rejects malformed or implicitly repaired EOF syntax, exhausted limits and
/// consumer failures. Success does not validate properties, selectors or URLs.
pub fn inspect<E>(
    input: Input<'_>,
    limits: Limits,
    mut consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    let bytes = match input {
        Input::Stylesheet(bytes) => bytes,
        Input::Decoded(text) => text.as_bytes(),
    };
    if bytes.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    let (source, header) = match input {
        Input::Stylesheet(bytes) => {
            let source = std::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
            // Only original bytes can carry a decoding signature; all were charged above.
            let source = source.strip_prefix('\u{feff}').unwrap_or(source);
            (source, utf8_header(source))
        }
        Input::Decoded(source) => (source, false),
    };
    let mut input = ParserInput::new(source);
    let mut parser = Parser::new(&mut input);
    let mut budget = Budget {
        tokens: limits.tokens,
        decoded: limits.decoded_bytes,
        depth: limits.depth.min(256),
        utf8_header: header,
    };
    walk(&mut parser, &mut budget, 0, &mut consumer).map(|_| ())
}

struct Budget {
    tokens: usize,
    decoded: usize,
    depth: usize,
    utf8_header: bool,
}

// CSS Syntax's encoding declaration is an exact byte prefix, not an at-rule
// grammar. Keep the admitted UTF-8-only subset narrower than native fallback
// encoding detection; still let the tokenizer inspect and charge every token.
fn utf8_header(source: &str) -> bool {
    source.strip_prefix("@charset \"").is_some_and(|rest| {
        rest.as_bytes()
            .get(..7)
            .is_some_and(|tail| tail[..5].eq_ignore_ascii_case(b"utf-8") && &tail[5..] == b"\";")
    })
}

impl Budget {
    fn token<E>(&mut self) -> Result<(), Error<E>> {
        self.tokens = self.tokens.checked_sub(1).ok_or(Error::Limit("tokens"))?;
        Ok(())
    }

    fn text<E>(
        &mut self,
        kind: TextKind,
        value: &str,
        consumer: &mut impl FnMut(TextKind, &str) -> Result<(), E>,
    ) -> Result<(), Error<E>> {
        self.decoded = self
            .decoded
            .checked_sub(value.len())
            .ok_or(Error::Limit("decoded bytes"))?;
        consumer(kind, value).map_err(Error::Consumer)
    }
}

fn walk<'i, E: 'i>(
    parser: &mut Parser<'i, '_>,
    budget: &mut Budget,
    depth: usize,
    consumer: &mut impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<SourcePosition, Error<E>> {
    loop {
        let start = parser.position();
        let token = match parser.next_including_whitespace_and_comments() {
            Ok(token) => token.clone(),
            Err(error) => {
                return match error.kind {
                    cssparser::BasicParseErrorKind::EndOfInput => Ok(parser.position()),
                    _ => Err(Error::Malformed),
                };
            }
        };
        budget.token()?;
        let raw = parser.slice_from(start);
        let close = match &token {
            Token::Function(_) | Token::ParenthesisBlock => Some(")"),
            Token::SquareBracketBlock => Some("]"),
            Token::CurlyBracketBlock => Some("}"),
            _ => None,
        };
        match &token {
            Token::AtKeyword(value) if value.eq_ignore_ascii_case("charset") => {
                if !budget.utf8_header || start.byte_index() != 0 || raw != "@charset" {
                    return Err(Error::Unsupported);
                }
            }
            Token::BadString(_)
            | Token::BadUrl(_)
            | Token::CloseParenthesis
            | Token::CloseSquareBracket
            | Token::CloseCurlyBracket => return Err(Error::Malformed),
            Token::QuotedString(_) if !terminated(raw, raw.as_bytes()[0], 2) => {
                return Err(Error::Malformed);
            }
            Token::UnquotedUrl(_) if !terminated(raw, b')', 1) => return Err(Error::Malformed),
            Token::Comment(_) if raw.len() < 4 || !raw.ends_with("*/") => {
                return Err(Error::Malformed);
            }
            _ => {}
        }
        // A final escaped EOF becomes a replacement character in native name tokens.
        // Refuse that repair; complete escapes remain entirely native-decoded.
        if !matches!(token, Token::Comment(_))
            && !trailing_slashes(raw.as_bytes()).is_multiple_of(2)
        {
            return Err(Error::Malformed);
        }
        match &token {
            Token::Ident(value)
            | Token::AtKeyword(value)
            | Token::Hash(value)
            | Token::IDHash(value)
            | Token::Function(value) => budget.text(TextKind::Identifier, value, consumer)?,
            Token::Dimension { unit, .. } => budget.text(TextKind::Identifier, unit, consumer)?,
            Token::QuotedString(value) => budget.text(TextKind::String, value, consumer)?,
            Token::UnquotedUrl(value) => budget.text(TextKind::Url, value, consumer)?,
            _ => {}
        }
        if let Some(close) = close {
            if depth >= budget.depth {
                return Err(Error::Limit("nesting depth"));
            }
            let end = parser
                .parse_nested_block(|nested| {
                    walk(nested, budget, depth + 1, consumer)
                        .map_err(|error| nested.new_custom_error(error))
                })
                .map_err(|error| match error.kind {
                    ParseErrorKind::Custom(error) => error,
                    ParseErrorKind::Basic(_) => Error::Malformed,
                })?;
            // Native nested parsing treats EOF as an implicit close. The callback's
            // position is before the actual delimiter, not before the nested content.
            if parser.slice_from(end) != close {
                return Err(Error::Malformed);
            }
            budget.token()?;
        }
    }
}

fn trailing_slashes(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
}

fn terminated(raw: &str, delimiter: u8, minimum: usize) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= minimum
        && bytes.last() == Some(&delimiter)
        && trailing_slashes(&bytes[..bytes.len() - 1]).is_multiple_of(2)
}
