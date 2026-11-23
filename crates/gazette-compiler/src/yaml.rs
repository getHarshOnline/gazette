//! Bounded YAML values with original-source locations, without publication admission.
//!
//! This deliberately excludes aliases, anchors, tags, complex keys and multiple
//! documents. A landing is a readable declaration, not an executable YAML graph.

use std::{collections::BTreeMap, fmt, str::Chars};

use gazette_core::source::{Sha256Digest, Span};
use serde::de::DeserializeOwned;
use serde_json::Value;
use yaml_rust2::{
    parser::{Event, Parser},
    scanner::{Marker, TScalarStyle},
};

use crate::toml::PathSegment;

/// Bounds checked before retaining an expanded value tree.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum complete original UTF-8 input bytes.
    pub source_bytes: usize,
    /// Maximum values and mapping keys, including the root.
    pub nodes: usize,
    /// Maximum container depth below the root.
    pub depth: usize,
    /// Maximum aggregate decoded scalar bytes.
    pub decoded_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1024 * 1024,
            nodes: 10_000,
            depth: 64,
            decoded_bytes: 1024 * 1024,
        }
    }
}

/// A bounded syntax or typed-projection error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    /// Stable class without source text.
    pub code: &'static str,
    /// Explanation without the rejected scalar contents.
    pub message: String,
    /// Original UTF-8 source coordinates, when syntax identified a location.
    pub span: Option<Span>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        if let Some(span) = self.span {
            write!(f, " at bytes {}..{}", span.start, span.end)?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}

/// Retained source and decoded data. Locations identify containing constructs,
/// never an exact replacement range inside an escaped or multiline scalar.
#[derive(Debug)]
pub struct ParsedYaml<'s> {
    source: &'s str,
    value: Value,
    locations: LocationNode,
    nodes: usize,
}

impl<'s> ParsedYaml<'s> {
    /// Parse one strict YAML mapping, preserving complete-source identity.
    ///
    /// # Errors
    /// Refuses exhausted bounds, ambiguous mappings and unsupported graph syntax.
    pub fn parse(source: &'s str, limits: Limits) -> Result<Self, Error> {
        if source.len() > limits.source_bytes {
            return Err(limit("source bytes"));
        }
        let mut reader = Reader {
            source,
            parser: Parser::new_from_str(source),
            // The pinned scanner mixes byte and character increments for its
            // absolute index. Line starts plus character columns remain stable
            // after multiline scalar content; never infer bytes from that index.
            line_offsets: line_offsets(source),
            limits,
            nodes: 0,
            decoded_bytes: 0,
        };
        reader.expect(Event::StreamStart)?;
        reader.expect(Event::DocumentStart)?;
        let (value, locations) = reader.value(0)?;
        if !value.is_object() {
            return Err(reader.invalid("root must be a mapping", 0));
        }
        reader.expect(Event::DocumentEnd)?;
        reader.expect(Event::StreamEnd)?;
        Ok(Self {
            source,
            value,
            locations,
            nodes: reader.nodes,
        })
    }

    /// Exact original source, not normalized YAML.
    pub fn source(&self) -> &'s str {
        self.source
    }
    /// Complete original byte identity.
    pub fn source_sha256(&self) -> Sha256Digest {
        Sha256Digest::of(self.source.as_bytes())
    }
    /// Number of retained values and keys.
    pub fn node_count(&self) -> usize {
        self.nodes
    }
    /// Decoded data for an explicit consumer schema.
    pub fn value(&self) -> &Value {
        &self.value
    }
    /// Source construct containing an explicitly authored value.
    pub fn span_at(&self, path: &[PathSegment]) -> Option<Span> {
        let mut node = &self.locations;
        for segment in path {
            node = node.children.get(segment)?;
        }
        Some(node.span)
    }
    /// Original mapping entry including its authored key spelling.
    pub(crate) fn entry_span_at(&self, path: &[PathSegment]) -> Option<Span> {
        let mut node = &self.locations;
        for segment in path {
            node = node.children.get(segment)?;
        }
        Some(node.entry_span)
    }
    /// Deserialize through the consumer's own schema; no parallel identity model.
    ///
    /// # Errors
    /// Returns a schema error; absent fields never acquire invented locations.
    pub fn deserialize<T: DeserializeOwned>(&self) -> Result<T, Error> {
        T::deserialize(&self.value).map_err(|error| Error {
            code: "yaml_schema",
            message: error.to_string(),
            span: None,
        })
    }
}

#[derive(Debug)]
struct LocationNode {
    span: Span,
    entry_span: Span,
    children: BTreeMap<PathSegment, LocationNode>,
}

struct Reader<'s> {
    source: &'s str,
    parser: Parser<Chars<'s>>,
    line_offsets: Vec<usize>,
    limits: Limits,
    nodes: usize,
    decoded_bytes: usize,
}

fn limit(resource: &'static str) -> Error {
    Error {
        code: "yaml_limit",
        message: format!("{resource} ceiling exceeded"),
        span: None,
    }
}

fn line_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    let mut chars = source.char_indices().peekable();
    while let Some((byte, ch)) = chars.next() {
        match ch {
            '\r' if chars.peek().is_some_and(|(_, next)| *next == '\n') => {
                let (newline, _) = chars.next().expect("peeked newline");
                offsets.push(newline + 1);
            }
            '\r' | '\n' => offsets.push(byte + 1),
            _ => (),
        }
    }
    offsets
}

impl Reader<'_> {
    fn byte(&self, marker: Marker) -> usize {
        let Some(&start) = self.line_offsets.get(marker.line().saturating_sub(1)) else {
            return self.source.len();
        };
        let line = self.source[start..]
            .split(['\r', '\n'])
            .next()
            .unwrap_or("");
        start
            + line
                .char_indices()
                .nth(marker.col())
                .map_or(line.len(), |(byte, _)| byte)
    }
    fn invalid(&self, message: &str, byte: usize) -> Error {
        Error {
            code: "yaml_invalid",
            message: message.into(),
            span: Some(Span {
                start: byte,
                end: byte,
            }),
        }
    }
    fn next(&mut self) -> Result<(Event, usize), Error> {
        match self.parser.next_token() {
            Ok((event, marker)) => Ok((event, self.byte(marker))),
            Err(error) => Err(self.invalid("invalid YAML syntax", self.byte(*error.marker()))),
        }
    }
    fn peek(&mut self) -> Result<(Event, usize), Error> {
        match self.parser.peek().cloned() {
            Ok((event, marker)) => Ok((event, self.byte(marker))),
            Err(error) => Err(self.invalid("invalid YAML syntax", self.byte(*error.marker()))),
        }
    }
    fn expect(&mut self, wanted: Event) -> Result<(), Error> {
        let (actual, at) = self.next()?;
        if actual != wanted {
            return Err(self.invalid("expected one complete YAML document", at));
        }
        Ok(())
    }
    fn charge(&mut self) -> Result<(), Error> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .filter(|n| *n <= self.limits.nodes)
            .ok_or_else(|| limit("nodes"))?;
        Ok(())
    }
    fn scalar(&mut self, text: String) -> Result<String, Error> {
        self.decoded_bytes = self
            .decoded_bytes
            .checked_add(text.len())
            .filter(|n| *n <= self.limits.decoded_bytes)
            .ok_or_else(|| limit("decoded bytes"))?;
        Ok(text)
    }
    fn value(&mut self, depth: usize) -> Result<(Value, LocationNode), Error> {
        if depth > self.limits.depth {
            return Err(limit("depth"));
        }
        self.charge()?;
        let (event, start) = self.next()?;
        let mut children = BTreeMap::new();
        let value = match event {
            Event::Scalar(text, style, 0, None) => {
                let text = self.scalar(text)?;
                if style == TScalarStyle::Plain {
                    match text.as_str() {
                        "null" | "Null" | "NULL" | "~" | "" => Value::Null,
                        "true" | "True" | "TRUE" => Value::Bool(true),
                        "false" | "False" | "FALSE" => Value::Bool(false),
                        _ => match serde_json::from_str::<Value>(&text) {
                            Ok(value @ Value::Number(_)) => value,
                            _ => Value::String(text),
                        },
                    }
                } else {
                    Value::String(text)
                }
            }
            Event::SequenceStart(0, None) => {
                let mut items = Vec::new();
                while self.peek()?.0 != Event::SequenceEnd {
                    let (value, location) = self.value(depth + 1)?;
                    children.insert(PathSegment::Index(items.len()), location);
                    items.push(value);
                }
                self.next()?;
                Value::Array(items)
            }
            Event::MappingStart(0, None) => {
                let mut items = serde_json::Map::new();
                while self.peek()?.0 != Event::MappingEnd {
                    self.charge()?;
                    let (key, at) = self.next()?;
                    let Event::Scalar(key, _, 0, None) = key else {
                        return Err(self.invalid("mapping keys must be untagged strings", at));
                    };
                    let key = self.scalar(key)?;
                    if key == "<<" {
                        return Err(self.invalid("merge keys are unsupported", at));
                    }
                    if items.contains_key(&key) {
                        return Err(self.invalid("duplicate mapping key", at));
                    }
                    let (value, mut location) = self.value(depth + 1)?;
                    location.entry_span.start = at;
                    children.insert(PathSegment::Key(key.clone()), location);
                    items.insert(key, value);
                }
                self.next()?;
                Value::Object(items)
            }
            _ => {
                return Err(self.invalid(
                    "anchors, aliases, tags and non-value events are unsupported",
                    start,
                ));
            }
        };
        let end = self.peek()?.1.max(start);
        Ok((
            value,
            LocationNode {
                span: Span { start, end },
                entry_span: Span { start, end },
                children,
            },
        ))
    }
}
