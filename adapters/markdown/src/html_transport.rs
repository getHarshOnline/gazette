//! Native tree serialization with transport-safe LF, not presentation rendering.

use crate::{
    bounded::Output,
    html_tree::{self, Handle, Kind},
};
use gazette_core::{
    markdown::{Error, Limits},
    source::Location,
};
use html5ever::{
    ns,
    serialize::{HtmlSerializer, SerializeOpts, Serializer},
};
use std::{
    fmt::Write as _,
    io::{self, Write},
};

pub(crate) fn encode(html: &str, limits: Limits, location: Location) -> Result<String, Error> {
    let tree = html_tree::parse(html, limits).map_err(|error| match error {
        Error::Formatting => Error::Unsupported {
            location,
            reason: "selected HTML does not have an unambiguous native fragment tree",
        },
        error => error,
    })?;
    encode_nodes(tree.root.children.borrow().iter(), limits, location, false)
}

/// Transport one already admitted subtree without including its presentation ancestors.
pub(crate) fn encode_node(
    node: &Handle,
    limits: Limits,
    location: Location,
) -> Result<String, Error> {
    encode_nodes(std::iter::once(node), limits, location, true)
}

fn encode_nodes<'a>(
    nodes: impl IntoIterator<Item = &'a Handle>,
    limits: Limits,
    location: Location,
    protect_markdown: bool,
) -> Result<String, Error> {
    let mut serializer = HtmlSerializer::new(
        Encoded {
            output: Output::new(limits.output_bytes),
            protect_markdown,
            text: false,
            literal: false,
            guard_literal: false,
            after_lf: false,
            blank_line: false,
            unsafe_boundary: false,
        },
        SerializeOpts::default(),
    );
    let result = nodes
        .into_iter()
        .try_for_each(|node| write_node(node, &mut serializer, 0, limits.depth, false, true));
    let writer = serializer.writer;
    if writer.unsafe_boundary {
        return Err(Error::Unsupported {
            location,
            reason: "selected HTML literal blank lines cannot be preserved in Markdown transport",
        });
    }
    result.map_err(|_| writer.output.error())?;
    Ok(writer.output.value)
}
fn raw(name: &html5ever::QualName) -> bool {
    name.ns == ns!(html)
        && matches!(
            name.local.as_ref(),
            "script"
                | "style"
                | "xmp"
                | "iframe"
                | "noembed"
                | "noframes"
                | "plaintext"
                | "noscript"
        )
}
fn write_node(
    node: &Handle,
    serializer: &mut HtmlSerializer<Encoded>,
    depth: usize,
    maximum: usize,
    literal: bool,
    outer: bool,
) -> io::Result<()> {
    if depth > maximum {
        return Err(io::ErrorKind::InvalidData.into());
    }
    match &node.kind {
        Kind::Document => {
            for child in node.children.borrow().iter() {
                write_node(child, serializer, depth + 1, maximum, literal, outer)?;
            }
        }
        Kind::Element {
            name,
            attrs,
            template,
            ..
        } => {
            serializer.writer.text = false;
            serializer.writer.literal = false;
            serializer.writer.guard_literal = false;
            serializer.start_elem(
                name.clone(),
                attrs.borrow().iter().map(|a| (&a.name, a.value.as_ref())),
            )?;
            let children = template.as_ref().unwrap_or(node).children.borrow();
            // Native reparsing discards one initial LF in these elements.
            if name.ns == ns!(html)
                && matches!(name.local.as_ref(), "pre" | "textarea" | "listing")
                && children.first().is_some_and(
                    |n| matches!(&n.kind, Kind::Text(text) if text.borrow().starts_with('\n')),
                )
            {
                serializer.write_text("\n")?;
            }
            for child in children.iter() {
                write_node(child, serializer, depth + 1, maximum, raw(name), false)?;
            }
            serializer.writer.text = false;
            serializer.writer.literal = false;
            serializer.writer.guard_literal = false;
            serializer.end_elem(name.clone())?;
        }
        Kind::Text(text) => {
            serializer.writer.text = true;
            serializer.writer.literal = literal || outer;
            serializer.writer.guard_literal = literal;
            serializer.write_text(&text.borrow())?;
        }
        Kind::Comment(text) => {
            serializer.writer.text = false;
            serializer.writer.literal = true;
            serializer.writer.guard_literal = !outer;
            serializer.write_comment(text)?;
        }
        Kind::Pi(target, data) => {
            serializer.writer.text = false;
            serializer.writer.literal = true;
            serializer.writer.guard_literal = !outer;
            serializer.write_processing_instruction(target, data)?;
        }
    }
    Ok(())
}
struct Encoded {
    output: Output,
    protect_markdown: bool,
    text: bool,
    literal: bool,
    guard_literal: bool,
    after_lf: bool,
    blank_line: bool,
    unsafe_boundary: bool,
}
impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let text = std::str::from_utf8(bytes).map_err(|_| io::ErrorKind::InvalidData)?;
        for character in text.chars() {
            if self.protect_markdown
                && self.text
                && !self.literal
                && matches!(
                    character,
                    '\\' | '`' | '*' | '_' | '[' | ']' | '!' | '~' | '$' | '#'
                )
            {
                // Isolated SVG/MathML can enter Markdown's inline HTML grammar.
                // Character references preserve their text without allowing it
                // to become emphasis, links or other Markdown syntax. Leave the
                // serializer's entity delimiters intact; attributes are separate.
                write!(self.output, "&#{};", character as u32)
                    .map_err(|_| io::ErrorKind::OutOfMemory)?;
            } else if character == '\n' && !self.literal {
                self.output
                    .write_str("&#10;")
                    .map_err(|_| io::ErrorKind::OutOfMemory)?;
                self.after_lf = false;
                self.blank_line = false;
            } else {
                if character == '\n' {
                    if self.guard_literal && self.after_lf && self.blank_line {
                        self.unsafe_boundary = true;
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    self.after_lf = true;
                    self.blank_line = true;
                } else if !matches!(character, ' ' | '\t' | '\r') {
                    self.blank_line = false;
                }
                self.output
                    .write_char(character)
                    .map_err(|_| io::ErrorKind::OutOfMemory)?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
