//! Structural addresses over the exact serialized projection, not another semantic graph.

use serde_json::Value;

/// One occurrence within a surface; a container is reviewed as well as its children.
pub struct ReviewUse<'a> {
    /// RFC 6901 pointer relative to the surface value; the root is the empty string.
    pub pointer: String,
    /// Borrowed scalar or container; keys, array order and repeated references remain intact.
    pub value: &'a Value,
}

/// Deterministic depth-first traversal of an immutable, bounded review surface.
pub struct ReviewUses<'a> {
    root: Option<&'a Value>,
    stack: Vec<(String, Children<'a>)>,
}

impl<'a> ReviewUses<'a> {
    pub(super) fn new(value: &'a Value) -> Self {
        Self {
            root: Some(value),
            stack: Vec::new(),
        }
    }
}

impl<'a> Iterator for ReviewUses<'a> {
    type Item = ReviewUse<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let usage = if let Some(value) = self.root.take() {
            ReviewUse {
                pointer: String::new(),
                value,
            }
        } else {
            loop {
                let (parent, children) = self.stack.last_mut()?;
                if let Some((key, value)) = children.next() {
                    break ReviewUse {
                        pointer: format!("{parent}/{key}"),
                        value,
                    };
                }
                self.stack.pop();
            }
        };
        match usage.value {
            Value::Array(items) => {
                self.stack.push((
                    usage.pointer.clone(),
                    Children::Array(items.iter().enumerate()),
                ));
            }
            Value::Object(fields) => {
                self.stack
                    .push((usage.pointer.clone(), Children::Object(fields.iter())));
            }
            _ => {}
        }
        Some(usage)
    }
}

// Retain only the ancestor chain, not allocated pointers for every pending sibling.
enum Children<'a> {
    Array(std::iter::Enumerate<std::slice::Iter<'a, Value>>),
    Object(serde_json::map::Iter<'a>),
}

impl<'a> Children<'a> {
    fn next(&mut self) -> Option<(String, &'a Value)> {
        match self {
            Self::Array(items) => items
                .next()
                .map(|(index, value)| (index.to_string(), value)),
            Self::Object(fields) => fields
                .next()
                .map(|(key, value)| (key.replace('~', "~0").replace('/', "~1"), value)),
        }
    }
}
