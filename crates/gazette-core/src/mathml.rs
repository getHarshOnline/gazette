//! One bounded `MathML` presentation invariant shared by native parsers.
//!
//! Parsers retain ownership of lexical syntax and namespace decoding. This
//! module owns only the finite element, relationship, attribute and text
//! contract that Gazette is prepared to retain and inspect.

use cssparser::{Parser, ParserInput, Token};

/// Required namespace of every admitted root.
pub const NAMESPACE: &str = "http://www.w3.org/1998/Math/MathML";

/// Maximum nesting admitted by every parser feeding this invariant.
pub const MAX_DEPTH: usize = 64;
/// Maximum decoded attributes admitted on one element.
pub const MAX_ATTRIBUTES: usize = 16;
/// Maximum decoded bytes admitted in one attribute value.
pub const MAX_ATTRIBUTE_BYTES: usize = 8 << 10;
/// Maximum decoded bytes admitted in the retained TeX annotation.
pub const MAX_ANNOTATION_BYTES: usize = 8 << 10;

/// One decoded, unprefixed attribute supplied by a native parser.
#[derive(Clone, Copy, Debug)]
pub struct Attribute<'a> {
    /// Decoded local name; namespace declarations remain parser-owned.
    pub name: &'a str,
    /// Decoded value.
    pub value: &'a str,
}

/// Failure of the shared presentation invariant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The decoded structure or value is outside the admitted profile.
    Invalid,
    /// A named complete-fragment bound was exhausted.
    Limit(&'static str),
}

/// Incremental validator for one complete `MathML` root.
///
/// `expected_tex` and `expected_display` are used by retained typesetting to
/// bind a provider result to its exact request. Passing neither still enforces
/// the same safe presentation vocabulary for immutable-output inspection.
pub struct Validator {
    expected_tex: Option<String>,
    expected_display: Option<bool>,
    stack: Vec<Frame>,
    annotation: String,
    roots: usize,
}

impl Validator {
    /// Create one empty complete-fragment validator.
    pub fn new(expected_tex: Option<&str>, expected_display: Option<bool>) -> Self {
        Self {
            expected_tex: expected_tex.map(str::to_owned),
            expected_display,
            stack: Vec::new(),
            annotation: String::new(),
            roots: 0,
        }
    }

    /// Admit one decoded start element after the parser has proved namespace
    /// correctness and excluded namespace declarations from `attributes`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] for structure or values outside the profile,
    /// or [`Error::Limit`] when a complete-fragment bound is exhausted.
    pub fn start(&mut self, tag: &str, attributes: &[Attribute<'_>]) -> Result<(), Error> {
        if self.stack.len() >= MAX_DEPTH {
            return Err(Error::Limit("math depth"));
        }
        if attributes.len() > MAX_ATTRIBUTES {
            return Err(Error::Limit("math attributes"));
        }
        if !element(tag) {
            return Err(Error::Invalid);
        }
        for attribute in attributes {
            if attribute.name.len() > 64 || attribute.value.len() > MAX_ATTRIBUTE_BYTES {
                return Err(Error::Limit("math attribute bytes"));
            }
            if !allowed_attribute(tag, attribute.name, attribute.value) {
                return Err(Error::Invalid);
            }
        }
        if let Some(parent) = self.stack.last_mut() {
            if !child(&parent.tag, parent.children, tag) {
                return Err(Error::Invalid);
            }
            parent.children += 1;
        } else {
            self.roots += 1;
            if self.roots != 1 || tag != "math" {
                return Err(Error::Invalid);
            }
            let display = attributes
                .iter()
                .any(|attribute| attribute.name == "display" && attribute.value == "block");
            if self
                .expected_display
                .is_some_and(|expected| expected != display)
            {
                return Err(Error::Invalid);
            }
        }
        if tag == "annotation"
            && (attributes.len() != 1
                || attributes[0].name != "encoding"
                || attributes[0].value != "application/x-tex")
        {
            return Err(Error::Invalid);
        }
        self.stack.push(Frame {
            tag: tag.to_owned(),
            children: 0,
        });
        Ok(())
    }

    /// Admit one decoded text or whitespace run.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] outside text-bearing elements, or
    /// [`Error::Limit`] when the retained annotation bound is exhausted.
    pub fn text(&mut self, value: &str) -> Result<(), Error> {
        let parent = self.stack.last().ok_or(Error::Invalid)?;
        if parent.tag == "annotation" {
            if self.annotation.len().saturating_add(value.len()) > MAX_ANNOTATION_BYTES {
                return Err(Error::Limit("math annotation"));
            }
            self.annotation.push_str(value);
        } else if !matches!(parent.tag.as_str(), "mi" | "mn" | "mo" | "mtext")
            && !value.trim().is_empty()
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Admit one exact decoded end element.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] for a mismatched close or invalid arity.
    pub fn end(&mut self, tag: &str) -> Result<(), Error> {
        let frame = self.stack.pop().ok_or(Error::Invalid)?;
        if frame.tag != tag || !arity(&frame.tag, frame.children) {
            return Err(Error::Invalid);
        }
        Ok(())
    }

    /// Whether the single root has closed. Call [`Self::finish`] before success.
    pub fn root_closed(&self) -> bool {
        self.roots == 1 && self.stack.is_empty()
    }

    /// Finish one complete root without accepting an empty or mismatched TeX annotation.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Invalid`] unless one complete root closed with the
    /// required annotation and, when supplied, exact provider-request binding.
    pub fn finish(self) -> Result<(), Error> {
        if self.roots != 1 || !self.stack.is_empty() || self.annotation.is_empty() {
            return Err(Error::Invalid);
        }
        if self
            .expected_tex
            .as_deref()
            .is_some_and(|expected| expected != self.annotation)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

struct Frame {
    tag: String,
    children: usize,
}

fn element(tag: &str) -> bool {
    matches!(
        tag,
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

fn child(parent: &str, index: usize, tag: &str) -> bool {
    match parent {
        "math" => index == 0 && tag == "semantics",
        "semantics" => {
            if index == 1 {
                tag == "annotation"
            } else {
                index == 0 && presentation(tag)
            }
        }
        "annotation" | "mspace" => false,
        "mtable" => tag == "mtr",
        "mtr" => tag == "mtd",
        _ => presentation(tag) && !matches!(tag, "mtr" | "mtd"),
    }
}

fn presentation(tag: &str) -> bool {
    !matches!(tag, "math" | "semantics" | "annotation")
}

fn arity(tag: &str, children: usize) -> bool {
    match tag {
        "math" => children == 1,
        "semantics" | "mfrac" | "mroot" | "msub" | "msup" | "mover" | "munder" => children == 2,
        "msubsup" | "munderover" => children == 3,
        "annotation" | "mspace" => children == 0,
        _ => true,
    }
}

fn allowed_attribute(tag: &str, name: &str, value: &str) -> bool {
    match name {
        "display" => tag == "math" && value == "block",
        "encoding" => tag == "annotation" && value == "application/x-tex",
        "accent" => tag == "mover" && boolean(value),
        "accentunder" => tag == "munder" && boolean(value),
        "fence" | "stretchy" | "separator" | "largeop" => tag == "mo" && boolean(value),
        "displaystyle" => tag == "mstyle" && boolean(value),
        "scriptlevel" => tag == "mstyle" && matches!(value, "0" | "1" | "2"),
        "mathvariant" => {
            matches!(tag, "mi" | "mn" | "mo" | "mtext")
                && matches!(
                    value,
                    "normal"
                        | "bold"
                        | "italic"
                        | "bold-italic"
                        | "double-struck"
                        | "script"
                        | "bold-script"
                        | "fraktur"
                        | "bold-fraktur"
                        | "sans-serif"
                        | "bold-sans-serif"
                        | "sans-serif-italic"
                        | "sans-serif-bold-italic"
                        | "monospace"
                )
        }
        "mathcolor" | "mathbackground" => {
            matches!(tag, "mstyle" | "mo" | "mpadded" | "mspace") && color(value)
        }
        "mathsize" => tag == "mstyle" && dimensions(value, false),
        "width" => {
            matches!(tag, "mspace" | "mpadded" | "mtable" | "mtd") && dimensions(value, false)
        }
        "height" | "depth" => matches!(tag, "mspace" | "mpadded") && dimensions(value, false),
        "lspace" | "rspace" => matches!(tag, "mo" | "mpadded") && dimensions(value, false),
        "voffset" => tag == "mpadded" && dimensions(value, false),
        "minsize" | "maxsize" => tag == "mo" && dimensions(value, false),
        "linethickness" => tag == "mfrac" && dimensions(value, false),
        "rowspacing" | "columnspacing" => tag == "mtable" && dimensions(value, true),
        "columnalign" => tag == "mtable" && words(value, &["left", "center", "right"]),
        "rowlines" | "columnlines" => tag == "mtable" && words(value, &["none", "solid", "dashed"]),
        "notation" => {
            tag == "menclose"
                && words(
                    value,
                    &[
                        "updiagonalstrike",
                        "downdiagonalstrike",
                        "phasorangle",
                        "horizontalstrike",
                        "box",
                        "actuarial",
                        "top",
                        "bottom",
                        "left",
                        "right",
                    ],
                )
        }
        "linebreak" => tag == "mspace" && value == "newline",
        "class" => matches!(
            (tag, value),
            ("mtd", "mtr-glue" | "mml-eqn-num") | ("mpadded", "vcenter")
        ),
        "style" => style(tag, value),
        _ => false,
    }
}

fn boolean(value: &str) -> bool {
    matches!(value, "true" | "false")
}

fn words(value: &str, allowed: &[&str]) -> bool {
    value.split_ascii_whitespace().next().is_some()
        && value
            .split_ascii_whitespace()
            .all(|part| allowed.contains(&part))
}

fn length(token: &Token<'_>) -> bool {
    match token {
        Token::Number { value, .. } => *value == 0.0,
        Token::Dimension { value, unit, .. } => {
            value.is_finite()
                && value.abs() <= 10_000.0
                && matches!(
                    unit.as_ref(),
                    "em" | "ex" | "px" | "pt" | "width" | "height" | "depth"
                )
        }
        Token::Percentage { unit_value, .. } => unit_value.is_finite() && unit_value.abs() <= 100.0,
        _ => false,
    }
}

fn dimensions(value: &str, multiple: bool) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut count = 0;
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
        if matches!(token, Token::WhiteSpace(_)) {
            continue;
        }
        count += 1;
        if count > 256 || (!multiple && count > 1) || !length(token) {
            return false;
        }
    }
    count > 0 && parser.is_exhausted()
}

fn color_token(token: &Token<'_>) -> bool {
    match token {
        Token::Ident(name) => {
            name.as_ref() == "transparent" || cssparser::color::parse_named_color(name).is_ok()
        }
        Token::Hash(value) | Token::IDHash(value) => {
            cssparser::color::parse_hash_color(value.as_bytes()).is_ok()
        }
        _ => false,
    }
}

fn color(value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    parser.next().is_ok_and(color_token) && parser.is_exhausted()
}

fn style(tag: &str, value: &str) -> bool {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    if tag == "mstyle"
        && parser
            .try_parse(|parser| parser.expect_ident_matching("text-shadow"))
            .is_ok()
    {
        return parser.expect_colon().is_ok()
            && (0..3).all(|_| parser.next().is_ok_and(length))
            && parser.is_exhausted();
    }
    tag == "mpadded"
        && parser.expect_ident_matching("border").is_ok()
        && parser.expect_colon().is_ok()
        && parser.next().is_ok_and(length)
        && parser.expect_ident_matching("solid").is_ok()
        && parser.next().is_ok_and(color_token)
        && parser.is_exhausted()
}
