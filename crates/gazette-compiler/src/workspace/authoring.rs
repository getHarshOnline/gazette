//! Tolerant workspace structure and generated schema facts for authoring.
//!
//! These facts assist an editor over the exact current bytes. They never admit a
//! workspace: [`super::WorkspaceInspection`] remains the sole strict parser and
//! resolver used by publication compilation.

mod model;
mod query;
mod schema;
mod syntax;

pub use model::{
    AuthoringError, AuthoringField, AuthoringFieldPresence, AuthoringHelp, AuthoringLiteral,
    AuthoringLiteralCandidate, AuthoringNode, AuthoringPathSegment, AuthoringStructureField,
    AuthoringValueKind, CompletionCandidate, CompletionKind, SchemaHover, StringOccurrence,
};

use std::sync::OnceLock;

use crate::toml;
use serde_json::Value;
use syntax::Syntax;

/// Shared tolerant TOML syntax facts. Semantic owners supply their own generated schema.
#[derive(Clone, Debug)]
pub(crate) struct TomlAuthoring {
    syntax: Syntax,
}

impl TomlAuthoring {
    pub(crate) fn parse(source: &str, limits: toml::Limits) -> Result<Self, AuthoringError> {
        if source.len() > limits.source_bytes {
            return Err(AuthoringError::Limit("source bytes"));
        }
        Ok(Self {
            syntax: Syntax::parse(source, limits)?,
        })
    }

    pub(crate) fn help(
        &self,
        schema: &Value,
        source: &str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<AuthoringHelp, AuthoringError> {
        query::help(schema, &self.syntax, source, offset, maximum_bytes)
    }

    pub(crate) fn help_beneath(
        &self,
        schema: &Value,
        source: &str,
        offset: usize,
        maximum_bytes: usize,
        path_prefix: &[AuthoringPathSegment],
    ) -> Result<AuthoringHelp, AuthoringError> {
        query::help_beneath(
            schema,
            &self.syntax,
            source,
            offset,
            maximum_bytes,
            path_prefix,
        )
    }

    pub(crate) fn structure(
        &self,
        schema: &Value,
        source: &str,
        maximum: usize,
    ) -> Result<Vec<AuthoringStructureField>, AuthoringError> {
        let mut result = Vec::new();
        for (fact, value) in self.syntax.fields() {
            let help = match query::help(schema, &self.syntax, source, fact.span.start, maximum) {
                Ok(help) => Some(help),
                Err(AuthoringError::Invalid) => None,
                Err(error) => return Err(error),
            };
            result.push(AuthoringStructureField {
                path: fact.path.clone(),
                location: gazette_core::source::Location {
                    span: fact.span,
                    precision: gazette_core::source::Precision::Exact,
                },
                name: fact
                    .span
                    .slice(source)
                    .ok_or(AuthoringError::Invalid)?
                    .to_owned(),
                value: match value {
                    Some(v) => Some(
                        v.span
                            .slice(source)
                            .ok_or(AuthoringError::Invalid)?
                            .to_owned(),
                    ),
                    None => None,
                },
                schema: help
                    .and_then(|help| help.hover)
                    .and_then(|hover| hover.field),
            });
        }
        let mut writer = gazette_core::render::LimitedWriter::new(std::io::sink(), maximum);
        serde_json::to_writer(&mut writer, &result)
            .map_err(|_| AuthoringError::Limit("structure bytes"))?;
        if writer.exceeded() {
            return Err(AuthoringError::Limit("structure bytes"));
        }
        Ok(result)
    }

    pub(crate) fn string_occurrence(
        &self,
        source: &str,
        offset: usize,
    ) -> Result<Option<StringOccurrence>, AuthoringError> {
        query::string_occurrence(&self.syntax, source, offset)
    }

    pub(crate) fn strings<'a>(
        &'a self,
        source: &'a str,
    ) -> impl Iterator<Item = Result<StringOccurrence, AuthoringError>> + 'a {
        self.syntax
            .values()
            .filter_map(move |fact| query::string_value(fact, source).transpose())
    }
}

/// Recoverable structure for one exact workspace buffer.
#[derive(Clone, Debug)]
pub struct WorkspaceAuthoring {
    inner: TomlAuthoring,
}

impl WorkspaceAuthoring {
    /// Recover source-local structure without asserting strict TOML or workspace validity.
    ///
    /// # Errors
    /// Refuses source, fact, or path depth beyond the strict workspace ceilings.
    pub fn parse(source: &str, limits: toml::Limits) -> Result<Self, AuthoringError> {
        Ok(Self {
            inner: TomlAuthoring::parse(source, limits)?,
        })
    }

    /// Return exact current completion and hover facts at a UTF-8 byte position.
    ///
    /// # Errors
    /// Refuses invalid coordinates, incomplete recovered indexes, unknown schema
    /// paths, and a complete result exceeding `maximum_bytes`.
    pub fn help(
        &self,
        source: &str,
        offset: usize,
        maximum_bytes: usize,
    ) -> Result<AuthoringHelp, AuthoringError> {
        self.inner
            .help(workspace_schema(), source, offset, maximum_bytes)
    }
    /// Project current source keys and native schema descriptions without admitting the workspace.
    ///
    /// # Errors
    /// Refuses invalid source spans and exhausted assistance or complete-output
    /// byte bounds.
    pub fn structure(
        &self,
        source: &str,
        maximum: usize,
    ) -> Result<Vec<AuthoringStructureField>, AuthoringError> {
        self.inner.structure(workspace_schema(), source, maximum)
    }
}

pub(crate) fn scalar_argument_candidates(
    schema: &Value,
    field: &str,
    rename: bool,
    sibling_names: impl IntoIterator<Item = impl AsRef<str>>,
) -> Result<Vec<CompletionCandidate>, AuthoringError> {
    if !rename {
        let node = schema::inspect_node(schema, &[AuthoringPathSegment::Field(field.to_owned())])?;
        return Ok(query::literal_candidates(&node));
    }
    let used = sibling_names
        .into_iter()
        .map(|name| name.as_ref().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    Ok(schema::inspect_node(schema, &[])?
        .fields
        .into_iter()
        .filter(|candidate| !used.contains(&candidate.name))
        .map(|field| CompletionCandidate {
            replacement: serde_json::to_string(&field.name).expect("string"),
            label: field.name,
            description: field.description,
            presence: Some(field.presence),
            value_kinds: field.value_kinds,
        })
        .collect())
}

fn workspace_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::to_value(schemars::schema_for!(super::Declaration))
            .expect("generated workspace schema serializes")
    })
}
