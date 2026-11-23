//! Bounded TOML syntax with retained source and caller-owned typed projections.

use std::fmt;

use gazette_core::source::Sha256Digest;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use toml_edit::{Document, Item, Table, Value};

use gazette_core::source::Span;

mod location;
mod set;
pub use location::PathSegment;
pub use set::{DocumentError, NamedInspection, ParsedSet, SetError, SetInspection, SetLimits};

/// Logical TOML limits, in addition to the pinned parser's own recursion guard.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum source bytes, checked before parsing.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// Maximum semantic syntax nodes, including the root and containers.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Maximum nesting below the root table (depth zero).
    #[schemars(range(max = usize::MAX))]
    pub depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 64 * 1024,
            nodes: 10_000,
            depth: 64,
        }
    }
}

/// A syntax, resource or caller-schema failure; it grants no partial admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A named logical ceiling was exhausted.
    Limit(&'static str),
    /// TOML syntax or the caller's deserializer rejected this source.
    Invalid {
        /// Diagnostic without a copy of the source document.
        message: String,
        /// Half-open byte range relative to this TOML source, when available.
        span: Option<Span>,
    },
}

impl Error {
    /// Stable diagnostic class; source text is not embedded in the class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Limit(_) => "toml_limit",
            Self::Invalid { .. } => "toml_invalid",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(resource) => write!(f, "TOML {resource} ceiling exceeded"),
            Self::Invalid { message, span } => {
                write!(f, "invalid TOML or typed projection: {message:?}")?;
                if let Some(span) = span {
                    write!(f, " at bytes {}..{}", span.start, span.end)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Error {}

/// Structural metadata only; keys are not a claim that their meanings are admitted.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact unmodified TOML bytes, including comments and whitespace.
    pub source_sha256: Sha256Digest,
    /// Root, container and value node count.
    #[schemars(range(max = usize::MAX))]
    pub node_count: usize,
    /// Complete top-level keys in source order; quoted/dotted syntax is parser-owned.
    pub keys: Vec<String>,
}

/// Immutable parsed TOML, retaining the exact borrowed source and native syntax tree.
///
/// No field defaults, lifecycle rules, imports, paths or publisher meaning are
/// inferred here. Unknown fields remain present until an explicit consumer schema
/// handles or rejects them. Parser-specific types do not cross the public API.
pub struct ParsedToml<'s> {
    limits: Limits,
    document: Document<&'s str>,
    node_count: usize,
}

impl fmt::Debug for ParsedToml<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedToml")
            .field("node_count", &self.node_count)
            .finish()
    }
}

impl<'s> ParsedToml<'s> {
    /// Parse a complete TOML document without discarding unknown fields.
    ///
    /// # Errors
    ///
    /// Rejects malformed/duplicate declarations and exhausted byte/node/depth
    /// ceilings. Parser allocation overhead is additional to these logical limits.
    pub fn parse(source: &'s str, limits: Limits) -> Result<Self, Error> {
        if source.len() > limits.source_bytes {
            return Err(Error::Limit("source bytes"));
        }
        let document = Document::parse(source).map_err(|error| Error::Invalid {
            message: error.message().to_owned(),
            span: error.span().and_then(|span| Span::checked(source, span)),
        })?;
        let node_count = count_nodes(document.as_item(), limits)?;
        Ok(Self {
            limits,
            document,
            node_count,
        })
    }

    /// Exact source, without reserialization, field omission or formatting changes.
    pub fn source(&self) -> &str {
        self.document.raw()
    }

    /// Carry caller ceilings into bounded compiler projections without reparsing.
    pub(crate) fn limits(&self) -> Limits {
        self.limits
    }

    /// Retained syntax-node count, without allocating an inspection projection.
    pub fn node_count(&self) -> usize {
        self.node_count
    }

    /// Whether the original root table explicitly contains this exact key.
    ///
    /// This observes syntax presence only. It does not decode a value, follow an
    /// include, or assign meaning to the key.
    pub(crate) fn has_root_key(&self, key: &str) -> bool {
        self.document.as_table().contains_key(key)
    }

    /// Inspect all root keys and structural identity, not metadata values or approval.
    pub fn inspect(&self) -> Inspection {
        Inspection {
            source_sha256: Sha256Digest::of(self.source().as_bytes()),
            node_count: self.node_count,
            keys: self
                .document
                .iter()
                .map(|(key, _)| key.to_owned())
                .collect(),
        }
    }

    /// Project the retained syntax tree into the caller's explicit Rust schema.
    ///
    /// This clones the bounded syntax tree for Serde's consuming API; it does not
    /// reparse the source. The caller owns required/unknown-field and semantic
    /// validation. Successful deserialization alone is not publication admission.
    ///
    /// # Errors
    ///
    /// Returns the caller schema's deserialization failure with TOML-local ranges.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, Error> {
        T::deserialize(toml_edit::de::Deserializer::from(self.document.clone())).map_err(|error| {
            Error::Invalid {
                message: error.message().to_owned(),
                span: error
                    .span()
                    .and_then(|span| Span::checked(self.source(), span)),
            }
        })
    }
}

enum Node<'a> {
    Item(&'a Item),
    Value(&'a Value),
    Table(&'a Table),
}

fn count_nodes(root: &Item, limits: Limits) -> Result<usize, Error> {
    if limits.nodes == 0 {
        return Err(Error::Limit("nodes"));
    }
    let mut stack = vec![(Node::Item(root), 0_usize)];
    let mut count = 1;
    while let Some((node, depth)) = stack.pop() {
        let mut push = |node| {
            if depth >= limits.depth {
                return Err(Error::Limit("depth"));
            }
            if count >= limits.nodes {
                return Err(Error::Limit("nodes"));
            }
            count += 1;
            stack.push((node, depth + 1));
            Ok(())
        };
        match node {
            Node::Item(Item::Table(table)) | Node::Table(table) => {
                for (_, item) in table.iter() {
                    push(Node::Item(item))?;
                }
            }
            Node::Item(Item::ArrayOfTables(array)) => {
                for table in array.iter() {
                    push(Node::Table(table))?;
                }
            }
            Node::Item(Item::Value(value)) | Node::Value(value) => match value {
                Value::Array(array) => {
                    for value in array.iter() {
                        push(Node::Value(value))?;
                    }
                }
                Value::InlineTable(table) => {
                    for (_, value) in table.iter() {
                        push(Node::Value(value))?;
                    }
                }
                _ => (),
            },
            Node::Item(Item::None) => {
                return Err(Error::Invalid {
                    message: "empty internal TOML item".into(),
                    span: None,
                });
            }
        }
    }
    Ok(count)
}
