//! Bounded HTML token decoding for disclosure inspection, not DOM validation.

use gazette_core::mathml::{self, Attribute, Validator};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
        states::RawKind,
    },
};
use std::{cell::RefCell, fmt};

mod svg;

/// Exact token-decoding profile and its pinned parser; not browser qualification.
pub const PROFILE: &str = "gazette-html-token-probe/6;html5ever=0.35.0";

/// Bounds for one HTML candidate, before and during native tokenization.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum UTF-8 input bytes, checked before tokenizer allocation.
    pub input_bytes: usize,
    /// Maximum bytes fed without any token emission. Tokenizer buffering may
    /// include one additional 256-byte feed chunk; this is not an exact token length.
    pub unemitted_bytes: usize,
    /// Maximum tokenizer callbacks, including character fragments.
    pub tokens: usize,
    /// Aggregate decoded text and attribute bytes passed to the consumer.
    pub decoded_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            input_bytes: 1 << 20,
            unemitted_bytes: 16 << 10,
            tokens: 100_000,
            decoded_bytes: 2 << 20,
        }
    }
}

/// Native context of one complete text run or attribute value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextKind {
    /// Entity-decoded HTML data or title/textarea text, joined across token chunks.
    Text,
    /// One entity-decoded attribute value; names remain raw inventory evidence.
    Attribute,
    /// Entity-decoded style attribute, still requiring CSS decoding.
    StyleAttribute,
    /// Entity-decoded value whose native normalized attribute name starts with `on`.
    /// This conservative prefix profile does not enumerate browser event handlers.
    EventAttribute,
    /// Inline classic JavaScript text, still requiring JavaScript inspection.
    Script,
    /// Inline module JavaScript text, still requiring module-goal inspection.
    ModuleScript,
    /// Uninterpreted non-JavaScript data, including data declarations with `src`.
    /// Empty bodies are emitted too; absence of bytes does not qualify the declared class.
    DataScript,
    /// A script declared as classic/module JavaScript with a `src` attribute.
    /// Its uninterpreted inline body is emitted even when empty. Neither the
    /// reference nor this body establishes inspection of the external resource.
    ExternalScript,
    /// Inline application/json, application/ld+json, importmap or speculationrules
    /// text, still needing JSON grammar inspection, not schema validation.
    JsonScript,
    /// CSS style text (absent/empty/text/css type), without entity decoding or CSS interpretation.
    Style,
}

/// Failure of token decoding or its speculative consumer.
#[derive(Debug)]
pub enum Error<E> {
    /// A declared resource bound stopped inspection.
    Limit(&'static str),
    /// HTML must be UTF-8; no lossy decoding or declared-encoding substitution.
    Utf8,
    /// The tokenizer left caller input unconsumed; never finalize a shortened input.
    Incomplete,
    /// Tokenization reported an error, including a duplicate attribute.
    Malformed {
        /// Tokenizer-reported source line, or zero for a Markdown fact boundary; never source payload.
        line: u64,
    },
    /// The profile does not implement this context-sensitive or foreign-content case.
    Unsupported {
        /// Tokenizer-reported source line, or zero for a Markdown fact boundary.
        line: u64,
    },
    /// The consuming scanner stopped; no further input is fed.
    Consumer(E),
}
impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(name) => write!(f, "HTML {name} ceiling exceeded"),
            Self::Utf8 => f.write_str("HTML candidate is not UTF-8"),
            Self::Incomplete => f.write_str("HTML tokenizer left input unconsumed"),
            Self::Malformed { line } => write!(f, "HTML token error at line {line}"),
            Self::Unsupported { line } => write!(f, "unsupported HTML context at line {line}"),
            Self::Consumer(error) => error.fmt(f),
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

/// Decode HTML text/attributes without building a DOM or changing the input.
///
/// Consumer calls are speculative until success; discard their effects on any
/// later error. This profile rejects every tokenizer error, unsupported foreign
/// content, raw/scripting contexts and non-HTML doctypes. One bounded `MathML`
/// presentation profile shares Gazette's retained-typesetting invariant. A finite
/// inert SVG profile permits path geometry and plain title/description text;
/// URL/style/event attributes, active elements and integration markup are refused.
/// Scripts/styles are
/// delivered in separate raw channels. HTML tree conformance, browser repair,
/// cross-element text concatenation and executable behavior are not validated.
/// Script classification follows decoded type/language attributes, not body
/// contents or execution eligibility. A src attribute leaves a recognized JavaScript
/// body's text in the external-script channel; other declarations remain data.
/// Observing the tag does not inspect an external file.
///
/// # Errors
/// Rejects UTF-8, tokenizer/context and resource failures, or propagates the consumer.
/// Quotas bound input and callback work, not wall-clock time or exact tokenizer
/// allocations. Native attribute deduplication can be quadratic within a tag;
/// bounded feeds and unemitted-input limits constrain buffered tags.
pub fn inspect<E>(
    bytes: &[u8],
    limits: Limits,
    consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
    inspect_parts(
        &[Part::Html(text.strip_prefix('\u{feff}').unwrap_or(text))],
        limits,
        consumer,
        false,
    )
}

/// Inspect native Markdown HTML in its ordered textual context. Inline tags may
/// span several parser facts; one scanner retains their finite foreign-content state.
/// Markdown text/code remains text, including HTML-looking code and unused notes.
/// Complete raw blocks retain the same script/style decoding as [`inspect`].
/// Split raw-text contexts are refused rather than assigning Markdown text a
/// different scripting meaning. This is a disclosure projection, not rendering.
///
/// # Errors
/// Rejects inconsistent native facts, unfinished/unsupported HTML and resource
/// limits, including the aggregate raw/decoded input bound, or the consumer's error.
/// Each raw fact must be lexically complete; incomplete tags cannot consume a
/// following Markdown text piece. Error line zero marks Markdown projection;
/// fragment-relative tokenizer lines are not reported as full-file coordinates.
pub fn inspect_markdown<E>(
    syntax: &gazette_core::markdown::Syntax,
    limits: Limits,
    consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    if syntax.text.len() > limits.tokens || syntax.html.len() > limits.tokens {
        return Err(Error::Limit("tokens"));
    }
    let mut html = syntax.html.iter().peekable();
    let mut parts = Vec::with_capacity(syntax.text.len());
    for piece in &syntax.text {
        if html
            .peek()
            .is_some_and(|next| next.location == piece.location && next.literal == piece.value)
        {
            html.next();
            parts.push(Part::Html(&piece.value));
        } else {
            parts.push(Part::Text(&piece.value));
        }
    }
    if html.next().is_some() {
        return Err(Error::Incomplete);
    }
    inspect_parts(&parts, limits, consumer, true).map_err(|error| match error {
        Error::Malformed { .. } => Error::Malformed { line: 0 },
        Error::Unsupported { .. } => Error::Unsupported { line: 0 },
        other => other,
    })
}

#[derive(Clone, Copy)]
enum Part<'a> {
    Html(&'a str),
    Text(&'a str),
}

pub(crate) fn inspect_generated<E>(
    bytes: &[u8],
    literals: &[crate::generated_mdx::Literal],
    limits: Limits,
    consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
) -> Result<(), Error<E>> {
    if bytes.len() > limits.input_bytes || literals.len() > limits.tokens {
        return Err(Error::Limit("generated input"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| Error::Utf8)?;
    let mut parts = Vec::new();
    let mut start = 0;
    for literal in literals {
        let before = source.get(start..literal.offset).ok_or(Error::Incomplete)?;
        parts.push(Part::Html(before));
        parts.push(Part::Text(&literal.value));
        start = literal.offset;
    }
    parts.push(Part::Html(source.get(start..).ok_or(Error::Incomplete)?));
    inspect_parts(&parts, limits, consumer, true)
}

fn inspect_parts<E>(
    parts: &[Part<'_>],
    limits: Limits,
    consumer: impl FnMut(TextKind, &str) -> Result<(), E>,
    fragments: bool,
) -> Result<(), Error<E>> {
    let mut input = 0usize;
    for part in parts {
        let value = match part {
            Part::Html(value) | Part::Text(value) => value,
        };
        input = input
            .checked_add(value.len())
            .filter(|&size| size <= limits.input_bytes)
            .ok_or(Error::Limit("input bytes"))?;
    }
    let sink = Sink(RefCell::new(State {
        consumer,
        failure: None,
        limits,
        tokens: 0,
        decoded: 0,
        text: String::new(),
        context: Context::Data,
        mathml: None,
        svg: None,
        fragments,
    }));
    for part in parts {
        match part {
            Part::Html(text) => tokenize(&sink, text, limits)?,
            Part::Text(text) => {
                let mut state = sink.0.borrow_mut();
                if state.context != Context::Data {
                    return Err(Error::Incomplete);
                }
                // These are already decoded native text facts. Sending them
                // as character tokens preserves code as text, not active HTML.
                let _ = state.token(Token::CharacterTokens(StrTendril::from_slice(text)), 0)?;
            }
        }
    }
    let mut state = sink.0.borrow_mut();
    if fragments {
        state.fragments = false;
        let _ = state.token(Token::EOFToken, 0)?;
    }
    Ok(())
}

fn tokenize<S: FnMut(TextKind, &str) -> Result<(), E>, E>(
    sink: &Sink<S, E>,
    text: &str,
    limits: Limits,
) -> Result<(), Error<E>> {
    let tokenizer = Tokenizer::new(
        sink,
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let queue = BufferQueue::default();
    let mut offset = 0;
    let mut unemitted = 0usize;
    while offset < text.len() {
        let mut end = text.len().min(offset.saturating_add(256));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        unemitted = unemitted
            .checked_add(end - offset)
            .filter(|&size| size <= limits.unemitted_bytes)
            .ok_or(Error::Limit("unemitted input bytes"))?;
        let before = sink.0.borrow().tokens;
        queue.push_back(StrTendril::from_slice(&text[offset..end]));
        let _ = tokenizer.feed(&queue);
        let mut state = sink.0.borrow_mut();
        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        if state.tokens != before {
            unemitted = 0;
        }
        offset = end;
    }
    if !queue.is_empty() {
        return Err(Error::Incomplete);
    }
    tokenizer.end();
    match sink.0.borrow_mut().failure.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Data,
    Rcdata,
    Script,
    ModuleScript,
    DataScript,
    ExternalScript,
    JsonScript,
    Style,
}
impl Context {
    fn kind(self) -> TextKind {
        match self {
            Self::Data | Self::Rcdata => TextKind::Text,
            Self::Script => TextKind::Script,
            Self::ModuleScript => TextKind::ModuleScript,
            Self::DataScript => TextKind::DataScript,
            Self::ExternalScript => TextKind::ExternalScript,
            Self::JsonScript => TextKind::JsonScript,
            Self::Style => TextKind::Style,
        }
    }
}

// WHATWG prepare-the-script-element steps 8–13 use essence *string* matching,
// not MIME parsing: parameters do not match, and language is never trimmed.
// https://html.spec.whatwg.org/multipage/scripting.html#prepare-the-script-element
// https://mimesniff.spec.whatwg.org/#javascript-mime-type
const JAVASCRIPT_TYPES: [&str; 16] = [
    "application/ecmascript",
    "application/javascript",
    "application/x-ecmascript",
    "application/x-javascript",
    "text/ecmascript",
    "text/javascript",
    "text/javascript1.0",
    "text/javascript1.1",
    "text/javascript1.2",
    "text/javascript1.3",
    "text/javascript1.4",
    "text/javascript1.5",
    "text/jscript",
    "text/livescript",
    "text/x-ecmascript",
    "text/x-javascript",
];

fn trim_ascii_whitespace(value: &str) -> &str {
    value.trim_matches(['\t', '\n', '\u{c}', '\r', ' '])
}

fn script_context(tag: &Tag) -> Context {
    let declared = declared_script_context(tag);
    if tag
        .attrs
        .iter()
        .any(|attribute| attribute.name.local.as_ref() == "src")
    {
        if matches!(declared, Context::Script | Context::ModuleScript) {
            Context::ExternalScript
        } else {
            Context::DataScript
        }
    } else {
        declared
    }
}

fn declared_script_context(tag: &Tag) -> Context {
    let attribute = |name| {
        tag.attrs
            .iter()
            .find(|attribute| attribute.name.local.as_ref() == name)
            .map(|attribute| attribute.value.as_ref())
    };
    let Some(kind) = attribute("type") else {
        let language = attribute("language").unwrap_or("");
        return if language.is_empty()
            || JAVASCRIPT_TYPES.iter().any(|kind| {
                kind.strip_prefix("text/")
                    .is_some_and(|suffix| suffix.eq_ignore_ascii_case(language))
            }) {
            Context::Script
        } else {
            Context::DataScript
        };
    };
    if kind.is_empty() {
        return Context::Script;
    }
    let kind = trim_ascii_whitespace(kind);
    if JAVASCRIPT_TYPES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(kind))
    {
        Context::Script
    } else if kind.eq_ignore_ascii_case("module") {
        Context::ModuleScript
    } else if [
        "application/json",
        "application/ld+json",
        "importmap",
        "speculationrules",
    ]
    .iter()
    .any(|candidate| candidate.eq_ignore_ascii_case(kind))
    {
        Context::JsonScript
    } else {
        Context::DataScript
    }
}

struct State<S, E> {
    consumer: S,
    failure: Option<Error<E>>,
    limits: Limits,
    tokens: usize,
    decoded: usize,
    text: String,
    context: Context,
    mathml: Option<Validator>,
    svg: Option<svg::Validator>,
    fragments: bool,
}
struct Sink<S, E>(RefCell<State<S, E>>);
impl<S: FnMut(TextKind, &str) -> Result<(), E>, E> State<S, E> {
    fn charge(&mut self, bytes: usize) -> Result<(), Error<E>> {
        self.decoded = self
            .decoded
            .checked_add(bytes)
            .filter(|&size| size <= self.limits.decoded_bytes)
            .ok_or(Error::Limit("decoded bytes"))?;
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Error<E>> {
        if !self.text.is_empty()
            || matches!(
                self.context,
                Context::JsonScript | Context::DataScript | Context::ExternalScript
            )
        {
            (self.consumer)(self.context.kind(), &self.text).map_err(Error::Consumer)?;
            self.text.clear();
        }
        Ok(())
    }
    fn attributes(&mut self, tag: &Tag) -> Result<(), Error<E>> {
        for attribute in &tag.attrs {
            self.charge(attribute.value.len())?;
            let kind = if attribute.name.local.as_ref() == "style" {
                TextKind::StyleAttribute
            } else if attribute.name.local.as_ref().starts_with("on") {
                TextKind::EventAttribute
            } else {
                TextKind::Attribute
            };
            (self.consumer)(kind, &attribute.value).map_err(Error::Consumer)?;
        }
        Ok(())
    }
    fn token(&mut self, token: Token, line: u64) -> Result<TokenSinkResult<()>, Error<E>> {
        self.tokens = self
            .tokens
            .checked_add(1)
            .filter(|&count| count <= self.limits.tokens)
            .ok_or(Error::Limit("tokens"))?;
        match token {
            Token::ParseError(_) | Token::NullCharacterToken => {
                return Err(Error::Malformed { line });
            }
            Token::CharacterTokens(value) => {
                if self.svg.as_ref().is_some_and(|svg| !svg.text(&value)) {
                    return Err(Error::Unsupported { line });
                }
                if let Some(mathml) = &mut self.mathml {
                    mathml
                        .text(&value)
                        .map_err(|error| math_error(error, line))?;
                }
                self.charge(value.len())?;
                self.text.push_str(&value);
            }
            Token::TagToken(tag) => {
                self.flush()?;
                if tag.kind == TagKind::EndTag {
                    if let Some(svg) = &mut self.svg {
                        if !tag.attrs.is_empty() || tag.self_closing {
                            return Err(Error::Unsupported { line });
                        }
                        if svg
                            .end(tag.name.as_ref())
                            .ok_or(Error::Unsupported { line })?
                        {
                            self.svg = None;
                        }
                        return Ok(TokenSinkResult::Continue);
                    }
                    if let Some(mathml) = &mut self.mathml {
                        if !tag.attrs.is_empty() || tag.self_closing {
                            return Err(Error::Unsupported { line });
                        }
                        mathml
                            .end(tag.name.as_ref())
                            .map_err(|error| math_error(error, line))?;
                        if mathml.root_closed() {
                            let complete = self.mathml.take().expect("checked MathML state");
                            complete.finish().map_err(|error| math_error(error, line))?;
                        }
                        self.context = Context::Data;
                        return Ok(TokenSinkResult::Continue);
                    }
                    if matches!(tag.name.as_ref(), "math" | "svg") {
                        return Err(Error::Unsupported { line });
                    }
                    self.context = Context::Data;
                    return Ok(TokenSinkResult::Continue);
                }

                let name = tag.name.as_ref();
                if self.svg.is_some() || name == "svg" {
                    if self.mathml.is_some() {
                        return Err(Error::Unsupported { line });
                    }
                    if let Some(svg) = &mut self.svg {
                        if !svg.start(&tag) {
                            return Err(Error::Unsupported { line });
                        }
                    } else {
                        let svg = svg::Validator::root(&tag).ok_or(Error::Unsupported { line })?;
                        if !tag.self_closing {
                            self.svg = Some(svg);
                        }
                    }
                    self.attributes(&tag)?;
                    return Ok(TokenSinkResult::Continue);
                }
                if self.mathml.is_some() || name == "math" {
                    let root = self.mathml.is_none();
                    let namespace_count = tag
                        .attrs
                        .iter()
                        .filter(|attribute| attribute.name.local.as_ref() == "xmlns")
                        .count();
                    if root {
                        if namespace_count != 1
                            || tag.attrs.iter().any(|attribute| {
                                attribute.name.local.as_ref() == "xmlns"
                                    && attribute.value.as_ref() != mathml::NAMESPACE
                            })
                        {
                            return Err(Error::Unsupported { line });
                        }
                        self.mathml = Some(Validator::new(None, None));
                    } else if namespace_count != 0 {
                        return Err(Error::Unsupported { line });
                    }
                    let attributes = tag
                        .attrs
                        .iter()
                        .filter(|attribute| attribute.name.local.as_ref() != "xmlns")
                        .map(|attribute| Attribute {
                            name: attribute.name.local.as_ref(),
                            value: attribute.value.as_ref(),
                        })
                        .collect::<Vec<_>>();
                    self.mathml
                        .as_mut()
                        .expect("created MathML state")
                        .start(name, &attributes)
                        .map_err(|error| math_error(error, line))?;
                    self.attributes(&tag)?;
                    if tag.self_closing {
                        self.mathml
                            .as_mut()
                            .expect("active MathML state")
                            .end(name)
                            .map_err(|error| math_error(error, line))?;
                    }
                    return Ok(TokenSinkResult::Continue);
                }
                self.attributes(&tag)?;
                let raw = match name {
                    "script" => {
                        self.context = script_context(&tag);
                        Some(RawKind::ScriptData)
                    }
                    "style" => {
                        if tag.attrs.iter().any(|attribute| {
                            attribute.name.local.as_ref() == "type"
                                && !trim_ascii_whitespace(&attribute.value).is_empty()
                                && !trim_ascii_whitespace(&attribute.value)
                                    .eq_ignore_ascii_case("text/css")
                        }) {
                            return Err(Error::Unsupported { line });
                        }
                        self.context = Context::Style;
                        Some(RawKind::Rawtext)
                    }
                    "title" | "textarea" => {
                        self.context = Context::Rcdata;
                        Some(RawKind::Rcdata)
                    }
                    "noscript" | "plaintext" | "xmp" | "iframe" | "noembed" | "noframes" => {
                        return Err(Error::Unsupported { line });
                    }
                    _ => None,
                };
                if let Some(raw) = raw {
                    if tag.self_closing {
                        return Err(Error::Unsupported { line });
                    }
                    return Ok(TokenSinkResult::RawData(raw));
                }
            }
            Token::CommentToken(_) => {
                if self.mathml.is_some() || self.svg.is_some() {
                    return Err(Error::Unsupported { line });
                }
                self.flush()?;
            }
            Token::DoctypeToken(value) => {
                self.flush()?;
                if self.svg.is_some()
                    || value.name.as_deref() != Some("html")
                    || value.public_id.is_some()
                    || value.system_id.is_some()
                    || value.force_quirks
                {
                    return Err(Error::Unsupported { line });
                }
            }
            Token::EOFToken => {
                if self.context != Context::Data
                    || (!self.fragments && (self.mathml.is_some() || self.svg.is_some()))
                {
                    return Err(Error::Malformed { line });
                }
                self.flush()?;
            }
        }
        Ok(TokenSinkResult::Continue)
    }
}

fn math_error<E>(error: mathml::Error, line: u64) -> Error<E> {
    match error {
        mathml::Error::Invalid => Error::Unsupported { line },
        mathml::Error::Limit(resource) => Error::Limit(resource),
    }
}
impl<S: FnMut(TextKind, &str) -> Result<(), E>, E> TokenSink for Sink<S, E> {
    type Handle = ();
    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        match state.token(token, line) {
            Ok(result) => result,
            Err(error) => {
                state.failure = Some(error);
                TokenSinkResult::Continue
            }
        }
    }
}

// Borrow one scanner state across lexically complete Markdown HTML facts.
impl<S: FnMut(TextKind, &str) -> Result<(), E>, E> TokenSink for &Sink<S, E> {
    type Handle = ();
    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<()> {
        <Sink<S, E> as TokenSink>::process_token(self, token, line)
    }
}
