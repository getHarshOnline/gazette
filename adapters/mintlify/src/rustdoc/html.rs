//! Bounded extraction of native Rustdoc content, not arbitrary HTML admission.

mod aliases;
mod math;
mod notable;

use crate::{Error, invalid};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
        states::RawKind,
    },
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

pub(super) struct Page {
    pub(super) title: String,
    pub(super) crate_name: String,
    pub(super) version: Option<String>,
    pub(super) generator: String,
    pub(super) source: bool,
    pub(super) anchors: BTreeSet<String>,
    pub(super) semantic_scripts: Vec<String>,
    pub(super) type_script: Option<(String, String)>,
    alias: bool,
    decoded: usize,
    tokens: usize,
    nodes: Vec<Node>,
    hrefs: BTreeSet<String>,
}

pub(super) enum Node {
    Start {
        name: String,
        attributes: BTreeMap<String, String>,
    },
    End(String),
    Text(String),
}

impl Page {
    pub(super) fn reading_text(&self, maximum: usize) -> Result<String, Error> {
        let mut text = String::new();
        for node in &self.nodes {
            let value = match node {
                Node::Text(value) => value.as_str(),
                Node::End(name)
                    if matches!(
                        name.as_str(),
                        "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "pre"
                    ) =>
                {
                    "\n"
                }
                _ => continue,
            };
            if text
                .len()
                .checked_add(value.len())
                .is_none_or(|length| length > maximum)
            {
                return Err(invalid("rustdoc", "catalog text byte bound exceeded"));
            }
            text.push_str(value);
        }
        Ok(text)
    }

    pub(super) fn links(&self) -> impl Iterator<Item = &str> {
        self.hrefs.iter().map(String::as_str)
    }

    pub(super) fn render(
        &self,
        context: &str,
        maximum: usize,
        links: &BTreeMap<String, String>,
        extra_ids: &BTreeMap<String, Vec<String>>,
    ) -> Result<Vec<u8>, Error> {
        super::emit::render(self, &self.nodes, context, maximum, links, extra_ids, false)
    }

    pub(super) fn render_provider(
        &self,
        context: &str,
        maximum: usize,
        links: &BTreeMap<String, String>,
        extra_ids: &BTreeMap<String, Vec<String>>,
    ) -> Result<Vec<u8>, Error> {
        super::emit::render(self, &self.nodes, context, maximum, links, extra_ids, true)
    }
}

struct Frame {
    name: String,
    emitted: Option<String>,
    hidden: bool,
    line_number: Option<(String, String)>,
    heading: bool,
    version: bool,
    root: bool,
    discarded: Option<(&'static str, String)>,
    notable_data: bool,
}

struct State<'a> {
    context: &'a str,
    maximum: usize,
    decoded: usize,
    tokens: usize,
    stack: Vec<Frame>,
    page: Page,
    inside: bool,
    roots: usize,
    headings: usize,
    body: bool,
    native_generator: bool,
    version_text: String,
    failure: Option<Error>,
    fragment: bool,
    notable: BTreeMap<String, String>,
    notable_json: Option<String>,
    notable_complete: bool,
    math: Option<gazette_core::mathml::Validator>,
}

fn attribute<'a>(tag: &'a Tag, name: &str) -> Option<&'a str> {
    tag.attrs
        .iter()
        .find(|attr| attr.name.local.as_ref() == name)
        .map(|attr| attr.value.as_ref())
}

fn class(tag: &Tag, name: &str) -> bool {
    attribute(tag, "class")
        .is_some_and(|value| value.split_ascii_whitespace().any(|part| part == name))
}

fn label(value: &str) -> bool {
    !value.trim().is_empty() && !value.chars().any(char::is_control)
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

impl State<'_> {
    fn fail(&self, reason: &'static str) -> Error {
        invalid(self.context, reason)
    }

    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.decoded = self
            .decoded
            .checked_add(bytes)
            .filter(|&size| size <= self.maximum)
            .ok_or_else(|| self.fail("Rustdoc decoded-content ceiling exceeded"))?;
        Ok(())
    }

    fn start(&mut self, tag: Tag) -> Result<TokenSinkResult<()>, Error> {
        let name = tag.name.as_ref();
        let notable_link = self.notable_link(&tag)?;
        let notable_data = self.notable_data(&tag)?;
        let type_data = self.type_data(&tag)?;
        if self.stack.len() >= 128 {
            return Err(self.fail("Rustdoc element-depth ceiling exceeded"));
        }
        for attr in &tag.attrs {
            self.charge(attr.name.local.len().saturating_add(attr.value.len()))?;
            if !attr.name.ns.is_empty()
                || attr.name.local.as_ref().starts_with("on")
                || attr.name.local.as_ref() == "data-self-path" && !type_data
            {
                return Err(self.fail("unsupported Rustdoc active or script-dependent attribute"));
            }
        }
        if name == "script"
            && !type_data
            && attribute(&tag, "src").is_some_and(|src| src.contains("type.impl/"))
        {
            return Err(self.fail("script-dependent Rustdoc type implementations are unsupported"));
        }
        if self.math_start(&tag)? {
            return Ok(TokenSinkResult::Continue);
        }
        if tag.self_closing && !void(name) {
            return Err(self.fail("unsupported self-closing Rustdoc element"));
        }
        if self.inside
            && matches!(
                name,
                "form"
                    | "input"
                    | "textarea"
                    | "select"
                    | "option"
                    | "object"
                    | "embed"
                    | "style"
                    | "link"
                    | "meta"
            )
        {
            return Err(self.fail("unsupported Rustdoc active content"));
        }
        if name == "body" {
            if self.body
                || !class(&tag, "rustdoc")
                || ![
                    "mod", "fn", "struct", "enum", "union", "trait", "macro", "constant", "static",
                    "src", "type",
                ]
                .iter()
                .any(|kind| class(&tag, kind))
            {
                return Err(self.fail("unsupported or duplicate Rustdoc page kind"));
            }
            self.body = true;
            self.page.source = class(&tag, "src");
            self.page.alias = class(&tag, "type");
        }
        if name == "meta" && attribute(&tag, "name") == Some("generator") {
            if self.native_generator || attribute(&tag, "content") != Some("rustdoc") {
                return Err(self.fail("missing or conflicting Rustdoc generator identity"));
            }
            self.native_generator = true;
        }
        if name == "meta" && attribute(&tag, "name") == Some("rustdoc-vars") {
            if !self.page.crate_name.is_empty() || !self.page.generator.is_empty() {
                return Err(self.fail("duplicate Rustdoc identity metadata"));
            }
            self.page.crate_name = attribute(&tag, "data-current-crate")
                .unwrap_or_default()
                .to_owned();
            self.page.generator = attribute(&tag, "data-rustdoc-version")
                .unwrap_or_default()
                .to_owned();
        }
        let root = attribute(&tag, "id") == Some("main-content");
        if root {
            self.roots += 1;
            if self.roots != 1
                || name != "section"
                || !class(&tag, "content")
                || !self.stack.iter().any(|frame| frame.name == "main")
                || !self.body
            {
                return Err(self.fail("unsupported Rustdoc main-content envelope"));
            }
            self.inside = true;
        }
        let inherited_hidden = self.stack.last().is_some_and(|frame| frame.hidden);
        let chrome = name == "rustdoc-toolbar"
            || name == "button" && attribute(&tag, "id") == Some("copy-path")
            || name == "summary" && class(&tag, "hideme");
        let discarded = if notable_link.is_some() {
            Some(("ⓘ", String::new()))
        } else if self.inside && chrome && !inherited_hidden {
            Some((
                match name {
                    "button" => "Copy item path",
                    "summary" => "summary",
                    _ => "",
                },
                String::new(),
            ))
        } else {
            None
        };
        let mut hidden = inherited_hidden || chrome;
        let mut emitted = None;
        let line_number = self.inside
            && self.page.source
            && name == "a"
            && attribute(&tag, "data-nosnippet").is_some();
        let heading = self.inside && !hidden && name == "h1";
        let version = !self.inside && name == "span" && class(&tag, "version");
        if version && (self.page.version.is_some() || self.stack.iter().any(|frame| frame.version))
        {
            return Err(self.fail("duplicate Rustdoc crate version"));
        }
        if heading {
            self.headings += 1;
        }
        if notable_data || type_data {
            hidden = true;
        } else if self.inside && name == "script" {
            let src = attribute(&tag, "src")
                .filter(|src| label(src) && src.contains("trait.impl/"))
                .ok_or_else(|| self.fail("unsupported executable Rustdoc content"))?;
            if tag.attrs.iter().any(|attr| {
                !matches!(
                    attr.name.local.as_ref(),
                    "src" | "async" | "data-ignore-extern-crates"
                )
            }) {
                return Err(self.fail("unsupported Rustdoc semantic script attributes"));
            }
            self.page.semantic_scripts.push(src.to_owned());
            hidden = true;
        } else if self.inside && !hidden {
            let output_name = match name {
                "section" | "div" | "details" | "summary" => "div",
                "a" if line_number => "span",
                "a" | "span" | "p" | "pre" | "code" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "ul" | "ol" | "li" | "dl" | "dt" | "dd" | "em" | "strong" | "b" | "i" | "s"
                | "del" | "sup" | "sub" | "blockquote" | "table" | "thead" | "tbody" | "tr"
                | "th" | "td" | "br" | "hr" | "wbr" => name,
                _ => return Err(self.fail("unsupported Rustdoc content element")),
            };
            let mut attributes = BTreeMap::new();
            for attr in &tag.attrs {
                let key = attr.name.local.as_ref();
                let value = attr.value.as_ref();
                if key == "href"
                    && let Some(anchor) = notable_link.as_deref()
                {
                    let href = format!("#{anchor}");
                    self.page.hrefs.insert(href.clone());
                    attributes.insert(key.to_owned(), href);
                    continue;
                }
                match key {
                    "id" => {
                        if !label(value)
                            || value.chars().any(char::is_whitespace)
                            || !self.page.anchors.insert(value.to_owned())
                        {
                            return Err(self.fail("invalid or duplicate Rustdoc content anchor"));
                        }
                        attributes.insert(key.to_owned(), value.to_owned());
                    }
                    "data-notable-ty" if notable_link.is_some() => {}
                    "href" if name == "a" && !line_number => {
                        if !label(value) {
                            return Err(self.fail("invalid Rustdoc reference"));
                        }
                        self.page.hrefs.insert(value.to_owned());
                        attributes.insert(key.to_owned(), value.to_owned());
                    }
                    "title" => {
                        attributes.insert(key.to_owned(), value.to_owned());
                    }
                    "class" | "open" | "data-nosnippet" | "data-aliases" => {}
                    "tabindex"
                        if !self.fragment
                            && name == "section"
                            && attribute(&tag, "id") == Some("main-content")
                            && value == "-1" => {}
                    "href" if line_number => {}
                    _ => return Err(self.fail("unsupported Rustdoc content attribute")),
                }
            }
            if line_number {
                let id = attribute(&tag, "id")
                    .filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()))
                    .ok_or_else(|| self.fail("invalid Rustdoc source-line marker"))?;
                if attribute(&tag, "href") != Some(format!("#{id}").as_str()) {
                    return Err(self.fail("conflicting Rustdoc source-line marker"));
                }
            }
            self.page.nodes.push(Node::Start {
                name: output_name.to_owned(),
                attributes,
            });
            if !void(name) {
                emitted = Some(output_name.to_owned());
            }
        }
        if !void(name) {
            self.stack.push(Frame {
                name: name.to_owned(),
                emitted,
                hidden,
                line_number: line_number.then(|| {
                    (
                        attribute(&tag, "id").unwrap_or_default().to_owned(),
                        String::new(),
                    )
                }),
                heading,
                version,
                root,
                discarded,
                notable_data,
            });
        }
        let raw = match name {
            "script" => Some(RawKind::ScriptData),
            "style" => Some(RawKind::Rawtext),
            "title" | "textarea" => Some(RawKind::Rcdata),
            "plaintext" | "xmp" | "iframe" | "noembed" | "noframes" | "svg" | "math" => {
                return Err(self.fail("unsupported Rustdoc raw or foreign context"));
            }
            _ => None,
        };
        Ok(raw.map_or(TokenSinkResult::Continue, TokenSinkResult::RawData))
    }

    fn end(&mut self, name: &str) -> Result<(), Error> {
        if self.math.is_some() {
            return self.math_end(name);
        }
        let frame = self
            .stack
            .pop()
            .ok_or_else(|| self.fail("unbalanced Rustdoc element"))?;
        if frame.name != name {
            return Err(self.fail("unbalanced Rustdoc element"));
        }
        if frame.line_number.is_some_and(|(id, text)| text != id) {
            return Err(self.fail("conflicting Rustdoc source-line text"));
        }
        if frame.discarded.is_some_and(|(expected, text)| {
            let text = text.trim();
            if expected == "summary" {
                text != "Expand description" && !counted_summary(text)
            } else {
                text != expected
            }
        }) {
            return Err(self.fail("unrecognized Rustdoc control content"));
        }
        if frame.notable_data {
            self.expand_notable()?;
        }
        if let Some(name) = frame.emitted {
            self.page.nodes.push(Node::End(name));
        }
        if frame.version {
            if !label(&self.version_text) {
                return Err(self.fail("invalid Rustdoc crate version"));
            }
            self.page.version = Some(std::mem::take(&mut self.version_text));
        }
        if frame.root {
            self.inside = false;
        }
        Ok(())
    }

    fn token(&mut self, token: Token) -> Result<TokenSinkResult<()>, Error> {
        self.tokens += 1;
        if self.tokens > self.maximum.min(250_000) {
            return Err(self.fail("Rustdoc token ceiling exceeded"));
        }
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => return self.start(tag),
            Token::TagToken(tag) => self.end(tag.name.as_ref())?,
            Token::CharacterTokens(text) => {
                self.charge(text.len())?;
                if let Some(math) = &mut self.math {
                    math.text(&text)
                        .map_err(|_| self.fail("invalid Rustdoc MathML text"))?;
                }
                if self.stack.last().is_some_and(|frame| frame.notable_data) {
                    let Some(literal) = &mut self.notable_json else {
                        return Err(self.fail("missing Rustdoc notable-trait data"));
                    };
                    literal.push_str(&text);
                    return Ok(TokenSinkResult::Continue);
                }
                for frame in &mut self.stack {
                    if let Some((_, discarded)) = &mut frame.discarded {
                        discarded.push_str(&text);
                    }
                    if let Some((_, number)) = &mut frame.line_number {
                        number.push_str(&text);
                    }
                }
                if self.stack.iter().any(|frame| frame.version) {
                    self.version_text.push_str(&text);
                }
                if self.inside
                    && self
                        .stack
                        .last()
                        .is_some_and(|frame| frame.name == "script")
                    && !text.trim().is_empty()
                {
                    return Err(self.fail("inline executable Rustdoc content is unsupported"));
                }
                if self.inside
                    && !self
                        .stack
                        .iter()
                        .any(|frame| frame.hidden || frame.line_number.is_some())
                {
                    if self.stack.iter().any(|frame| frame.heading) {
                        self.page.title.push_str(&text);
                    }
                    if let Some(Node::Text(previous)) = self.page.nodes.last_mut() {
                        previous.push_str(&text);
                    } else {
                        self.page.nodes.push(Node::Text(text.to_string()));
                    }
                }
            }
            Token::CommentToken(_) => {}
            Token::DoctypeToken(doctype)
                if doctype.name.as_deref() == Some("html")
                    && doctype.public_id.is_none()
                    && doctype.system_id.is_none()
                    && !doctype.force_quirks => {}
            Token::EOFToken if self.stack.is_empty() && self.math.is_none() => {}
            _ => return Err(self.fail("malformed or unsupported Rustdoc HTML")),
        }
        Ok(TokenSinkResult::Continue)
    }
}

fn counted_summary(text: &str) -> bool {
    let Some(count) = text.strip_prefix("Show ").and_then(|value| {
        value
            .strip_suffix(" fields")
            .or_else(|| value.strip_suffix(" variants"))
    }) else {
        return false;
    };
    !count.is_empty() && !count.starts_with('0') && count.bytes().all(|byte| byte.is_ascii_digit())
}

struct Sink<'a>(RefCell<State<'a>>);
impl TokenSink for Sink<'_> {
    type Handle = ();
    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        match state.token(token) {
            Ok(result) => result,
            Err(error) => {
                state.failure = Some(error);
                TokenSinkResult::Continue
            }
        }
    }
}

pub(super) fn parse(bytes: &[u8], context: &str, maximum: usize) -> Result<Page, Error> {
    if bytes.len() > maximum || bytes.is_empty() {
        return Err(invalid(
            context,
            "Rustdoc input ceiling exceeded or empty page",
        ));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid(context, "Rustdoc HTML must be UTF-8"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut state = tokenize(text, state(context, maximum))?;
    if !state.notable.is_empty() && !state.notable_complete {
        return Err(invalid(context, "missing Rustdoc notable-trait data"));
    }
    state.page.title = state.page.title.trim().to_owned();
    if state.roots != 1
        || state.headings != 1
        || !state.native_generator
        || !label(&state.page.title)
        || !label(&state.page.crate_name)
        || !label(&state.page.generator)
        || (!state.page.source && state.page.version.is_none())
    {
        return Err(invalid(
            context,
            "incomplete qualified Rustdoc page identity",
        ));
    }
    state.page.decoded = state.decoded;
    state.page.tokens = state.tokens;
    Ok(state.page)
}

fn state(context: &str, maximum: usize) -> State<'_> {
    State {
        context,
        maximum,
        decoded: 0,
        tokens: 0,
        stack: Vec::new(),
        inside: false,
        roots: 0,
        headings: 0,
        body: false,
        native_generator: false,
        version_text: String::new(),
        failure: None,
        fragment: false,
        notable: BTreeMap::new(),
        notable_json: None,
        notable_complete: false,
        math: None,
        page: Page {
            title: String::new(),
            crate_name: String::new(),
            version: None,
            generator: String::new(),
            source: false,
            anchors: BTreeSet::new(),
            semantic_scripts: Vec::new(),
            type_script: None,
            alias: false,
            decoded: 0,
            tokens: 0,
            nodes: Vec::new(),
            hrefs: BTreeSet::new(),
        },
    }
}

fn tokenize<'a>(text: &str, state: State<'a>) -> Result<State<'a>, Error> {
    let context = state.context;
    let maximum = state.maximum;
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(state)),
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let queue = BufferQueue::default();
    let mut offset = 0;
    let mut buffered = 0usize;
    while offset < text.len() {
        let mut end = text.len().min(offset.saturating_add(256));
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        buffered = buffered.saturating_add(end - offset);
        if buffered > maximum.min(64 << 10) {
            return Err(invalid(context, "Rustdoc unemitted-token ceiling exceeded"));
        }
        let before = tokenizer.sink.0.borrow().tokens;
        queue.push_back(StrTendril::from_slice(&text[offset..end]));
        let _ = tokenizer.feed(&queue);
        let mut state = tokenizer.sink.0.borrow_mut();
        if let Some(error) = state.failure.take() {
            return Err(error);
        }
        if before != state.tokens {
            buffered = 0;
        }
        offset = end;
    }
    if !queue.is_empty() {
        return Err(invalid(context, "incomplete Rustdoc HTML tokenization"));
    }
    if tokenizer.sink.0.borrow().fragment {
        let open: Vec<_> = tokenizer
            .sink
            .0
            .borrow()
            .stack
            .iter()
            .map(|frame| frame.name.clone())
            .collect();
        if open == ["pre", "code"] {
            queue.push_back(StrTendril::from_slice("</code></pre>"));
            let _ = tokenizer.feed(&queue);
        }
    }
    tokenizer.end();
    let mut state = tokenizer.sink.0.into_inner();
    if let Some(error) = state.failure.take() {
        return Err(error);
    }
    Ok(state)
}

pub(super) fn append_implementations(
    page: &mut Page,
    rows: Vec<super::type_impls::Implementation>,
    origin: &str,
    data_origin: &str,
    maximum: usize,
) -> Result<(), Error> {
    aliases::append(page, rows, origin, data_origin, maximum)
}

#[cfg(test)]
#[path = "../../tests/internal/rustdoc_html.rs"]
mod tests;
