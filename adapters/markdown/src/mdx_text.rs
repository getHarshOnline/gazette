//! Admitted reading text from the same native tree used for provider transport.

use crate::{bounded::Output, inspect, source::Lines};
use comrak::nodes::{Node, NodeValue};
use gazette_core::markdown::{Error, Limits, Syntax};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
};
use std::{cell::RefCell, collections::BTreeMap, fmt::Write};

pub(crate) fn collect(
    root: Node<'_>,
    syntax: &Syntax,
    lines: &Lines<'_>,
    limits: Limits,
) -> Result<String, Error> {
    let mut prefixes = BTreeMap::new();
    let mut remaining = limits.output_bytes;
    let mut markers = std::collections::BTreeSet::new();
    for node in root.descendants() {
        let data = node.data();
        if let NodeValue::Text(text) = &data.value
            && let Some(label) =
                crate::alerts::source_label(node, text, lines.location(data.sourcepos)?, lines)
        {
            let quote = node
                .parent()
                .and_then(|paragraph| paragraph.parent())
                .ok_or(Error::InvalidSource)?;
            remaining = remaining
                .checked_sub(label.len())
                .ok_or(Error::Limit("transport text bytes"))?;
            prefixes.insert(std::ptr::from_ref(quote).addr(), label.to_owned());
            markers.insert(std::ptr::from_ref(node).addr());
        }
        let literal = match &data.value {
            NodeValue::HtmlBlock(value) => Some(value.literal.as_str()),
            NodeValue::HtmlInline(value) => Some(value.as_str()),
            _ => None,
        };
        if let Some(literal) = literal {
            let value = html(literal, remaining)?;
            remaining = remaining
                .checked_sub(value.len())
                .ok_or(Error::Limit("transport text bytes"))?;
            if !value.is_empty() {
                prefixes.insert(std::ptr::from_ref(node).addr(), value);
            }
        }
    }
    for layout in crate::mdx::layout::plan(root, syntax, lines)? {
        if let Some(title) = layout.reading_title() {
            remaining = remaining
                .checked_sub(title.len())
                .ok_or(Error::Limit("transport text bytes"))?;
            prefixes.insert(std::ptr::from_ref(layout.node).addr(), title.to_owned());
        }
    }

    let unused: std::collections::BTreeSet<_> = syntax
        .footnotes
        .iter()
        .filter(|note| note.references == 0)
        .map(|note| note.name.as_str())
        .collect();
    inspect::text_filtered(
        root,
        limits.output_bytes,
        |node| {
            if markers.contains(&std::ptr::from_ref(node).addr()) {
                return Ok(true);
            }
            if let NodeValue::FootnoteDefinition(definition) = &node.data().value {
                return Ok(unused.contains(definition.name.as_str()));
            }
            Ok(false)
        },
        |node| {
            Ok(prefixes
                .get(&std::ptr::from_ref(node).addr())
                .map(String::as_str))
        },
    )
}

struct Sink(RefCell<(Output, bool)>);
impl TokenSink for Sink {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if !state.1 {
            match token {
                Token::CharacterTokens(text) => {
                    if state.0.write_str(&text).is_err() {
                        state.1 = true;
                    }
                }
                Token::TagToken(tag)
                    if matches!(
                        tag.name.as_ref(),
                        "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    ) =>
                {
                    if state.0.write_char('\n').is_err() {
                        state.1 = true;
                    }
                }
                _ => {}
            }
        }
        TokenSinkResult::Continue
    }
}
fn html(source: &str, limit: usize) -> Result<String, Error> {
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new((Output::new(limit), false))),
        TokenizerOpts::default(),
    );
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from(source));
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    let (output, failed) = tokenizer.sink.0.into_inner();
    if failed {
        Err(output.error())
    } else {
        Ok(output.value)
    }
}
