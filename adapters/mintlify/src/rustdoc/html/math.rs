//! Feed native `MathML` into Gazette's shared presentation invariant.

use super::{Node, State, Tag, attribute};
use crate::Error;
use gazette_core::mathml::{self, Attribute, Validator};

impl State<'_> {
    pub(super) fn math_start(&mut self, tag: &Tag) -> Result<bool, Error> {
        let name = tag.name.as_ref();
        let root = self.math.is_none();
        if root && !(self.inside && name == "math") {
            return Ok(false);
        }
        if tag.attrs.len() > mathml::MAX_ATTRIBUTES
            || tag.attrs.iter().any(|attr| {
                attr.name.prefix.is_some()
                    || !attr.name.ns.is_empty()
                    || attr.name.local.contains(':')
            })
            || self
                .stack
                .iter()
                .any(|frame| frame.hidden || frame.heading || frame.line_number.is_some())
        {
            return Err(self.fail("unsupported Rustdoc MathML context or attributes"));
        }
        if root {
            if attribute(tag, "xmlns") != Some(mathml::NAMESPACE) {
                return Err(self.fail("invalid Rustdoc MathML namespace"));
            }
            self.math = Some(Validator::new(None, None));
        } else if attribute(tag, "xmlns").is_some() {
            return Err(self.fail("nested Rustdoc MathML namespace declaration"));
        }
        let attributes: Vec<_> = tag
            .attrs
            .iter()
            .filter(|attr| attr.name.local.as_ref() != "xmlns")
            .map(|attr| Attribute {
                name: attr.name.local.as_ref(),
                value: attr.value.as_ref(),
            })
            .collect();
        let Some(math) = self.math.as_mut() else {
            return Err(self.fail("missing Rustdoc MathML presentation"));
        };
        math.start(name, &attributes)
            .map_err(|_| self.fail("invalid Rustdoc MathML presentation"))?;
        self.page.nodes.push(Node::Start {
            name: name.into(),
            attributes: tag
                .attrs
                .iter()
                .map(|attr| (attr.name.local.to_string(), attr.value.to_string()))
                .collect(),
        });
        if tag.self_closing {
            self.math_end(name)?;
        }
        Ok(true)
    }

    pub(super) fn math_end(&mut self, name: &str) -> Result<(), Error> {
        let Some(math) = self.math.as_mut() else {
            return Err(self.fail("missing Rustdoc MathML presentation"));
        };
        let closed = match math.end(name) {
            Ok(()) => math.root_closed(),
            Err(_) => return Err(self.fail("invalid Rustdoc MathML closing element")),
        };
        if closed {
            let Some(math) = self.math.take() else {
                return Err(self.fail("missing Rustdoc MathML presentation"));
            };
            math.finish()
                .map_err(|_| self.fail("incomplete Rustdoc MathML presentation"))?;
        }
        self.page.nodes.push(Node::End(name.into()));
        Ok(())
    }
}
