//! Join equivalent italic runs before the provider converts HTML to Markdown.

use crate::{Error, invalid, output::BoundedBuffer};
use html5ever::{
    QualName, ns,
    serialize::{HtmlSerializer, SerializeOpts, Serializer},
    tendril::StrTendril,
    tokenizer::{
        BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
    },
};
use std::cell::RefCell;

const MAX_NODES: usize = 100_000;
const MAX_DEPTH: usize = 128;

struct Element {
    tag: Tag,
    children: Vec<Node>,
}

enum Node {
    Element(Element),
    Text(StrTendril),
    Comment(StrTendril),
}

enum Failure {
    Limit(&'static str),
    Syntax,
}

#[derive(Default)]
struct State {
    roots: Vec<Node>,
    stack: Vec<Element>,
    nodes: usize,
    failure: Option<Failure>,
}

impl State {
    fn push(&mut self, node: Node) {
        if let Some(parent) = self.stack.last_mut() {
            parent.children.push(node);
        } else {
            self.roots.push(node);
        }
    }
}

struct Sink(RefCell<State>);

impl TokenSink for Sink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        if state.failure.is_some() {
            return TokenSinkResult::Continue;
        }
        state.nodes += 1;
        if state.nodes > MAX_NODES {
            state.failure = Some(Failure::Limit(
                "Sphinx emphasis conversion exceeds token limit",
            ));
            return TokenSinkResult::Continue;
        }
        match token {
            Token::TagToken(tag) if tag.kind == TagKind::StartTag => {
                let empty = void(tag.name.as_ref());
                // These elements need tokenizer states supplied by an HTML tree
                // builder. Re-serializing their decoded text could create markup.
                // Obsolete void elements also have serializer-specific behavior.
                if matches!(
                    tag.name.as_ref(),
                    "script"
                        | "style"
                        | "title"
                        | "textarea"
                        | "xmp"
                        | "iframe"
                        | "noembed"
                        | "noframes"
                        | "plaintext"
                        | "noscript"
                        | "basefont"
                        | "bgsound"
                        | "keygen"
                        | "frame"
                ) || tag.self_closing && !empty
                {
                    state.failure = Some(Failure::Syntax);
                } else if state.stack.len() >= MAX_DEPTH {
                    state.failure = Some(Failure::Limit(
                        "Sphinx emphasis conversion exceeds nesting limit",
                    ));
                } else {
                    let element = Element {
                        tag,
                        children: Vec::new(),
                    };
                    if empty {
                        state.push(Node::Element(element));
                    } else {
                        state.stack.push(element);
                    }
                }
            }
            Token::TagToken(tag) => {
                if let Some(element) = state.stack.pop() {
                    if element.tag.name != tag.name {
                        state.failure = Some(Failure::Syntax);
                    } else {
                        state.push(Node::Element(element));
                    }
                } else {
                    state.failure = Some(Failure::Syntax);
                }
            }
            Token::CharacterTokens(text) => state.push(Node::Text(text)),
            Token::CommentToken(text) => state.push(Node::Comment(text)),
            Token::EOFToken => {
                if !state.stack.is_empty() {
                    state.failure = Some(Failure::Syntax);
                }
            }
            _ => state.failure = Some(Failure::Syntax),
        }
        TokenSinkResult::Continue
    }
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

fn plain_emphasis(element: &Element) -> bool {
    element.tag.name.as_ref() == "em" && element.tag.attrs.is_empty()
}

fn emphasized(node: &Node) -> bool {
    let Node::Element(element) = node else {
        return false;
    };
    plain_emphasis(element)
        || element.tag.name.as_ref() == "a"
            && matches!(element.children.as_slice(), [Node::Element(child)] if plain_emphasis(child))
}

fn contents(node: Node) -> Vec<Node> {
    let Node::Element(mut element) = node else {
        unreachable!("checked emphasis element")
    };
    if plain_emphasis(&element) {
        return element.children;
    }
    let Node::Element(child) = element.children.remove(0) else {
        unreachable!("checked emphasized link")
    };
    element.children = child.children;
    vec![Node::Element(element)]
}

fn join(nodes: &mut Vec<Node>) -> bool {
    let mut changed = false;
    for node in nodes.iter_mut() {
        if let Node::Element(element) = node
            && !matches!(element.tag.name.as_ref(), "pre" | "code")
        {
            changed |= join(&mut element.children);
        }
    }
    let mut source = std::mem::take(nodes).into_iter().peekable();
    while let Some(node) = source.next() {
        if emphasized(&node) && source.peek().is_some_and(emphasized) {
            let mut children = contents(node);
            while source.peek().is_some_and(emphasized) {
                children.extend(contents(source.next().expect("checked next node")));
            }
            nodes.push(Node::Element(Element {
                tag: Tag {
                    kind: TagKind::StartTag,
                    name: "em".into(),
                    self_closing: false,
                    attrs: Vec::new(),
                },
                children,
            }));
            changed = true;
        } else {
            nodes.push(node);
        }
    }
    changed
}

fn serialize(nodes: &[Node], writer: &mut HtmlSerializer<BoundedBuffer>) -> std::io::Result<()> {
    for node in nodes {
        match node {
            Node::Text(text) => writer.write_text(text)?,
            Node::Comment(text) => writer.write_comment(text)?,
            Node::Element(element) => {
                let name = QualName::new(None, ns!(html), element.tag.name.clone());
                writer.start_elem(
                    name.clone(),
                    element
                        .tag
                        .attrs
                        .iter()
                        .map(|a| (&a.name, a.value.as_ref())),
                )?;
                serialize(&element.children, writer)?;
                writer.end_elem(name)?;
            }
        }
    }
    Ok(())
}

pub(super) fn normalize(
    fragment: &str,
    context: &str,
    ceiling: u64,
) -> Result<Option<String>, Error> {
    // Ordinary contexts remain byte-identical, including their original JSON spelling.
    if !fragment.contains("<em") {
        return Ok(None);
    }
    if fragment.len() as u64 > ceiling {
        return Err(invalid(context, "Sphinx HTML exceeds target byte limit"));
    }
    let tokenizer = Tokenizer::new(
        Sink(RefCell::new(State::default())),
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
    if let Some(reason) = state.failure {
        return match reason {
            Failure::Limit(reason) => Err(invalid(context, reason)),
            // Keep the existing pass-through contract for fragments outside this
            // balanced HTML profile, such as optional closing tags. Never guess.
            Failure::Syntax => Ok(None),
        };
    }
    if !join(&mut state.roots) {
        return Ok(None);
    }
    let mut writer = HtmlSerializer::new(BoundedBuffer::new(ceiling), SerializeOpts::default());
    serialize(&state.roots, &mut writer)
        .map_err(|_| invalid(context, "Sphinx HTML conversion exceeds target byte limit"))?;
    let bytes = writer.writer.finish();
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| invalid(context, "Sphinx HTML conversion produced invalid UTF-8"))
}
