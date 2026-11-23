//! Inert MDX transport over Comrak facts and native HTML tokens, never a JSX parser.

use crate::{
    alerts::{label as alert_label, source_label as source_alert_label},
    bounded::{Budget, Output},
    source::Lines,
};
use comrak::nodes::{Node, NodeHtmlBlock, NodeValue};
use gazette_core::{
    markdown::{Error, HeadingBinding, Limits, Profile, Syntax},
    source::{Location, Span},
};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[path = "mdx_layout.rs"]
pub(crate) mod layout;

fn module_prefix(text: &str) -> bool {
    let text = text.trim_start();
    text.starts_with("import") || text.starts_with("export")
}

// Mintlify names differ from the five source meanings; recognition is shared
// with publication rendering, while this mapping belongs to this transport.
fn take_callout_markers<'a>(
    root: Node<'a>,
    lines: &Lines<'_>,
) -> Result<Vec<(Node<'a>, &'static str)>, Error> {
    crate::alerts::take_markers(root, lines).map(|alerts| {
        alerts
            .into_iter()
            .map(|(quote, kind)| {
                let component = match kind {
                    comrak::nodes::AlertType::Note => "Note",
                    comrak::nodes::AlertType::Tip => "Tip",
                    comrak::nodes::AlertType::Important => "Info",
                    comrak::nodes::AlertType::Warning => "Warning",
                    comrak::nodes::AlertType::Caution => "Danger",
                };
                (quote, component)
            })
            .collect()
    })
}

fn wrap_callouts<'a>(
    arena: &'a comrak::Arena<'a>,
    callouts: Vec<(Node<'a>, &'static str)>,
    limit: usize,
) -> Result<(), Error> {
    for (quote, component) in callouts.into_iter().rev() {
        let mut opening = Output::new(limit);
        write!(opening, "<{component}>").map_err(|_| opening.error())?;
        let mut closing = Output::new(limit);
        write!(closing, "</{component}>").map_err(|_| closing.error())?;
        // Paragraph formatting keeps the active quote/list prefix on blank
        // lines too. Raw HTML blocks can split an enclosing ordinary quote.
        let start = arena.alloc(NodeValue::Paragraph.into());
        start.append(arena.alloc(NodeValue::HtmlInline(opening.value).into()));
        quote.insert_before(start);
        while let Some(child) = quote.first_child() {
            child.detach();
            quote.insert_before(child);
        }
        let end = arena.alloc(NodeValue::Paragraph.into());
        end.append(arena.alloc(NodeValue::HtmlInline(closing.value).into()));
        quote.insert_after(end);
        quote.detach();
    }
    Ok(())
}

pub(crate) fn needed(root: Node<'_>, profile: Profile) -> bool {
    root.descendants().any(|node| match &node.data().value {
        NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => true,
        NodeValue::Text(text) => {
            text.contains(['{', '}', '<', '>']) || module_prefix(text) || alert_label(node, text).is_some()
        }
        NodeValue::CodeBlock(code) => {
            !code.fenced || (profile == Profile::PublicationMath && code.info == "math")
        }
        NodeValue::BlockDirective(_) => true,
        NodeValue::Math(math) => !math.dollar_math || math.literal.contains('$'),
        NodeValue::Link(link) => node.first_child().is_some_and(|child| {
            matches!(&child.data().value, NodeValue::Text(text) if text == link.url.strip_prefix("mailto:").unwrap_or(&link.url))
        }),
        _ => false,
    })
}

fn heading_plan<'a>(
    syntax: &Syntax,
    headings: &'a [HeadingBinding],
    limit: usize,
) -> Result<BTreeMap<Span, &'a HeadingBinding>, Error> {
    let expected: BTreeMap<_, _> = syntax
        .headings
        .iter()
        .filter(|h| h.emitted)
        .map(|h| (h.location.span, h))
        .collect();
    if headings.len() != expected.len() {
        return Err(Error::InvalidPlan);
    }
    let mut selected = BTreeMap::new();
    let mut ids = BTreeSet::new();
    let mut budget = Budget(limit);
    for binding in headings {
        budget.charge(
            binding
                .id
                .len()
                .checked_add(5)
                .ok_or(Error::Limit("MDX heading bytes"))?,
        )?;
        if expected.get(&binding.occurrence.location.span).copied() != Some(&binding.occurrence)
            || binding.level != binding.occurrence.level
            || selected
                .insert(binding.occurrence.location.span, binding)
                .is_some()
            || !ids.insert(binding.id.as_str())
        {
            return Err(Error::InvalidPlan);
        }
        if binding.id.is_empty()
            || binding.id.chars().any(|c| {
                c.is_whitespace()
                    || c.is_control()
                    || matches!(c, '{' | '}' | '<' | '>' | '\\' | '"' | '\'' | '#')
            })
        {
            return Err(Error::Unsupported {
                location: binding.occurrence.location,
                reason: "heading ID cannot be represented in MDX custom-anchor syntax",
            });
        }
    }
    Ok(selected)
}

fn replace_title(
    root: Node<'_>,
    title: &HeadingBinding,
    lines: &Lines<'_>,
    limit: usize,
) -> Result<(), Error> {
    if title.level != 1 || title.occurrence.level != 1 {
        return Err(Error::InvalidPlan);
    }
    let heading = root
        .first_child()
        .filter(|node| {
            let data = node.data();
            matches!(data.value, NodeValue::Heading(_))
                && lines.location(data.sourcepos).ok() == Some(title.occurrence.location)
        })
        .ok_or(Error::InvalidPlan)?;
    if heading
        .descendants()
        .skip(1)
        .any(|node| !matches!(node.data().value, NodeValue::Text(_) | NodeValue::SoftBreak))
    {
        return Err(Error::Unsupported {
            location: title.occurrence.location,
            reason: "a separately rendered title cannot discard authored inline structure",
        });
    }
    let mut anchor = Output::new(limit);
    anchor.write_str("<a id=\"").map_err(|_| anchor.error())?;
    literal(&mut anchor, &title.id)?;
    anchor.write_str("\"></a>\n").map_err(|_| anchor.error())?;
    for child in heading.children() {
        child.detach();
    }
    heading.data_mut().value = NodeValue::HtmlBlock(NodeHtmlBlock {
        block_type: 6,
        literal: anchor.value,
    });
    Ok(())
}

fn text_transport(text: &str, limit: usize) -> Result<String, Error> {
    let escaped = if module_prefix(text) {
        let trimmed = text.trim_start();
        let prefix = &text[..text.len() - trimmed.len()];
        let mut out = Output::new(limit);
        out.write_str(&comrak::escape_commonmark_inline(prefix))
            .map_err(|_| out.error())?;
        write!(out, "&#{};", trimmed.as_bytes()[0]).map_err(|_| out.error())?;
        out.write_str(&comrak::escape_commonmark_inline(&trimmed[1..]))
            .map_err(|_| out.error())?;
        out.value
    } else {
        comrak::escape_commonmark_inline(text)
    };
    let mut output = Output::new(limit);
    for c in escaped.chars() {
        if matches!(c, '{' | '}' | '|') {
            output.write_char('\\').map_err(|_| output.error())?;
        }
        output.write_char(c).map_err(|_| output.error())?;
    }
    Ok(output.value)
}

fn display_math(literal: &str, limit: usize) -> Result<NodeValue, Error> {
    let mut output = Output::new(limit);
    output.write_str("$$").map_err(|_| output.error())?;
    if !literal.starts_with('\n') {
        output.write_char('\n').map_err(|_| output.error())?;
    }
    output.write_str(literal).map_err(|_| output.error())?;
    if !literal.ends_with('\n') {
        output.write_char('\n').map_err(|_| output.error())?;
    }
    output.write_str("$$\n").map_err(|_| output.error())?;
    Ok(NodeValue::HtmlBlock(NodeHtmlBlock {
        block_type: 6,
        literal: output.value,
    }))
}

fn math_delimiters_safe(literal: &str) -> bool {
    let mut slashes = 0usize;
    for c in literal.chars() {
        if c == '$' && slashes.is_multiple_of(2) {
            return false;
        }
        slashes = if c == '\\' { slashes + 1 } else { 0 };
    }
    true
}

/// Source-bound heading and anchor choices for one MDX projection.
pub(crate) struct Projection<'a> {
    pub headings: &'a [HeadingBinding],
    pub title: Option<&'a HeadingBinding>,
    pub anchors: Option<&'a gazette_core::documentation::projection::AnchorProjection>,
}

pub(crate) fn adapt<'a>(
    arena: &'a comrak::Arena<'a>,
    root: Node<'a>,
    syntax: &Syntax,
    projection: Projection<'_>,
    lines: &Lines<'_>,
    limits: Limits,
) -> Result<(), Error> {
    let Projection {
        headings,
        title,
        anchors,
    } = projection;
    let layouts = if anchors.is_some() {
        layout::plan(root, syntax, lines)?
    } else {
        Vec::new()
    };
    let selected = heading_plan(syntax, headings, limits.output_bytes)?;
    let layout_ids = (!layouts.is_empty())
        .then(|| RefCell::new(headings.iter().map(|heading| heading.id.clone()).collect()));
    if let Some(title) = title {
        if selected.get(&title.occurrence.location.span).copied() != Some(title) {
            return Err(Error::InvalidPlan);
        }
        replace_title(root, title, lines, limits.output_bytes)?;
    }
    let callouts = if anchors.is_some() {
        take_callout_markers(root, lines)?
    } else {
        Vec::new()
    };
    let stack = RefCell::new(Vec::new());
    let mut last_html = None;
    for node in root.descendants() {
        let mut data = node.data_mut();
        let location = lines.location(data.sourcepos)?;
        match &mut data.value {
            NodeValue::Text(text) => {
                let transported =
                    if let Some(label) = source_alert_label(node, text, location, lines) {
                        let mut out = Output::new(limits.output_bytes);
                        write!(out, "<strong>{label}</strong>").map_err(|_| out.error())?;
                        out.value
                    } else {
                        text_transport(text, limits.output_bytes)?
                    };
                data.value = NodeValue::HtmlInline(transported);
            }
            NodeValue::HtmlInline(html) => {
                *html =
                    html_transport(html, location, limits, &stack, anchors, layout_ids.as_ref())?;
                last_html = Some(location);
            }
            NodeValue::HtmlBlock(html) => {
                let raw_anchors =
                    if title.is_some_and(|title| title.occurrence.location == location) {
                        None
                    } else {
                        anchors
                    };
                html.literal = html_transport(
                    &html.literal,
                    location,
                    limits,
                    &stack,
                    raw_anchors,
                    layout_ids.as_ref(),
                )?;
                last_html = Some(location);
            }
            NodeValue::Math(math) => {
                let contained = if math.display_math && math.literal.contains('\n') {
                    math_delimiters_safe(&math.literal)
                } else {
                    // The target's inline tokenizer closes on dollar characters
                    // even after a TeX backslash; native parsing differs here.
                    !math.literal.contains('$')
                };
                if !contained || (!math.dollar_math && math.literal.contains(['\r', '\n'])) {
                    return Err(Error::Unsupported {
                        location,
                        reason: "math literal cannot be contained in target dollar delimiters",
                    });
                }
                // Keep the native inline/block position and exact TeX. MDX's
                // formula parser consumes dollar math, including native code-math
                // without its source-specific backtick fences.
                math.dollar_math = true;
            }
            NodeValue::CodeBlock(code)
                if syntax.identity.profile == Profile::PublicationMath
                    && code.fenced
                    && code.info == "math" =>
            {
                if !math_delimiters_safe(&code.literal) {
                    return Err(Error::Unsupported {
                        location,
                        reason: "fenced math literal cannot be contained in target dollar delimiters",
                    });
                }
                data.value = display_math(&code.literal, limits.output_bytes)?;
            }
            _ => {}
        }
    }
    if !stack.borrow().is_empty() {
        return Err(Error::Unsupported {
            location: last_html.ok_or(Error::InvalidSource)?,
            reason: "optional or missing HTML end tags require a qualified MDX tree projection",
        });
    }
    // Platform-owned custom syntax carries the already allocated native IDs.
    // Appending after literal transport keeps authored braces separate from it.
    for node in root.descendants() {
        let data = node.data();
        if !matches!(data.value, NodeValue::Heading(_)) {
            continue;
        }
        let location = lines.location(data.sourcepos)?;
        if let Some(binding) = selected.get(&location.span) {
            let deep = anchors.is_some() && binding.level >= 5;
            drop(data);
            let mut annotation = Output::new(limits.output_bytes);
            if deep {
                // A fixed platform-owned constructor, after authored HTML has
                // passed its separate transport. No authored component is admitted.
                write!(annotation, "<Heading level={{{}}} id=\"", binding.level)
                    .map_err(|_| annotation.error())?;
                literal(&mut annotation, &binding.id)?;
                annotation
                    .write_str("\" className=\"scroll-mt-(--scroll-mt)\">")
                    .map_err(|_| annotation.error())?;
                node.data_mut().value = NodeValue::Paragraph;
                node.prepend(arena.alloc(NodeValue::HtmlInline(annotation.value).into()));
                node.append(arena.alloc(NodeValue::HtmlInline("</Heading>".into()).into()));
            } else {
                write!(annotation, " {{#{}}}", binding.id).map_err(|_| annotation.error())?;
                node.append(arena.alloc(NodeValue::HtmlInline(annotation.value).into()));
            }
        }
    }
    // Comrak inserts an HTML comment at these boundaries during serialization,
    // after authored HTML has already been transported. Keep the structural
    // separator in the AST using inert target syntax; never replace text in code.
    let boundaries: Vec<_> = root
        .descendants()
        .filter(|node| {
            matches!(node.data().value, NodeValue::List(_))
                && node.next_sibling().is_some_and(|next| {
                    matches!(
                        next.data().value,
                        NodeValue::List(_) | NodeValue::CodeBlock(_)
                    )
                })
        })
        .collect();
    for node in boundaries {
        node.insert_after(
            arena.alloc(
                NodeValue::HtmlBlock(NodeHtmlBlock {
                    block_type: 6,
                    literal: "{/* list boundary */}\n".into(),
                })
                .into(),
            ),
        );
    }
    // Child links, literals, HTML and headings have already passed their usual
    // transport. Only these provider-owned wrappers are introduced afterwards;
    // the existing complete formatter ceiling charges their serialized bytes.
    wrap_callouts(arena, callouts, limits.output_bytes)?;
    layout::wrap(
        arena,
        layouts,
        layout_ids.map(RefCell::into_inner).unwrap_or_default(),
        limits.output_bytes,
    )?;
    Ok(())
}

// Numeric entities preserve HTML text/attribute values inside JSX. They also
// prevent raw HTML text from acquiring Markdown or expression semantics.
fn literal(output: &mut Output, text: &str) -> Result<(), Error> {
    for c in text.chars() {
        if c.is_ascii_punctuation() {
            write!(output, "&#{};", u32::from(c)).map_err(|_| output.error())?;
        } else {
            output.write_char(c).map_err(|_| output.error())?;
        }
    }
    Ok(())
}

fn name(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'-' | b'_'))
}

fn void(name: &str) -> bool {
    matches!(
        name,
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "param"
            | "source"
            | "track"
            | "wbr"
    )
}

fn boolean(name: &str) -> bool {
    matches!(
        name,
        "allowfullscreen"
            | "async"
            | "autofocus"
            | "autoplay"
            | "checked"
            | "controls"
            | "default"
            | "defer"
            | "disabled"
            | "formnovalidate"
            | "hidden"
            | "inert"
            | "ismap"
            | "itemscope"
            | "loop"
            | "multiple"
            | "muted"
            | "nomodule"
            | "novalidate"
            | "open"
            | "playsinline"
            | "readonly"
            | "required"
            | "reversed"
            | "selected"
    )
}

fn attribute(name: &str) -> &str {
    match name {
        "class" => "className",
        "for" => "htmlFor",
        "tabindex" => "tabIndex",
        "colspan" => "colSpan",
        "rowspan" => "rowSpan",
        "itemscope" => "itemScope",
        "readonly" => "readOnly",
        "autofocus" => "autoFocus",
        "allowfullscreen" => "allowFullScreen",
        "playsinline" => "playsInline",
        "formnovalidate" => "formNoValidate",
        "nomodule" => "noModule",
        value => value,
    }
}

struct HtmlState<'a> {
    output: Output,
    failure: Option<Error>,
    stack: &'a RefCell<Vec<String>>,
    depth: usize,
    anchors: Option<&'a gazette_core::documentation::projection::AnchorProjection>,
    layout_ids: Option<&'a RefCell<BTreeSet<String>>>,
}

impl HtmlState<'_> {
    fn tag(&mut self, tag: Tag) -> Result<(), Error> {
        if self.anchors.is_some()
            && (matches!(tag.name.as_ref(), "h5" | "h6" | "heading")
                || layout::reserved_html_name(tag.name.as_ref()))
        {
            return Err(Error::Formatting);
        }
        if !name(&tag.name) || (tag.self_closing && !void(&tag.name)) {
            return Err(Error::Formatting);
        }
        if tag.kind == TagKind::EndTag {
            if void(&tag.name)
                || self.stack.borrow_mut().pop().as_deref() != Some(tag.name.as_ref())
            {
                return Err(Error::Formatting);
            }
            write!(self.output, "</{}>", tag.name).map_err(|_| self.output.error())?;
            return Ok(());
        }
        if !void(&tag.name) {
            let mut stack = self.stack.borrow_mut();
            if stack.len() >= self.depth {
                return Err(Error::Limit("MDX HTML depth"));
            }
            stack.push(tag.name.to_string());
        }
        write!(self.output, "<{}", tag.name).map_err(|_| self.output.error())?;
        for attr in tag.attrs {
            if !attr.name.ns.is_empty() || attr.name.prefix.is_some() || !name(&attr.name.local) {
                return Err(Error::Formatting);
            }
            let key = attribute(&attr.name.local);
            if boolean(&attr.name.local) {
                write!(self.output, " {key}").map_err(|_| self.output.error())?;
            } else {
                write!(self.output, " {key}=\"").map_err(|_| self.output.error())?;
                let value = if (attr.name.local.as_ref() == "id"
                    || (tag.name.as_ref() == "a" && attr.name.local.as_ref() == "name"))
                    && let Some(anchors) = self.anchors
                {
                    anchors.id(&attr.value)
                } else {
                    attr.value.to_string()
                };
                if (attr.name.local.as_ref() == "id"
                    || (tag.name.as_ref() == "a" && attr.name.local.as_ref() == "name"))
                    && let Some(ids) = self.layout_ids
                {
                    ids.borrow_mut().insert(value.clone());
                }
                literal(&mut self.output, &value)?;
                self.output
                    .write_char('"')
                    .map_err(|_| self.output.error())?;
            }
        }
        self.output
            .write_str(if void(&tag.name) { " />" } else { ">" })
            .map_err(|_| self.output.error())?;
        Ok(())
    }
}

struct HtmlSink<'a>(RefCell<HtmlState<'a>>);
impl TokenSink for HtmlSink<'_> {
    type Handle = ();
    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        let result = match token {
            Token::TagToken(tag) => state.tag(tag),
            Token::CharacterTokens(text) => literal(&mut state.output, &text),
            Token::CommentToken(_) | Token::EOFToken => Ok(()),
            _ => Err(Error::Formatting),
        };
        if let Err(error) = result {
            state.failure = Some(error);
        }
        TokenSinkResult::Continue
    }
}

fn html_transport(
    html: &str,
    location: Location,
    limits: Limits,
    stack: &RefCell<Vec<String>>,
    anchors: Option<&gazette_core::documentation::projection::AnchorProjection>,
    layout_ids: Option<&RefCell<BTreeSet<String>>>,
) -> Result<String, Error> {
    let tokenizer = Tokenizer::new(
        HtmlSink(RefCell::new(HtmlState {
            output: Output::new(limits.output_bytes),
            failure: None,
            stack,
            depth: limits.depth,
            anchors,
            layout_ids,
        })),
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(html));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if let Some(error) = state.failure {
        return Err(match error {
            Error::Limit(_) => error,
            _ => Error::Unsupported {
                location,
                reason: "HTML cannot be transported to inert MDX without changing its meaning",
            },
        });
    }
    Ok(state.output.value)
}
