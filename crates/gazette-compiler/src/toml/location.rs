//! Source coordinates come from the retained parser tree, never textual key searches.

use serde::{Deserialize, Serialize};

use super::{Item, Node, ParsedToml, Span, Value};

/// A decoded TOML key or zero-based array position, not a dotted string convention.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(untagged)]
pub enum PathSegment {
    /// Exact decoded key; dots and numeric spelling remain ordinary key characters.
    Key(String),
    /// Position in an array or array of tables, in original source order.
    Index(#[schemars(range(max = usize::MAX))] usize),
}

impl ParsedToml<'_> {
    /// Exact parser-owned key spelling and decoded key, distinct from its value.
    pub(crate) fn key_at(&self, path: &[PathSegment]) -> Option<(Span, &str)> {
        let (last, parent) = path.split_last()?;
        let PathSegment::Key(name) = last else {
            return None;
        };
        let mut node = Node::Item(self.document.as_item());
        for segment in parent {
            node = node.child(segment)?;
        }
        let native = match node {
            Node::Item(Item::Table(table)) | Node::Table(table) => table.key(name)?,
            Node::Item(Item::Value(Value::InlineTable(table)))
            | Node::Value(Value::InlineTable(table)) => table.key(name)?,
            _ => return None,
        };
        Some((Span::checked(self.source(), native.span()?)?, native.get()))
    }

    /// Exact original string scalar and its native decoded value, not a container.
    pub(crate) fn string_at(&self, path: &[PathSegment]) -> Option<(Span, &str)> {
        let mut node = Node::Item(self.document.as_item());
        for segment in path {
            node = node.child(segment)?;
        }
        let value = match node {
            Node::Item(Item::Value(value)) | Node::Value(value) => value,
            _ => return None,
        };
        Some((
            Span::checked(self.source(), value.span()?)?,
            value.as_str()?,
        ))
    }

    /// Locate a value or explicit container in the original TOML bytes.
    ///
    /// Missing keys, wrong container kinds, out-of-range indices and implicit
    /// containers without a native span return `None`. No value is reserialized,
    /// decoded again or searched as text. Ranges are UTF-8 bytes, not LSP positions.
    pub fn span_at(&self, path: &[PathSegment]) -> Option<Span> {
        let mut node = Node::Item(self.document.as_item());
        for segment in path {
            node = node.child(segment)?;
        }
        let span = match node {
            Node::Item(item) => item.span(),
            Node::Table(table) => table.span(),
            Node::Value(value) => value.span(),
        }?;
        Span::checked(self.source(), span)
    }
}

impl<'a> Node<'a> {
    fn child(self, segment: &PathSegment) -> Option<Self> {
        match (self, segment) {
            (Self::Item(Item::Table(table)) | Self::Table(table), PathSegment::Key(key)) => {
                table.get(key).map(Self::Item)
            }
            (Self::Item(Item::ArrayOfTables(array)), PathSegment::Index(index)) => {
                array.get(*index).map(Self::Table)
            }
            (Self::Item(Item::Value(value)), segment) => Self::Value(value).child(segment),
            (Self::Value(Value::InlineTable(table)), PathSegment::Key(key)) => {
                table.get(key).map(Self::Value)
            }
            (Self::Value(Value::Array(array)), PathSegment::Index(index)) => {
                array.get(*index).map(Self::Value)
            }
            _ => None,
        }
    }
}
