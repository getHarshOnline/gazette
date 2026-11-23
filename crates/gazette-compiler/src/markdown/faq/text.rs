//! Decode words without introducing spaces at inline emphasis or link boundaries.

use crate::html::{self, Limits, TextKind};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
        states::RawKind,
    },
};
use std::cell::RefCell;

struct Text {
    value: String,
    remaining: usize,
    tokens: usize,
    failed: bool,
}
impl Text {
    fn append(&mut self, text: &str) {
        if self.failed {
            return;
        }
        let Some(remaining) = self.remaining.checked_sub(text.len()) else {
            self.failed = true;
            return;
        };
        self.remaining = remaining;
        self.value.push_str(text);
    }
}
struct Sink(RefCell<Text>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failed {
            return TokenSinkResult::Continue;
        }
        let Some(tokens) = state.tokens.checked_sub(1) else {
            state.failed = true;
            return TokenSinkResult::Continue;
        };
        state.tokens = tokens;
        match token {
            Token::CharacterTokens(value) => state.append(&value),
            Token::TagToken(tag) => {
                let name = tag.name.as_ref();
                // Native block boundaries separate words even without source
                // whitespace. Inline markup never splits a word on its own.
                if matches!(
                    name,
                    "address"
                        | "article"
                        | "aside"
                        | "blockquote"
                        | "br"
                        | "caption"
                        | "dd"
                        | "details"
                        | "div"
                        | "dl"
                        | "dt"
                        | "fieldset"
                        | "figcaption"
                        | "figure"
                        | "footer"
                        | "form"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "header"
                        | "hr"
                        | "legend"
                        | "li"
                        | "main"
                        | "nav"
                        | "ol"
                        | "p"
                        | "pre"
                        | "section"
                        | "summary"
                        | "table"
                        | "tbody"
                        | "td"
                        | "tfoot"
                        | "th"
                        | "thead"
                        | "tr"
                        | "ul"
                ) {
                    state.append(" ");
                }
                if tag.kind == TagKind::StartTag && matches!(name, "title" | "textarea") {
                    return TokenSinkResult::RawData(RawKind::Rcdata);
                }
            }
            Token::ParseError(_) | Token::NullCharacterToken => state.failed = true,
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

pub(super) fn decode(source: &str, limits: Limits) -> Result<String, ()> {
    // Reuse the existing native admission and speculative-buffer bounds first.
    // Executable, style and embedded-data bodies are not FAQ reading content.
    html::inspect(source.as_bytes(), limits, |kind, _| match kind {
        TextKind::Text | TextKind::Attribute | TextKind::StyleAttribute => Ok(()),
        _ => Err(()),
    })
    .map_err(|_| ())?;
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(Text {
            value: String::new(),
            remaining: limits.decoded_bytes,
            tokens: limits.tokens,
            failed: false,
        })),
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    let mut start = 0;
    while start < source.len() {
        let mut end = source.len().min(start.saturating_add(256));
        while !source.is_char_boundary(end) {
            end -= 1;
        }
        input.push_back(StrTendril::from_slice(&source[start..end]));
        let _ = tokenizer.feed(&input);
        if tokenizer.sink.0.borrow().failed {
            return Err(());
        }
        start = end;
    }
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if state.failed || !input.is_empty() {
        return Err(());
    }
    Ok(state.value)
}
