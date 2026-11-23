//! Closed local references over retained Sphinx HTML fragments.

use crate::{Error, invalid};
use html5ever::{
    tendril::StrTendril,
    tokenizer::{BufferQueue, Tag, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts},
};
use percent_encoding::percent_decode_str;
use serde_json::Value;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
};

#[derive(Debug)]
struct Reference {
    destination: String,
    kind: Kind,
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Link,
    Resource,
}

#[derive(Default)]
struct Page {
    anchors: BTreeSet<String>,
    references: Vec<Reference>,
}

struct State<'a> {
    context: &'a str,
    page: Page,
    failure: Option<Error>,
    text: Option<(String, usize)>,
}

impl State<'_> {
    fn fail(&mut self, reason: &'static str) {
        if self.failure.is_none() {
            self.failure = Some(invalid(self.context, reason));
        }
    }

    fn start(&mut self, tag: Tag) {
        let attribute = |name: &str| {
            tag.attrs
                .iter()
                .find(|attribute| attribute.name.local.as_ref() == name)
                .map(|attribute| attribute.value.as_ref())
        };
        let anchor = attribute("id").or_else(|| {
            (tag.name.as_ref() == "a")
                .then(|| attribute("name"))
                .flatten()
        });
        if let Some(anchor) = anchor
            && (anchor.is_empty()
                || anchor.chars().any(char::is_control)
                || !self.page.anchors.insert(anchor.to_owned()))
        {
            self.fail("invalid or duplicate Sphinx HTML anchor");
            return;
        }
        for (name, kind) in [("href", Kind::Link), ("src", Kind::Resource)] {
            if let Some(destination) = attribute(name) {
                if destination.is_empty() || destination.chars().any(char::is_control) {
                    self.fail("invalid Sphinx HTML reference");
                    return;
                }
                self.page.references.push(Reference {
                    destination: destination.to_owned(),
                    kind,
                });
            }
        }
    }
}

struct Sink<'a>(RefCell<State<'a>>);

impl TokenSink for Sink<'_> {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<Self::Handle> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) => {
                if matches!(
                    tag.name.as_ref(),
                    "p" | "div"
                        | "br"
                        | "li"
                        | "tr"
                        | "dt"
                        | "dd"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                ) && let Some((text, maximum)) = &mut state.text
                {
                    if text.len() >= *maximum {
                        state.fail("catalog text byte bound exceeded");
                    } else {
                        text.push('\n');
                    }
                }
                if tag.kind == html5ever::tokenizer::TagKind::StartTag {
                    state.start(tag);
                }
            }
            Token::CharacterTokens(value) => {
                if let Some((text, maximum)) = &mut state.text {
                    if text
                        .len()
                        .checked_add(value.len())
                        .is_none_or(|size| size > *maximum)
                    {
                        state.fail("catalog text byte bound exceeded");
                    } else {
                        text.push_str(&value);
                    }
                }
            }
            Token::ParseError(_) => state.fail("malformed Sphinx HTML fragment"),
            _ => {}
        }
        TokenSinkResult::Continue
    }
}

pub(super) fn validate(
    root: &str,
    contexts: &BTreeMap<&str, Value>,
    files: &BTreeMap<&str, &[u8]>,
) -> Result<BTreeMap<String, BTreeSet<String>>, Error> {
    let mut pages = BTreeMap::new();
    for (document, context) in contexts {
        let mut page = Page::default();
        for field in ["body", "toc"] {
            let Some(fragment) = context.get(field).and_then(Value::as_str) else {
                continue;
            };
            page = inspect(fragment, &format!("{document}.fjson:{field}"), page)?;
        }
        pages.insert(*document, page);
    }
    for (document, page) in &pages {
        for reference in &page.references {
            validate_reference(root, document, reference, &pages, files)?;
        }
    }
    Ok(pages
        .into_iter()
        .map(|(document, page)| (document.to_owned(), page.anchors))
        .collect())
}

fn inspect(fragment: &str, context: &str, page: Page) -> Result<Page, Error> {
    gazette_core::documentation::projection::checked_html_anchors(fragment, context)?;
    let sink = Sink(RefCell::new(State {
        context,
        page,
        failure: None,
        text: None,
    }));
    let tokenizer = Tokenizer::new(
        sink,
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(fragment));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let mut state = tokenizer.sink.0.into_inner();
    if let Some(error) = state.failure.take() {
        return Err(error);
    }
    Ok(state.page)
}

pub(super) fn reading_text(fragment: &str, context: &str, maximum: usize) -> Result<String, Error> {
    gazette_core::documentation::projection::checked_html_anchors(fragment, context)?;
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(State {
            context,
            page: Page::default(),
            failure: None,
            text: Some((String::new(), maximum)),
        })),
        TokenizerOpts {
            exact_errors: true,
            discard_bom: false,
            ..Default::default()
        },
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from(fragment));
    let _ = tokenizer.feed(&input);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if let Some(error) = state.failure {
        return Err(error);
    }
    Ok(state.text.expect("reading text requested").0)
}

fn validate_reference(
    root: &str,
    origin: &str,
    reference: &Reference,
    pages: &BTreeMap<&str, Page>,
    files: &BTreeMap<&str, &[u8]>,
) -> Result<(), Error> {
    let destination = reference.destination.as_str();
    if external(destination)? {
        return Ok(());
    }
    let (path, fragment) = match destination.split_once('#') {
        Some(("", "")) if matches!(reference.kind, Kind::Link) => return Ok(()),
        Some((_, "")) => return Err(invalid(destination, "empty Sphinx HTML fragment")),
        Some((path, fragment)) => (path, Some(decode_fragment(fragment)?)),
        None => (destination, None),
    };
    if path.contains('?') {
        return Err(invalid(
            destination,
            "local Sphinx HTML references cannot contain queries",
        ));
    }
    if path.is_empty() {
        let fragment =
            fragment.ok_or_else(|| invalid(destination, "empty Sphinx HTML reference"))?;
        return require_anchor(origin, &fragment, pages, destination);
    }
    let target = resolve(root, origin, path, destination)?;
    if let Some(document) = page_target(&target, pages) {
        if matches!(reference.kind, Kind::Resource) {
            return Err(invalid(
                destination,
                "Sphinx resource reference resolves to a page",
            ));
        }
        if let Some(fragment) = fragment {
            require_anchor(document, &fragment, pages, destination)?;
        }
        return Ok(());
    }
    if fragment.is_some() {
        return Err(invalid(
            destination,
            "Sphinx asset reference cannot contain a fragment",
        ));
    }
    if files.contains_key(target.as_str()) {
        return Ok(());
    }
    Err(invalid(
        destination,
        "local Sphinx HTML reference has no exact retained target",
    ))
}

fn external(destination: &str) -> Result<bool, Error> {
    if destination.starts_with("//") {
        return Err(invalid(
            destination,
            "scheme-relative Sphinx HTML reference is unsupported",
        ));
    }
    let Some((scheme, _)) = destination.split_once(':') else {
        return Ok(false);
    };
    if !scheme.bytes().enumerate().all(|(index, byte)| {
        byte.is_ascii_alphabetic() || index > 0 && matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9')
    }) {
        return Ok(false);
    }
    if matches!(scheme, "http" | "https" | "mailto") && url::Url::parse(destination).is_ok() {
        return Ok(true);
    }
    Err(invalid(
        destination,
        "unsupported Sphinx HTML reference scheme",
    ))
}

fn resolve(root: &str, origin: &str, path: &str, context: &str) -> Result<String, Error> {
    if path.contains('\\') || path.starts_with("//") {
        return Err(invalid(context, "invalid Sphinx local reference path"));
    }
    let mut decoded = String::new();
    for (index, component) in path.split('/').enumerate() {
        if index > 0 {
            decoded.push('/');
        }
        let component = percent_decode_str(component)
            .decode_utf8()
            .map_err(|_| invalid(context, "invalid Sphinx local reference encoding"))?;
        if component.contains(['/', '\\']) {
            return Err(invalid(
                context,
                "encoded Sphinx path separators are unsupported",
            ));
        }
        decoded.push_str(&component);
    }
    let mut components = if path.starts_with('/') {
        Vec::new()
    } else {
        let root_document = origin == root;
        let mut origin_components: Vec<_> = origin.split('/').collect();
        // Sphinx's JSON builder uses directory-style links such as
        // `../model/` from the virtual `api/` page, while `.html` and asset
        // references retain file-relative semantics.
        if root_document || !path.ends_with('/') {
            origin_components.pop();
        }
        origin_components.into_iter().map(str::to_owned).collect()
    };
    for component in decoded.trim_start_matches('/').split('/') {
        match component {
            "" if decoded.ends_with('/') => {}
            "" | "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid(
                        context,
                        "Sphinx local reference escapes the selected directory",
                    ));
                }
            }
            value if value.chars().any(char::is_control) => {
                return Err(invalid(context, "invalid Sphinx local reference path"));
            }
            value => components.push(value.to_owned()),
        }
    }
    if path.ends_with('/') {
        components.push("index".into());
    }
    let target = components.join("/");
    if target.is_empty() {
        return Err(invalid(context, "empty Sphinx local reference target"));
    }
    Ok(target)
}

fn page_target<'a>(target: &str, pages: &'a BTreeMap<&'a str, Page>) -> Option<&'a str> {
    let target = target.strip_suffix(".html").unwrap_or(target);
    let target = target.strip_suffix("/index").unwrap_or(target);
    pages.get_key_value(target).map(|(document, _)| *document)
}

fn decode_fragment(fragment: &str) -> Result<String, Error> {
    let decoded = percent_decode_str(fragment)
        .decode_utf8()
        .map_err(|_| invalid(fragment, "invalid Sphinx HTML fragment encoding"))?;
    if decoded.is_empty() || decoded.chars().any(char::is_control) {
        return Err(invalid(fragment, "invalid Sphinx HTML fragment"));
    }
    Ok(decoded.into_owned())
}

fn require_anchor(
    document: &str,
    fragment: &str,
    pages: &BTreeMap<&str, Page>,
    context: &str,
) -> Result<(), Error> {
    if pages
        .get(document)
        .is_some_and(|page| page.anchors.contains(fragment))
    {
        Ok(())
    } else {
        Err(invalid(
            context,
            "Sphinx HTML reference has no exact retained fragment",
        ))
    }
}
