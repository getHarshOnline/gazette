//! Cursor-local completion and hover over separately owned syntax and schema facts.

use gazette_core::source::{Location, Precision, Span};
use serde_json::Value;

use super::{
    model::{
        AuthoringError, AuthoringField, AuthoringHelp, AuthoringLiteral, AuthoringLiteralCandidate,
        AuthoringNode, AuthoringPathSegment, AuthoringValueKind, Completion, CompletionCandidate,
        CompletionKind, SchemaHover, StringOccurrence,
    },
    schema::inspect_node,
    syntax::{Fact, FactKind, Syntax, SyntaxPosition},
};

pub(super) fn string_occurrence(
    syntax: &Syntax,
    source: &str,
    offset: usize,
) -> Result<Option<StringOccurrence>, AuthoringError> {
    if !source.is_char_boundary(offset) {
        return Err(AuthoringError::Invalid);
    }
    let Some(fact) = syntax.position(offset).fact else {
        return Ok(None);
    };
    if fact.kind != FactKind::Value {
        return Ok(None);
    }
    string_value(fact, source)
}

pub(super) fn string_value(
    fact: &Fact,
    source: &str,
) -> Result<Option<StringOccurrence>, AuthoringError> {
    let raw = fact.span.slice(source).ok_or(AuthoringError::Invalid)?;
    let value = raw
        .parse::<toml_edit::Value>()
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned));
    Ok(value.map(|value| StringOccurrence {
        location: Location {
            span: fact.span,
            precision: Precision::Exact,
        },
        path: fact.path.clone(),
        value,
    }))
}

pub(super) fn help(
    schema: &Value,
    syntax: &Syntax,
    source: &str,
    offset: usize,
    maximum_bytes: usize,
) -> Result<AuthoringHelp, AuthoringError> {
    help_beneath(schema, syntax, source, offset, maximum_bytes, &[])
}

pub(super) fn help_beneath(
    schema: &Value,
    syntax: &Syntax,
    source: &str,
    offset: usize,
    maximum_bytes: usize,
    path_prefix: &[AuthoringPathSegment],
) -> Result<AuthoringHelp, AuthoringError> {
    if !source.is_char_boundary(offset) {
        return Err(AuthoringError::Invalid);
    }
    let position = syntax.position(offset);
    let hover = position
        .fact
        .and_then(|fact| hover_for(schema, source, fact, path_prefix).transpose())
        .transpose()?;
    let completion = completion_for(schema, source, offset, &position, path_prefix)?;
    let (completion_location, completion_preimage, completion_kind, candidates) = completion
        .map_or((None, None, None, Vec::new()), |value| {
            (
                Some(value.location),
                Some(value.preimage),
                Some(value.kind),
                value.candidates,
            )
        });
    let help = AuthoringHelp {
        completion_location,
        completion_preimage,
        completion_kind,
        candidates,
        hover,
    };
    let bytes = serde_json::to_vec(&help).map_err(|_| AuthoringError::Invalid)?;
    if bytes.len() > maximum_bytes {
        return Err(AuthoringError::Limit("result bytes"));
    }
    Ok(help)
}

fn completion_for(
    schema: &Value,
    source: &str,
    offset: usize,
    position: &SyntaxPosition<'_>,
    path_prefix: &[AuthoringPathSegment],
) -> Result<Option<Completion>, AuthoringError> {
    if let Some(fact) = position.fact
        && matches!(fact.kind, FactKind::Value | FactKind::ValueSlot)
    {
        let node = inspect_node(schema, beneath(&fact.path, path_prefix)?)?;
        let candidates = literal_candidates(&node);
        if candidates.is_empty() {
            return Ok(None);
        }
        let span = if fact.kind == FactKind::Value {
            fact.span
        } else {
            empty_value_span(source, fact.span, offset).ok_or(AuthoringError::Invalid)?
        };
        return Ok(Some(Completion {
            location: Location {
                span,
                precision: Precision::Exact,
            },
            preimage: span
                .slice(source)
                .ok_or(AuthoringError::Invalid)?
                .to_owned(),
            kind: CompletionKind::Literal,
            candidates,
        }));
    }

    let (object_path, prefix, span) = match position.fact {
        Some(Fact {
            kind: FactKind::PartialKey | FactKind::Key,
            span,
            path,
        }) => (
            path.get(..path.len().saturating_sub(1)).unwrap_or_default(),
            span.slice(source).ok_or(AuthoringError::Invalid)?,
            *span,
        ),
        None if blank_prefix(source, offset) => (
            position.object_path,
            "",
            Span::checked(source, offset..offset).ok_or(AuthoringError::Invalid)?,
        ),
        _ => return Ok(None),
    };
    let node = inspect_node(schema, beneath(object_path, path_prefix)?)?;
    let candidates = node
        .fields
        .into_iter()
        .filter(|field| !position.existing_fields.contains(&field.name))
        .filter(|field| field.name.starts_with(prefix))
        .map(|field| CompletionCandidate {
            label: field.name.clone(),
            replacement: field.name,
            description: field.description,
            presence: Some(field.presence),
            value_kinds: field.value_kinds,
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Ok(None);
    }
    Ok(Some(Completion {
        location: Location {
            span,
            precision: Precision::Exact,
        },
        preimage: span
            .slice(source)
            .ok_or(AuthoringError::Invalid)?
            .to_owned(),
        kind: CompletionKind::Field,
        candidates,
    }))
}

fn hover_for(
    schema: &Value,
    source: &str,
    fact: &Fact,
    path_prefix: &[AuthoringPathSegment],
) -> Result<Option<SchemaHover>, AuthoringError> {
    if !matches!(
        fact.kind,
        FactKind::Key
            | FactKind::HeaderKey
            | FactKind::Value
            | FactKind::Table
            | FactKind::ArrayTable
    ) {
        return Ok(None);
    }
    fact.span.slice(source).ok_or(AuthoringError::Invalid)?;
    let path = beneath(&fact.path, path_prefix)?;
    let field = if matches!(fact.kind, FactKind::Key | FactKind::HeaderKey) {
        let field = field_at(schema, path)?;
        if field.is_none() {
            return Ok(None);
        }
        field
    } else {
        None
    };
    let node = inspect_node(schema, path)?;
    Ok(Some(SchemaHover {
        location: Location {
            span: fact.span,
            precision: if matches!(fact.kind, FactKind::Table | FactKind::ArrayTable) {
                Precision::Construct
            } else {
                Precision::Exact
            },
        },
        path: path.to_vec(),
        field,
        node,
    }))
}

fn beneath<'a>(
    path: &'a [AuthoringPathSegment],
    prefix: &[AuthoringPathSegment],
) -> Result<&'a [AuthoringPathSegment], AuthoringError> {
    path.strip_prefix(prefix).ok_or(AuthoringError::Invalid)
}

fn field_at(
    schema: &Value,
    path: &[AuthoringPathSegment],
) -> Result<Option<AuthoringField>, AuthoringError> {
    let Some(AuthoringPathSegment::Field(name)) = path.last() else {
        return Ok(None);
    };
    let parent = inspect_node(schema, &path[..path.len() - 1])?;
    Ok(parent.fields.into_iter().find(|field| field.name == *name))
}

pub(super) fn literal_candidates(node: &AuthoringNode) -> Vec<CompletionCandidate> {
    let mut values = node.allowed_values.clone();
    if values.is_empty() && node.value_kinds.contains(&AuthoringValueKind::Boolean) {
        values.extend([false, true].map(|value| AuthoringLiteralCandidate {
            value: AuthoringLiteral::Boolean(value),
            description: None,
        }));
    }
    values
        .into_iter()
        .map(|candidate| {
            let (label, replacement) = match &candidate.value {
                AuthoringLiteral::String(value) => (
                    value.clone(),
                    serde_json::to_string(value).expect("a Rust string always serializes"),
                ),
                AuthoringLiteral::Integer(value) => (value.to_string(), value.to_string()),
                AuthoringLiteral::Boolean(value) => (value.to_string(), value.to_string()),
            };
            CompletionCandidate {
                label,
                replacement,
                description: candidate.description,
                presence: None,
                value_kinds: node.value_kinds.clone(),
            }
        })
        .collect()
}

fn empty_value_span(source: &str, span: Span, offset: usize) -> Option<Span> {
    let start = span.start.min(offset);
    let end = span.end.min(
        source[start..]
            .find(['\r', '\n', '#'])
            .map_or(offset, |relative| start + relative),
    );
    let raw = source.get(start..end)?;
    let leading = raw.len() - raw.trim_start_matches([' ', '\t']).len();
    let trimmed = raw.trim_matches([' ', '\t']);
    let value_start = start + leading;
    Span::checked(source, value_start..value_start + trimmed.len())
}

fn blank_prefix(source: &str, offset: usize) -> bool {
    let start = source[..offset].rfind('\n').map_or(0, |index| index + 1);
    source[start..offset].trim().is_empty()
}
