//! Finite inert SVG vocabulary over the existing HTML tokenizer's tokens.
//!
//! This inspects path geometry and accessible text, not arbitrary SVG, external
//! resources, rendering validity or runtime mutations. The caller owns all quotas.

use html5ever::tokenizer::Tag;

pub(super) struct Validator {
    child: Option<&'static str>,
}

impl Validator {
    pub(super) fn root(tag: &Tag) -> Option<Self> {
        tag.attrs
            .iter()
            .all(|attribute| match attribute.name.local.as_ref() {
                "xmlns" => attribute.value.as_ref() == "http://www.w3.org/2000/svg",
                "viewbox" => numbers(&attribute.value),
                "preserveaspectratio" => aspect_ratio(&attribute.value),
                "class" | "id" | "role" | "aria-hidden" | "aria-label" => true,
                _ => false,
            })
            .then_some(Self { child: None })
    }

    pub(super) fn start(&mut self, tag: &Tag) -> bool {
        if self.child.is_some() {
            return false;
        }
        let name = match tag.name.as_ref() {
            "path" => "path",
            "title" => "title",
            "desc" => "desc",
            _ => return false,
        };
        if !tag
            .attrs
            .iter()
            .all(|attribute| match attribute.name.local.as_ref() {
                "class" | "id" => true,
                "d" if name == "path" => attribute
                    .value
                    .bytes()
                    .all(|byte| number_byte(byte) || b"MmZzLlHhVvCcSsQqTtAa".contains(&byte)),
                "vector-effect" if name == "path" => matches!(
                    super::trim_ascii_whitespace(&attribute.value),
                    "none" | "non-scaling-stroke"
                ),
                _ => false,
            })
        {
            return false;
        }
        if !tag.self_closing {
            self.child = Some(name);
        }
        true
    }

    /// Return whether this is the root's close; an unmatched close is refused.
    pub(super) fn end(&mut self, name: &str) -> Option<bool> {
        match self.child {
            Some(child) if child == name => {
                self.child = None;
                Some(false)
            }
            None if name == "svg" => Some(true),
            _ => None,
        }
    }

    pub(super) fn text(&self, value: &str) -> bool {
        matches!(self.child, Some("title" | "desc"))
            || value
                .bytes()
                .all(|byte| matches!(byte, b'\t' | b'\n' | b'\x0c' | b'\r' | b' '))
    }
}

// Geometry is not interpreted or certified. This lexical subset cannot carry a
// CSS string, escape, function or URL requiring another decoding language.
fn number_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || b"eE+-. ,\t\n\r\x0c".contains(&byte)
}

fn numbers(value: &str) -> bool {
    value.bytes().all(number_byte)
}

fn aspect_ratio(value: &str) -> bool {
    let mut words = value.split_ascii_whitespace();
    match words.next() {
        Some("none") => words.next().is_none(),
        Some(
            "xMinYMin" | "xMidYMin" | "xMaxYMin" | "xMinYMid" | "xMidYMid" | "xMaxYMid"
            | "xMinYMax" | "xMidYMax" | "xMaxYMax",
        ) => matches!(words.next(), None | Some("meet" | "slice")) && words.next().is_none(),
        _ => false,
    }
}
