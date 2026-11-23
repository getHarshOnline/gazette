//! Decode the finite literal-only MDX emitted for native documentation providers.
//!
//! This is not a general MDX parser. Only a target-owned title envelope, inert
//! HTML and JSON string carriers in code/headings are admitted. Consumers must
//! inspect the result with the native HTML profile, including `MathML`.

use std::{collections::BTreeSet, fmt};

/// Exact generated grammar, independent of provider execution.
pub const PROFILE: &str = "gazette-generated-literal-mdx/1";

/// Bounds checked before allocation and throughout the nonrecursive traversal.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Complete source byte ceiling.
    pub input_bytes: usize,
    /// Aggregate title, exact literals and reconstructed HTML byte ceiling.
    pub output_bytes: usize,
    /// Maximum text, element and literal occurrences.
    pub tokens: usize,
    /// Maximum open elements.
    pub depth: usize,
}

/// Complete syntactic decoding; no expressions have been evaluated.
#[derive(Debug)]
pub struct Decoded {
    /// JSON-decoded target title.
    pub title: String,
    pub(crate) html: Vec<u8>,
    pub(crate) literals: Vec<Literal>,
}

#[derive(Debug)]
pub(crate) struct Literal {
    pub(crate) offset: usize,
    pub(crate) value: String,
}

impl Decoded {
    /// Number of exact JSON text carriers retained in the body.
    pub fn literal_count(&self) -> usize {
        self.literals.len()
    }

    /// Aggregate retained title, HTML and exact literal bytes.
    pub fn decoded_bytes(&self) -> usize {
        self.title.len()
            + self.html.len()
            + self
                .literals
                .iter()
                .map(|literal| literal.value.len())
                .sum::<usize>()
    }
}

/// Inspect a completely decoded generated document in its original text order.
/// Literal strings enter as text, including carriage returns and HTML-looking code.
///
/// # Errors
/// Propagates native HTML/`MathML` validation, resource and consumer failures.
pub fn inspect<E>(
    decoded: &Decoded,
    limits: crate::html::Limits,
    consumer: impl FnMut(crate::html::TextKind, &str) -> Result<(), E>,
) -> Result<(), crate::html::Error<E>> {
    crate::html::inspect_generated(&decoded.html, &decoded.literals, limits, consumer)
}

/// No partial result is returned for unsupported grammar or exhausted bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Source is outside the closed generated grammar.
    Unsupported,
    /// A caller-selected ceiling was exhausted.
    Limit(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("unsupported generated literal MDX grammar"),
            Self::Limit(resource) => write!(f, "generated MDX {resource} ceiling exceeded"),
        }
    }
}
impl std::error::Error for Error {}

/// Decode only the generated literal representation without executing JavaScript.
///
/// # Errors
/// Refuses general MDX, malformed literals, unbalanced elements and resource exhaustion.
pub fn decode(bytes: &[u8], limits: Limits) -> Result<Decoded, Error> {
    if bytes.len() > limits.input_bytes {
        return Err(Error::Limit("input bytes"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| Error::Unsupported)?;
    let source = source
        .strip_prefix("---\ntitle: ")
        .ok_or(Error::Unsupported)?;
    let (title, source) = source.split_once('\n').ok_or(Error::Unsupported)?;
    let title: String = serde_json::from_str(title).map_err(|_| Error::Unsupported)?;
    let mut source = source.strip_prefix("---\n\n").ok_or(Error::Unsupported)?;
    if title.len() > limits.output_bytes {
        return Err(Error::Limit("output bytes"));
    }
    let mut output = Output {
        bytes: Vec::new(),
        maximum: limits.output_bytes - title.len(),
    };
    let mut stack = Vec::<&str>::new();
    let mut literals = Vec::new();
    let mut tokens = 0usize;
    while !source.is_empty() {
        tokens = tokens
            .checked_add(1)
            .filter(|count| *count <= limits.tokens)
            .ok_or(Error::Limit("tokens"))?;
        if let Some(rest) = source.strip_prefix("<span children={") {
            if stack.len() >= limits.depth {
                return Err(Error::Limit("depth"));
            }
            if !stack.iter().any(|name| protected(name)) || stack.contains(&"math") {
                return Err(Error::Unsupported);
            }
            if !rest.starts_with('"') {
                return Err(Error::Unsupported);
            }
            let mut strings = serde_json::Deserializer::from_str(rest).into_iter::<String>();
            let value = strings
                .next()
                .ok_or(Error::Unsupported)?
                .map_err(|_| Error::Unsupported)?;
            source = rest[strings.byte_offset()..]
                .strip_prefix("} />")
                .ok_or(Error::Unsupported)?;
            if value.len() > output.maximum.saturating_sub(output.bytes.len()) {
                return Err(Error::Limit("output bytes"));
            }
            output.maximum -= value.len();
            output.push("<span>")?;
            if value
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
            {
                return Err(Error::Unsupported);
            }
            let offset = output.bytes.len();
            output.push("</span>")?;
            literals.push(Literal { offset, value });
        } else if let Some(rest) = source.strip_prefix("</") {
            let (name, rest) = rest.split_once('>').ok_or(Error::Unsupported)?;
            if stack.pop() != Some(name) {
                return Err(Error::Unsupported);
            }
            output.push(&source[..source.len() - rest.len()])?;
            source = rest;
        } else if let Some(rest) = source.strip_prefix('<') {
            let name_end = rest
                .find(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
                .ok_or(Error::Unsupported)?;
            let name = &rest[..name_end];
            let math = name == "math" || stack.contains(&"math");
            if !(if math {
                math_element(name)
            } else {
                element(name)
            }) {
                return Err(Error::Unsupported);
            }
            let mut rest = &rest[name_end..];
            let mut attributes = BTreeSet::new();
            while rest.starts_with(' ') && !rest.starts_with(" />") {
                rest = &rest[1..];
                let (key, value) = rest.split_once("=\"").ok_or(Error::Unsupported)?;
                if !attributes.insert(key)
                    || !(if math {
                        math_attribute(key)
                    } else {
                        matches!(key, "id" | "title") || (key == "href" && name == "a")
                    })
                {
                    return Err(Error::Unsupported);
                }
                let (value, next) = value.split_once('"').ok_or(Error::Unsupported)?;
                inert(value)?;
                if key == "href"
                    && !(value.starts_with('/') && !value.starts_with("//")
                        || value.starts_with("https://")
                        || value.starts_with("http://"))
                {
                    return Err(Error::Unsupported);
                }
                rest = next;
            }
            let void = matches!(name, "br" | "hr" | "wbr");
            let rest = rest
                .strip_prefix(if void { " />" } else { ">" })
                .ok_or(Error::Unsupported)?;
            if !void {
                if stack.len() >= limits.depth {
                    return Err(Error::Limit("depth"));
                }
                stack.push(name);
            }
            output.push(&source[..source.len() - rest.len()])?;
            source = rest;
        } else {
            let end = source.find('<').unwrap_or(source.len());
            let value = &source[..end];
            inert(value)?;
            if stack.is_empty() && !value.bytes().all(|b| matches!(b, b' ' | b'\n')) {
                return Err(Error::Unsupported);
            }
            output.push(value)?;
            source = &source[end..];
        }
    }
    if !stack.is_empty() || tokens == 0 {
        return Err(Error::Unsupported);
    }
    Ok(Decoded {
        title,
        html: output.bytes,
        literals,
    })
}

fn inert(value: &str) -> Result<(), Error> {
    if value
        .chars()
        .any(|c| matches!(c, '<' | '{' | '}') || (c.is_control() && c != '\n'))
    {
        return Err(Error::Unsupported);
    }
    Ok(())
}

fn protected(name: &str) -> bool {
    matches!(
        name,
        "pre" | "code" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    )
}

fn element(name: &str) -> bool {
    matches!(
        name,
        "div"
            | "a"
            | "span"
            | "p"
            | "pre"
            | "code"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "ul"
            | "ol"
            | "li"
            | "dl"
            | "dt"
            | "dd"
            | "em"
            | "strong"
            | "b"
            | "i"
            | "s"
            | "del"
            | "sup"
            | "sub"
            | "blockquote"
            | "table"
            | "thead"
            | "tbody"
            | "tr"
            | "th"
            | "td"
            | "br"
            | "hr"
            | "wbr"
    )
}

fn math_element(name: &str) -> bool {
    matches!(
        name,
        "math"
            | "semantics"
            | "annotation"
            | "mrow"
            | "mi"
            | "mn"
            | "mo"
            | "mtext"
            | "mspace"
            | "mstyle"
            | "mpadded"
            | "mphantom"
            | "menclose"
            | "mfrac"
            | "msqrt"
            | "mroot"
            | "msub"
            | "msup"
            | "msubsup"
            | "mover"
            | "munder"
            | "munderover"
            | "mtable"
            | "mtr"
            | "mtd"
    )
}

fn math_attribute(name: &str) -> bool {
    matches!(
        name,
        "xmlns"
            | "display"
            | "encoding"
            | "accent"
            | "accentunder"
            | "fence"
            | "stretchy"
            | "separator"
            | "largeop"
            | "displaystyle"
            | "scriptlevel"
            | "mathvariant"
            | "mathcolor"
            | "mathbackground"
            | "mathsize"
            | "width"
            | "height"
            | "depth"
            | "lspace"
            | "rspace"
            | "voffset"
            | "minsize"
            | "maxsize"
            | "linethickness"
            | "rowspacing"
            | "columnspacing"
            | "columnalign"
            | "rowlines"
            | "columnlines"
            | "notation"
            | "linebreak"
            | "class"
            | "style"
    )
}

struct Output {
    bytes: Vec<u8>,
    maximum: usize,
}
impl Output {
    fn push(&mut self, value: &str) -> Result<(), Error> {
        if value.len() > self.maximum.saturating_sub(self.bytes.len()) {
            return Err(Error::Limit("output bytes"));
        }
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }
}
