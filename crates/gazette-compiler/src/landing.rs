//! Structured landing syntax lowered into the existing Markdown/component compiler.
//!
//! The YAML file is the source of truth. Normalized Markdown is an in-memory
//! compiler representation, never an authored companion or a replacement identity.

use gazette_core::{
    markdown::{ComponentName, Profile},
    source::{Location, Precision, Sha256Digest, Span},
};
use gazette_ports::markdown::Markdown;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{document, markdown, toml::PathSegment, yaml};

/// Admission rule for a section without a selected Theme component.
pub const UNNAMED_SECTION_REQUIREMENT: &str =
    "an unnamed section requires a heading, nonempty body, or child section";

/// An ordered authored group, with an optional configured component boundary.
/// Component contracts, resources and presentation remain resolved by Gazette.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// Selected component name; absence is an ordinary semantic group.
    #[serde(default)]
    pub component: Option<ComponentName>,
    /// Typed component arguments checked against the selected theme's contract.
    #[serde(default)]
    pub arguments: serde_json::Map<String, Value>,
    /// Optional explicit heading; the existing compiler owns anchor allocation.
    #[serde(default)]
    pub heading: Option<Heading>,
    /// Authored Markdown within this group, resolved in the complete page scope.
    #[serde(default)]
    pub body: Option<String>,
    /// Nested groups in authored order.
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// A heading's meaning, without a typography or positioning instruction.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Heading {
    /// Heading level, from one through six.
    pub level: u8,
    /// Single-line plain text; Markdown punctuation is escaped during lowering.
    pub text: String,
}

/// Retained original declaration and its one normalized body.
#[derive(Debug)]
pub struct ParsedLanding<'s> {
    source: yaml::ParsedYaml<'s>,
    metadata: Value,
    body: markdown::ParsedMarkdown<'s>,
    origins: Vec<BodyOrigin>,
}

/// Native source coordinate for one explicitly declared YAML component.
#[derive(Clone, Debug)]
pub struct ComponentSlot {
    /// Parsed component identity.
    pub name: ComponentName,
    /// Original name range. Only exact locations are editable.
    pub name_location: Location,
    /// Direct arguments with source coordinates from the same YAML parse.
    pub arguments: Vec<ArgumentSlot>,
}

/// An authored argument and its verified token ranges where available.
#[derive(Clone, Debug)]
pub struct ArgumentSlot {
    /// Decoded key as supplied to the component schema.
    pub name: String,
    /// Original key token, including quotes when present.
    pub key: Option<Span>,
    /// Original value, exact only for directly proved scalar spellings.
    pub value: Location,
}

/// Normalized byte range and its containing original YAML construct.
#[derive(Clone, Debug)]
struct BodyOrigin {
    normalized: Span,
    original: Span,
}

/// Syntax, schema, bound or normalized-Markdown failure.
#[derive(Debug)]
pub enum Error {
    /// Original-source YAML failure.
    Yaml(yaml::Error),
    /// Structured landing declaration failure at its original construct.
    Invalid {
        /// Bounded explanation.
        reason: &'static str,
        /// Original source range, if authored.
        span: Option<Span>,
    },
    /// Existing Markdown admission failed; coordinates belong to normalized body.
    Markdown(markdown::Error),
}

impl Error {
    /// Stable native failure class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Yaml(error) => error.code,
            Self::Invalid { .. } => "landing_invalid",
            Self::Markdown(error) => error.code(),
        }
    }
}

/// Original landing identity and normalized body evidence; body coordinates are
/// explicitly normalized and must be mapped with `original_location`.
#[derive(Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact YAML identity.
    pub source_sha256: Sha256Digest,
    /// Original source byte count.
    pub source_bytes: usize,
    /// Retained YAML syntax count.
    pub nodes: usize,
    /// Compiler-owned normalized Markdown evidence.
    pub normalized_markdown: markdown::Inspection,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Yaml(error) => error.fmt(f),
            Self::Invalid { reason, span } => {
                write!(f, "invalid landing: {reason}; original construct {span:?}")
            }
            Self::Markdown(error) => write!(f, "normalized landing body: {error}"),
        }
    }
}
impl std::error::Error for Error {}

impl<'s> ParsedLanding<'s> {
    /// Parse `format: gazette-landing/v1`, shared metadata and ordered sections.
    ///
    /// # Errors
    /// Rejects unsupported format, unknown section fields, invalid component or
    /// heading syntax, empty groups and exhausted source/projection bounds.
    pub fn parse(
        source: &'s str,
        limits: document::Limits,
        adapter: &dyn Markdown,
    ) -> Result<Self, Error> {
        Self::parse_with_profile(source, Profile::Publication, limits, adapter)
    }

    /// Parse ordered sections using the workspace's explicitly selected syntax profile.
    ///
    /// # Errors
    /// Uses the same schema and source bounds as [`Self::parse`].
    pub fn parse_with_profile(
        source: &'s str,
        profile: Profile,
        limits: document::Limits,
        adapter: &dyn Markdown,
    ) -> Result<Self, Error> {
        let yaml = yaml::ParsedYaml::parse(
            source,
            yaml::Limits {
                source_bytes: limits.source_bytes,
                nodes: limits.metadata.nodes,
                depth: limits.metadata.depth,
                decoded_bytes: limits.source_bytes,
            },
        )
        .map_err(Error::Yaml)?;
        let mut metadata = yaml.value().clone();
        let object = metadata.as_object_mut().ok_or(Error::Invalid {
            reason: "root must be a mapping",
            span: None,
        })?;
        if object.remove("format") != Some(Value::String("gazette-landing/v1".into())) {
            return Err(Error::Invalid {
                reason: "format must be gazette-landing/v1",
                span: yaml.span_at(&[key("format")]),
            });
        }
        let sections = object.remove("sections").ok_or(Error::Invalid {
            reason: "sections are required",
            span: None,
        })?;
        let sections: Vec<Section> =
            serde_json::from_value(sections).map_err(|_| Error::Invalid {
                reason: "sections do not match the structured landing schema",
                span: yaml.span_at(&[key("sections")]),
            })?;
        if sections.is_empty() {
            return Err(Error::Invalid {
                reason: "at least one section is required",
                span: yaml.span_at(&[key("sections")]),
            });
        }
        let metadata_bytes = object.keys().try_fold(0usize, |used, name| {
            let span = yaml.entry_span_at(&[key(name)])?;
            used.checked_add(span.end - span.start)
        });
        if metadata_bytes.is_none_or(|bytes| bytes > limits.metadata.source_bytes) {
            return Err(Error::Invalid {
                reason: "metadata byte ceiling exceeded",
                span: None,
            });
        }
        let mut lowered = Lowering {
            yaml: &yaml,
            text: String::new(),
            origins: Vec::new(),
            ceiling: limits.markdown.source_bytes,
            adapter,
            markdown_limits: limits.markdown,
            profile,
        };
        // Parent boundaries are longer than child boundaries or any authored run.
        let delimiter = max_colons(&sections) + max_depth(&sections) + 3;
        for (index, section) in sections.iter().enumerate() {
            lowered.section(
                section,
                &mut vec![key("sections"), PathSegment::Index(index)],
                delimiter,
            )?;
        }
        let Lowering { text, origins, .. } = lowered;
        let body = markdown::ParsedMarkdown::parse_owned(text, profile, limits.markdown, adapter)
            .map_err(Error::Markdown)?;
        Ok(Self {
            source: yaml,
            metadata,
            body,
            origins,
        })
    }

    /// Inspect the retained body without reparsing.
    ///
    /// # Errors
    /// Returns the retained Markdown inspection failure without a partial inspection.
    pub fn inspect(&self) -> Result<Inspection, Error> {
        Ok(Inspection {
            source_sha256: self.source_sha256(),
            source_bytes: self.source().len(),
            nodes: self.node_count(),
            normalized_markdown: self.body.inspect().map_err(Error::Markdown)?,
        })
    }
    /// Attach checked typesetting to the normalized body, retaining original identity.
    ///
    /// # Errors
    /// Rejects math bindings that fail the underlying Markdown identity or binding checks.
    pub fn with_math(
        mut self,
        identity: gazette_core::markdown::Identity,
        bindings: Vec<gazette_core::markdown::MathBinding>,
    ) -> Result<Self, Error> {
        self.body = self
            .body
            .with_math(identity, bindings)
            .map_err(Error::Markdown)?;
        Ok(self)
    }
    /// Exact original YAML source.
    pub fn source(&self) -> &'s str {
        self.source.source()
    }
    /// Exact original source identity, never the normalized body's digest.
    pub fn source_sha256(&self) -> Sha256Digest {
        self.source.source_sha256()
    }
    /// The same Markdown AST consumed by existing Gazette projection.
    pub fn body(&self) -> &markdown::ParsedMarkdown<'s> {
        &self.body
    }
    /// Retained YAML values/keys charged to corpus metadata bounds.
    pub fn node_count(&self) -> usize {
        self.source.node_count()
    }
    /// Deserialize metadata through the existing authored-content schema.
    ///
    /// # Errors
    /// Unknown or invalid metadata remains an error of the caller's schema.
    pub fn metadata<T: DeserializeOwned>(&self) -> Result<T, yaml::Error> {
        T::deserialize(&self.metadata).map_err(|error| yaml::Error {
            code: "yaml_schema",
            message: error.to_string(),
            span: None,
        })
    }
    /// A declared string value and its original containing construct.
    pub fn string_at(&self, path: &[PathSegment]) -> Option<(Span, &str)> {
        let mut value = self.source.value();
        for part in path {
            value = match part {
                PathSegment::Key(key) => value.get(key)?,
                PathSegment::Index(index) => value.get(*index)?,
            };
        }
        Some((self.source.span_at(path)?, value.as_str()?))
    }
    /// Locate an explicitly authored metadata or section field.
    pub fn span_at(&self, path: &[PathSegment]) -> Option<Span> {
        self.source.span_at(path)
    }
    /// Enumerate declared components in authored section order using the retained YAML locations.
    pub fn component_slots(&self) -> Vec<ComponentSlot> {
        let mut slots = Vec::new();
        self.section_components(
            &mut vec![key("sections")],
            self.source.value().get("sections"),
            &mut slots,
        );
        slots
    }

    fn section_components(
        &self,
        path: &mut Vec<PathSegment>,
        sections: Option<&Value>,
        slots: &mut Vec<ComponentSlot>,
    ) {
        let Some(sections) = sections.and_then(Value::as_array) else {
            return;
        };
        for (index, section) in sections.iter().enumerate() {
            path.push(PathSegment::Index(index));
            path.push(key("component"));
            let component = self.string_at(path).and_then(|(span, value)| {
                Some((
                    value.parse::<ComponentName>().ok()?,
                    self.component_name_location(span, value),
                ))
            });
            path.pop();
            if let Some((name, name_location)) = component {
                path.push(key("arguments"));
                let mut arguments = Vec::new();
                if let Some(values) = section.get("arguments").and_then(Value::as_object) {
                    for (name, value) in values {
                        path.push(key(name));
                        if let Some(span) = self.span_at(path) {
                            let key = self.source.entry_span_at(path).and_then(|entry| {
                                let raw = self.source().get(entry.start..span.start)?;
                                let length = scalar_prefix(raw, name, true)?;
                                Span::checked(self.source(), entry.start..entry.start + length)
                            });
                            let spelled = match value {
                                Value::String(text) => {
                                    Some(std::borrow::Cow::Borrowed(text.as_str()))
                                }
                                Value::Number(_) | Value::Bool(_) | Value::Null => {
                                    Some(std::borrow::Cow::Owned(value.to_string()))
                                }
                                _ => None,
                            };
                            let exact = spelled.and_then(|text| {
                                let length =
                                    scalar_prefix(span.slice(self.source())?, &text, false)?;
                                Span::checked(self.source(), span.start..span.start + length)
                            });
                            arguments.push(ArgumentSlot {
                                name: name.clone(),
                                key,
                                value: exact.map_or(
                                    Location {
                                        span,
                                        precision: Precision::Construct,
                                    },
                                    |span| Location {
                                        span,
                                        precision: Precision::Exact,
                                    },
                                ),
                            });
                        }
                        path.pop();
                    }
                }
                path.pop();
                slots.push(ComponentSlot {
                    name,
                    name_location,
                    arguments,
                });
            }
            path.push(key("sections"));
            self.section_components(path, section.get("sections"), slots);
            path.pop();
            path.pop();
        }
    }

    fn component_name_location(&self, span: Span, value: &str) -> Location {
        let exact = span.slice(self.source()).and_then(|raw| {
            let length = scalar_prefix(raw, value, false)?;
            Span::checked(self.source(), span.start..span.start + length)
        });
        exact.map_or(
            Location {
                span,
                precision: Precision::Construct,
            },
            |span| Location {
                span,
                precision: Precision::Exact,
            },
        )
    }

    /// Map normalized syntax to its containing YAML value. This deliberately
    /// reports construct precision, including escaped and multiline text.
    pub fn original_location(&self, location: Location) -> Option<Location> {
        let mut spans = self
            .origins
            .iter()
            .filter(|origin| {
                origin.normalized.start < location.span.end
                    && location.span.start < origin.normalized.end
                    || location.span.start == location.span.end
                        && origin.normalized.start <= location.span.start
                        && location.span.start < origin.normalized.end
            })
            .map(|origin| origin.original);
        let first = spans.next()?;
        let span = spans.fold(first, |left, right| Span {
            start: left.start.min(right.start),
            end: left.end.max(right.end),
        });
        Some(Location {
            span,
            precision: Precision::Construct,
        })
    }
}

// YAML locations may include whitespace up to the next token. Only direct
// scalar spellings whose bytes and lexical boundary are proved become edits.
fn scalar_prefix(raw: &str, decoded: &str, key: bool) -> Option<usize> {
    if decoded.contains(['\r', '\n']) {
        return None;
    }
    let quote = raw
        .as_bytes()
        .first()
        .copied()
        .filter(|b| matches!(b, b'\'' | b'"'));
    let start = usize::from(quote.is_some());
    let suffix = raw.get(start..)?.strip_prefix(decoded)?;
    let suffix = match quote {
        Some(quote) => suffix.strip_prefix(char::from(quote))?,
        None => suffix,
    };
    if key {
        if !suffix.trim_start().starts_with(':') {
            return None;
        }
    } else if !suffix.is_empty()
        && !suffix.starts_with(|c: char| c.is_ascii_whitespace() || matches!(c, ',' | '}' | ']'))
    {
        return None;
    }
    Some(start * 2 + decoded.len())
}

fn key(value: &str) -> PathSegment {
    PathSegment::Key(value.into())
}
fn max_depth(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|s| 1 + max_depth(&s.sections))
        .max()
        .unwrap_or(0)
}
fn max_colons(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|s| {
            let local = s
                .body
                .as_deref()
                .unwrap_or("")
                .lines()
                .map(|line| line.trim_start().bytes().take_while(|b| *b == b':').count())
                .max()
                .unwrap_or(0);
            local.max(max_colons(&s.sections))
        })
        .max()
        .unwrap_or(0)
}

struct Lowering<'a, 's> {
    yaml: &'a yaml::ParsedYaml<'s>,
    text: String,
    origins: Vec<BodyOrigin>,
    ceiling: usize,
    adapter: &'a dyn Markdown,
    markdown_limits: markdown::Limits,
    profile: Profile,
}
impl Lowering<'_, '_> {
    fn append(&mut self, text: &str, path: &[PathSegment]) -> Result<(), Error> {
        let end = self
            .text
            .len()
            .checked_add(text.len())
            .filter(|size| *size <= self.ceiling)
            .ok_or(Error::Invalid {
                reason: "normalized body byte ceiling exceeded",
                span: self.yaml.span_at(path),
            })?;
        let original = self.yaml.span_at(path).ok_or(Error::Invalid {
            reason: "authored construct has no source location",
            span: None,
        })?;
        self.origins.push(BodyOrigin {
            normalized: Span {
                start: self.text.len(),
                end,
            },
            original,
        });
        self.text.push_str(text);
        Ok(())
    }
    fn check_body_boundary(&self, body: &str, path: &[PathSegment]) -> Result<(), Error> {
        // This transient syntax probe detects an open fenced block or component
        // swallowing the next section. It does not resolve references or allocate
        // page anchors; those still use the one complete normalized body.
        const PROBE: &str = "\n\n# Gazette section boundary\n";
        let mut limits = self.markdown_limits;
        limits.source_bytes = limits.source_bytes.saturating_add(PROBE.len());
        limits.nodes = limits.nodes.saturating_add(2);
        let source = format!("{body}{PROBE}");
        let parsed = self
            .adapter
            .parse(&source, self.profile, limits)
            .map_err(|error| Error::Markdown(error.into()))?;
        let start = body.len() + 2;
        let heading_survives = parsed
            .syntax()
            .headings
            .iter()
            .any(|heading| heading.emitted && heading.location.span.start == start);
        let swallowed_by_component = parsed.syntax().directives.iter().any(|directive| {
            directive.location.span.start < start && directive.location.span.end > start
        });
        if !heading_survives || swallowed_by_component {
            return Err(Error::Invalid {
                reason: "body leaves a block open across a section boundary",
                span: self.yaml.span_at(path),
            });
        }
        Ok(())
    }
    fn section(
        &mut self,
        section: &Section,
        path: &mut Vec<PathSegment>,
        delimiter: usize,
    ) -> Result<(), Error> {
        let failure = |reason| Error::Invalid {
            reason,
            span: self.yaml.span_at(path),
        };
        if section.component.is_none()
            && section.heading.is_none()
            && section
                .body
                .as_ref()
                .is_none_or(|body| body.trim().is_empty())
            && section.sections.is_empty()
        {
            return Err(failure(UNNAMED_SECTION_REQUIREMENT));
        }
        if section.component.is_none() && !section.arguments.is_empty() {
            return Err(failure("arguments require a component"));
        }
        let fence = if section.component.is_some() {
            if delimiter
                .checked_mul(2)
                .is_none_or(|size| size > self.ceiling.saturating_sub(self.text.len()))
            {
                return Err(failure("component boundary byte ceiling exceeded"));
            }
            ":".repeat(delimiter)
        } else {
            String::new()
        };
        if let Some(component) = &section.component {
            let arguments = component_arguments(&section.arguments).ok_or_else(|| {
                failure("component arguments must be representable by native TOML data")
            })?;
            self.append(&format!("{fence}{component} {arguments}\n"), path)?;
        }
        if let Some(heading) = &section.heading {
            if !(1..=6).contains(&heading.level)
                || heading.text.trim().is_empty()
                || heading.text.contains(['\n', '\r'])
            {
                return Err(failure("heading requires level 1–6 and single-line text"));
            }
            let text: String = heading
                .text
                .chars()
                .flat_map(|c| {
                    if c.is_ascii_punctuation() {
                        vec!['\\', c]
                    } else {
                        vec![c]
                    }
                })
                .collect();
            path.push(key("heading"));
            self.append(
                &format!("{} {text}\n\n", "#".repeat(usize::from(heading.level))),
                path,
            )?;
            path.pop();
        }
        if let Some(body) = &section.body {
            path.push(key("body"));
            self.check_body_boundary(body, path)?;
            self.append(&format!("{body}\n\n"), path)?;
            path.pop();
        }
        for (index, child) in section.sections.iter().enumerate() {
            path.push(key("sections"));
            path.push(PathSegment::Index(index));
            self.section(child, path, delimiter - 1)?;
            path.pop();
            path.pop();
        }
        if section.component.is_some() {
            self.append(&format!("{fence}\n\n"), path)?;
        }
        Ok(())
    }
}

// TOML component arguments are a closed data grammar. JSON-escaped strings are
// valid TOML basic strings; null and integers outside TOML's range are refused.
// The native directive scanner forbids literal colons throughout its header,
// including quoted data. A TOML Unicode escape preserves each decoded colon.
fn component_arguments(arguments: &serde_json::Map<String, Value>) -> Option<String> {
    fn string(value: &str) -> Option<String> {
        serde_json::to_string(value)
            .ok()
            .map(|value| value.replace(':', "\\u003a"))
    }
    fn convert(value: &Value) -> Option<String> {
        match value {
            Value::Null => None,
            Value::Bool(_) => serde_json::to_string(value).ok(),
            Value::String(value) => string(value),
            Value::Number(value) if value.is_i64() || value.is_u64() => {
                Some(value.as_i64()?.to_string())
            }
            Value::Number(value) => Some(value.to_string()),
            Value::Array(values) => Some(format!(
                "[{}]",
                values
                    .iter()
                    .map(convert)
                    .collect::<Option<Vec<_>>>()?
                    .join(", ")
            )),
            Value::Object(values) => component_arguments(values),
        }
    }
    let fields = arguments
        .iter()
        .map(|(key, value)| Some(format!("{} = {}", string(key)?, convert(value)?)))
        .collect::<Option<Vec<_>>>()?;
    Some(format!("{{{}}}", fields.join(", ")))
}
