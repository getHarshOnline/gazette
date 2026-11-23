//! Inert stored-HTML projection. This is a rejecting profile, not an HTML sanitizer.

use std::{cell::RefCell, io::Write};

use gazette_core::render::LimitedWriter;
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};

use super::render::RenderError;
use crate::markdown::Limits;

struct Frame {
    name: String,
    // Table children have explicit order: caption, thead, tbody(s), tfoot.
    table_phase: u8,
}

struct State {
    cell: usize,
    limits: Limits,
    tokens: usize,
    decoded: usize,
    stack: Vec<Frame>,
    html: LimitedWriter<Vec<u8>>,
    text: LimitedWriter<Vec<u8>>,
    failure: Option<RenderError>,
}

fn phrasing(name: &str) -> bool {
    matches!(
        name,
        "span" | "strong" | "em" | "b" | "i" | "u" | "s" | "small" | "sub" | "sup" | "code" | "br"
    )
}

fn flow(name: &str) -> bool {
    phrasing(name)
        || matches!(
            name,
            "div" | "p" | "blockquote" | "ul" | "ol" | "table" | "hr"
        )
}

fn void(name: &str) -> bool {
    matches!(name, "br" | "hr")
}

fn structural(name: &str) -> bool {
    matches!(
        name,
        "table" | "thead" | "tbody" | "tfoot" | "tr" | "ul" | "ol"
    )
}

impl State {
    fn unsupported(&self) -> RenderError {
        RenderError::Unsupported {
            cell: Some(self.cell),
            feature: "HTML representation",
        }
    }

    fn markup(&mut self, text: &str) -> Result<(), RenderError> {
        self.html
            .write_all(text.as_bytes())
            .map_err(|_| RenderError::Limit("stored HTML output bytes"))
    }

    fn plain(&mut self, text: &str) -> Result<(), RenderError> {
        self.text
            .write_all(text.as_bytes())
            .map_err(|_| RenderError::Limit("stored HTML text bytes"))
    }

    fn charge(&mut self, bytes: usize) -> Result<(), RenderError> {
        self.decoded = self
            .decoded
            .checked_add(bytes)
            .filter(|count| *count <= self.limits.fact_bytes)
            .ok_or(RenderError::Limit("stored HTML decoded bytes"))?;
        Ok(())
    }

    fn start(&mut self, tag: Tag) -> Result<(), RenderError> {
        let name = tag.name.as_ref();
        let parent = self.stack.last().map(|frame| frame.name.as_str());
        let allowed = match parent {
            Some("table") => matches!(name, "caption" | "thead" | "tbody" | "tfoot"),
            Some("thead" | "tbody" | "tfoot") => name == "tr",
            Some("tr") => matches!(name, "td" | "th"),
            Some("ul" | "ol") => name == "li",
            Some("p" | "caption") => phrasing(name),
            Some(parent) if phrasing(parent) => phrasing(name),
            _ => flow(name),
        };
        if !allowed
            || (tag.self_closing && !void(name))
            || (name == "table" && self.stack.iter().any(|frame| frame.name == "table"))
        {
            return Err(self.unsupported());
        }
        if parent == Some("table") {
            let phase = match name {
                "caption" => 1,
                "thead" => 2,
                "tbody" => 3,
                "tfoot" => 4,
                _ => unreachable!(),
            };
            let table = self.stack.last_mut().ok_or(RenderError::Structure)?;
            if phase < table.table_phase || (phase == table.table_phase && phase != 3) {
                return Err(self.unsupported());
            }
            table.table_phase = phase;
        }
        if !void(name) && self.stack.len() >= self.limits.depth {
            return Err(RenderError::Limit("stored HTML depth"));
        }
        self.markup("<")?;
        self.markup(name)?;
        for attribute in &tag.attrs {
            let key = attribute.name.local.as_ref();
            let value = attribute.value.as_ref();
            self.charge(value.len())?;
            let valid = attribute.name.ns.is_empty()
                && attribute.name.prefix.is_none()
                && match key {
                    // Classes preserve source-owned presentation hooks; they grant no
                    // style/resource admission and cannot introduce markup or IDs.
                    "class" => {
                        !value.is_empty()
                            && value.bytes().all(|byte| {
                                byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'_' | b'-')
                            })
                    }
                    "colspan" | "rowspan" if matches!(name, "td" | "th") => {
                        !value.is_empty()
                            && value.bytes().all(|byte| byte.is_ascii_digit())
                            && value
                                .parse::<u16>()
                                .is_ok_and(|value| (1..=1000).contains(&value))
                    }
                    "scope" if name == "th" => {
                        matches!(value, "row" | "col" | "rowgroup" | "colgroup")
                    }
                    _ => false,
                };
            if !valid {
                return Err(self.unsupported());
            }
            self.markup(" ")?;
            self.markup(key)?;
            self.markup("=\"")?;
            self.markup(&quick_xml::escape::escape(value))?;
            self.markup("\"")?;
        }
        self.markup(">")?;
        if !phrasing(name) || name == "br" {
            self.plain("\n")?;
        }
        if !void(name) {
            self.stack.push(Frame {
                name: name.to_owned(),
                table_phase: 0,
            });
        }
        Ok(())
    }

    fn token(&mut self, token: Token) -> Result<(), RenderError> {
        self.tokens = self
            .tokens
            .checked_add(1)
            .filter(|count| *count <= self.limits.nodes)
            .ok_or(RenderError::Limit("stored HTML tokens"))?;
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => self.start(tag),
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                if tag.kind != TagKind::EndTag
                    || !tag.attrs.is_empty()
                    || tag.self_closing
                    || self.stack.last().map(|frame| frame.name.as_str()) != Some(name)
                {
                    return Err(self.unsupported());
                }
                self.stack.pop();
                self.markup("</")?;
                self.markup(name)?;
                self.markup(">")?;
                if !phrasing(name) {
                    self.plain("\n")?;
                }
                Ok(())
            }
            Token::CharacterTokens(text) => {
                self.charge(text.len())?;
                if self
                    .stack
                    .last()
                    .is_some_and(|frame| structural(&frame.name))
                    && !text
                        .chars()
                        .all(|value| matches!(value, ' ' | '\t' | '\n' | '\r' | '\u{c}'))
                {
                    return Err(self.unsupported());
                }
                self.markup(&quick_xml::escape::escape(&*text))?;
                self.plain(&text)
            }
            Token::EOFToken if self.stack.is_empty() => Ok(()),
            // Comments, declarations, parse errors and foreign/active contexts are
            // refused, never deleted to make a different document appear admitted.
            _ => Err(self.unsupported()),
        }
    }
}

struct Sink(RefCell<State>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_none()
            && let Err(error) = state.token(token)
        {
            state.failure = Some(error);
        }
        TokenSinkResult::Continue
    }
}

/// Render only the complete balanced inert profile. No native reads or script execution.
pub(super) fn render(
    source: &str,
    limits: Limits,
    cell: usize,
) -> Result<(String, String), RenderError> {
    if source.len() > limits.source_bytes {
        return Err(RenderError::Limit("stored HTML source bytes"));
    }
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(State {
            cell,
            limits,
            tokens: 0,
            decoded: 0,
            stack: Vec::new(),
            html: LimitedWriter::new(Vec::new(), limits.output_bytes),
            text: LimitedWriter::new(Vec::new(), limits.output_bytes),
            failure: None,
        })),
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let queue = BufferQueue::default();
    let mut offset = 0;
    // Bound native token buffering and attribute-deduplication work independently
    // of the complete notebook ceiling. A feed contributes at most 256 bytes.
    let mut unemitted = 0usize;
    while offset < source.len() {
        let mut end = source.len().min(offset.saturating_add(256));
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        unemitted = unemitted
            .checked_add(end - offset)
            .filter(|count| *count <= limits.source_bytes.min(16 << 10))
            .ok_or(RenderError::Limit("stored HTML unemitted input bytes"))?;
        let before = tokenizer.sink.0.borrow().tokens;
        queue.push_back(StrTendril::from_slice(&source[offset..end]));
        let _ = tokenizer.feed(&queue);
        let mut state = tokenizer.sink.0.borrow_mut();
        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        if state.tokens != before {
            unemitted = 0;
        }
        offset = end;
    }
    if !queue.is_empty() {
        return Err(RenderError::Structure);
    }
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if let Some(error) = state.failure {
        return Err(error);
    }
    Ok((
        String::from_utf8(state.html.into_inner()).map_err(|_| RenderError::Structure)?,
        String::from_utf8(state.text.into_inner()).map_err(|_| RenderError::Structure)?,
    ))
}
