//! Theme-owned definitions and source-bound arguments, separate from formatting.

use std::collections::BTreeMap;
use std::ops::Range;

use gazette_core::{
    markdown::{ComponentBinding, ComponentName, Directive},
    source::EntryPath,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{markdown, toml};

/// A static block's captured template and data-only JSON Schema argument contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// Explicit authored meaning; omitted components retain ordinary body semantics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<super::ComponentSemantic>,
    /// Playback or decorative payload, excluded from reading text and Markdown.
    /// Meaningful descriptions and captions must remain in ordinary sibling content.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub presentation_only: bool,
    /// Exact template entry relative to this Theme's selected template directory.
    pub template: EntryPath,
    /// Draft 2020-12 schema; local references are supported, external retrieval is forbidden.
    /// An omitted contract permits only an empty argument object.
    #[serde(default = "empty_arguments")]
    pub arguments: Value,
    /// Optional explicit interaction intent. The static template remains mandatory.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "super::interaction::selected"
    )]
    #[schemars(with = "super::ComponentInteraction")]
    pub interaction: Option<super::ComponentInteraction>,
}

fn empty_arguments() -> Value {
    serde_json::json!({"type": "object", "additionalProperties": false})
}

struct Compiled {
    definition: Component,
    validator: jsonschema::Validator,
}

/// Immutable selected component contracts; no renderer, source reads or provider selection.
#[derive(Default)]
pub struct Components {
    entries: BTreeMap<ComponentName, Compiled>,
}

/// Source-local component-name slot recovered by the same lexer used for admission.
///
/// Offsets are relative to [`Directive::info`]. They are not full-document
/// coordinates and carry no Theme, renderer or publication authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentHeader<'a> {
    /// Exact authored spelling, including an incomplete or invalid name.
    pub name: &'a str,
    /// Exact name bytes within the retained directive header.
    pub name_range: Range<usize>,
    /// Unnormalized argument suffix after surrounding whitespace is removed.
    pub arguments: &'a str,
    /// Exact argument bytes within the retained directive header.
    pub arguments_range: Range<usize>,
}

impl std::fmt::Debug for Components {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Components")
            .field("names", &self.entries.keys())
            .finish()
    }
}

impl Components {
    /// Inspect one retained directive header without admitting its name or arguments.
    /// Completion and rendering therefore share one component-name boundary.
    pub fn header<'a>(occurrence: &'a Directive) -> ComponentHeader<'a> {
        let info = occurrence.info.as_str();
        let leading = info.len() - info.trim_start().len();
        let remaining = &info[leading..];
        let name_bytes = remaining
            .find(char::is_whitespace)
            .unwrap_or(remaining.len());
        let name = &remaining[..name_bytes];
        let suffix = &remaining[name_bytes..];
        let argument_start = leading + name_bytes + (suffix.len() - suffix.trim_start().len());
        let arguments = suffix.trim();
        let argument_end = argument_start + arguments.len();
        ComponentHeader {
            name,
            name_range: leading..leading + name_bytes,
            arguments,
            arguments_range: argument_start..argument_end,
        }
    }

    /// Project the selected component's exact argument schema over one retained
    /// TOML inline table. The Theme contract remains the sole schema root, so
    /// local JSON Schema references retain their native meaning.
    ///
    /// # Errors
    ///
    /// Returns the shared authoring failure when the retained argument source,
    /// Theme schema, projected candidates or response exceed their exact bounds.
    pub fn argument_help(
        &self,
        name: &ComponentName,
        arguments: &str,
        offset: usize,
        limits: toml::Limits,
        maximum_bytes: usize,
    ) -> Result<Option<crate::workspace::AuthoringHelp>, crate::workspace::AuthoringError> {
        const PREFIX: &str = "arguments = ";
        if !arguments.starts_with('{')
            || offset > arguments.len()
            || !arguments.is_char_boundary(offset)
        {
            return Ok(None);
        }
        let Some(definition) = self.definition(name) else {
            return Ok(None);
        };
        let source = format!("{PREFIX}{arguments}");
        let authoring = crate::workspace::authoring::TomlAuthoring::parse(&source, limits)?;
        let prefix = [crate::workspace::AuthoringPathSegment::Field(
            "arguments".into(),
        )];
        let help = authoring.help_beneath(
            &definition.arguments,
            &source,
            PREFIX.len() + offset,
            maximum_bytes,
            &prefix,
        )?;
        let parent = gazette_core::source::Span::checked(&source, PREFIX.len()..source.len())
            .ok_or(crate::workspace::AuthoringError::Invalid)?;
        Ok(Some(help.relative_to(&source, parent)?))
    }

    /// Return data-only candidates for a YAML argument key or closed scalar value.
    ///
    /// # Errors
    /// Refuses an unknown component, unavailable schema path or exhausted schema bound.
    pub fn yaml_argument_candidates<'a>(
        &self,
        name: &ComponentName,
        field: &str,
        rename: bool,
        siblings: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<crate::workspace::CompletionCandidate>, crate::workspace::AuthoringError> {
        let definition = self
            .definition(name)
            .ok_or(crate::workspace::AuthoringError::Invalid)?;
        crate::workspace::authoring::scalar_argument_candidates(
            &definition.arguments,
            field,
            rename,
            siblings,
        )
    }

    pub(crate) fn compile(
        definitions: &BTreeMap<ComponentName, Component>,
    ) -> Result<Self, InvalidComponents> {
        let mut entries = BTreeMap::new();
        for (name, definition) in definitions {
            let failure = || InvalidComponents;
            if definition.semantic.is_some() && definition.presentation_only
                || matches!(&definition.semantic, Some(super::ComponentSemantic::FaqItem { question_argument }) if question_argument.trim().is_empty())
            {
                return Err(failure());
            }
            if definition.arguments.get("$schema").is_some_and(|schema| {
                schema.as_str() != Some("https://json-schema.org/draft/2020-12/schema")
            }) {
                return Err(failure());
            }
            let validator = jsonschema::draft202012::options()
                .with_retriever(crate::schema::DenyExternal)
                .build(&definition.arguments)
                .map_err(|_| failure())?;
            entries.insert(
                name.clone(),
                Compiled {
                    definition: definition.clone(),
                    validator,
                },
            );
        }
        Ok(Self { entries })
    }

    /// Exact declared templates, for captured-input validation before rendering any page.
    pub fn definitions(&self) -> impl Iterator<Item = (&ComponentName, &Component)> {
        self.entries
            .iter()
            .map(|(name, entry)| (name, &entry.definition))
    }

    /// Resolve one exact selected definition without searching other Themes or sources.
    pub fn definition(&self, name: &ComponentName) -> Option<&Component> {
        self.entries.get(name).map(|entry| &entry.definition)
    }

    /// Validate every source occurrence, including unused footnotes, then bind emitted uses.
    /// Original header bytes are parsed as a data-only TOML inline table. No normalized
    /// adapter-normalized info, authored-source rewrite or template-produced meaning is used.
    ///
    /// # Errors
    /// Rejects unknown definitions, malformed or schema-invalid arguments and bounds.
    pub fn bind(
        &self,
        directives: &[Directive],
        limits: toml::Limits,
    ) -> Result<Vec<ComponentBinding>, markdown::Error> {
        let mut bindings = Vec::new();
        for occurrence in directives {
            let failure = |reason| {
                markdown::Error::Adapter(gazette_core::markdown::Error::Unsupported {
                    location: occurrence.header,
                    reason,
                })
            };
            let header = Self::header(occurrence);
            let name: ComponentName = header
                .name
                .parse()
                .map_err(|_| failure("invalid component name"))?;
            let definition = self
                .entries
                .get(&name)
                .ok_or_else(|| failure("component is not defined by the selected Theme"))?;
            let arguments = header.arguments;
            if !(arguments.is_empty() || arguments.starts_with('{') && arguments.ends_with('}')) {
                return Err(failure("component arguments must be one TOML inline table"));
            }
            let source = format!(
                "arguments = {}",
                if arguments.is_empty() {
                    "{}"
                } else {
                    arguments
                }
            );
            let parsed = toml::ParsedToml::parse(&source, limits)
                .map_err(|_| failure("invalid or oversized component argument table"))?;
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Arguments {
                arguments: BTreeMap<String, Value>,
            }
            let arguments = parsed
                .deserialize::<Arguments>()
                .map_err(|_| failure("component arguments must be data values in one table"))?
                .arguments;
            let value = Value::Object(
                arguments
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect(),
            );
            if !definition.validator.is_valid(&value) {
                return Err(failure(
                    "component arguments do not satisfy the selected definition",
                ));
            }
            if let Some(super::ComponentSemantic::FaqItem { question_argument }) =
                &definition.definition.semantic
                && arguments
                    .get(question_argument)
                    .and_then(Value::as_str)
                    .is_none_or(|question| question.trim().is_empty())
            {
                return Err(failure(
                    "FAQ question argument must contain nonempty plain text",
                ));
            }
            if occurrence.emitted {
                let reading_prefix = match &definition.definition.semantic {
                    Some(super::ComponentSemantic::FaqItem { question_argument }) => arguments
                        .get(question_argument)
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    _ => None,
                };
                bindings.push(ComponentBinding {
                    container_contents: None,
                    reading_prefix,
                    presentation_only: definition.definition.presentation_only,
                    occurrence: occurrence.clone(),
                    name,
                    arguments,
                    context: None,
                });
            }
        }
        Ok(bindings)
    }
}

/// A Theme component catalog contains an invalid local argument contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InvalidComponents;

impl InvalidComponents {
    pub(crate) const fn reason(self) -> &'static str {
        "component arguments require a valid local Draft 2020-12 schema and compatible semantic declaration"
    }
}
