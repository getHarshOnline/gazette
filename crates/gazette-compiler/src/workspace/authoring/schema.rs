//! Bounded schema traversal over the actual workspace declaration type.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::model::{
    AuthoringError, AuthoringField, AuthoringFieldPresence, AuthoringLiteral,
    AuthoringLiteralCandidate, AuthoringNode, AuthoringPathSegment, AuthoringValueKind,
};

pub(super) fn inspect_node(
    root: &Value,
    path: &[AuthoringPathSegment],
) -> Result<AuthoringNode, AuthoringError> {
    let mut nodes = vec![NodeRef(root)];
    for segment in path {
        let mut expanded = Vec::new();
        for node in &nodes {
            expand(root, *node, 0, &mut BTreeSet::new(), &mut expanded)?;
        }
        nodes = expanded
            .into_iter()
            .filter_map(|node| match segment {
                AuthoringPathSegment::Field(field) => node
                    .0
                    .get("properties")
                    .and_then(|value| value.get(field))
                    .or_else(|| {
                        node.0
                            .get("additionalProperties")
                            .filter(|schema| schema.is_object())
                    })
                    .map(NodeRef),
                AuthoringPathSegment::ArrayItem => node.0.get("items").map(NodeRef),
            })
            .collect();
        if nodes.is_empty() {
            return Err(AuthoringError::Invalid);
        }
    }
    project_node(root, &nodes)
}

#[derive(Clone, Copy)]
struct NodeRef<'a>(&'a Value);

fn expand<'a>(
    root: &'a Value,
    node: NodeRef<'a>,
    depth: usize,
    visited: &mut BTreeSet<String>,
    output: &mut Vec<NodeRef<'a>>,
) -> Result<(), AuthoringError> {
    if depth > 64 {
        return Err(AuthoringError::Limit("schema reference depth"));
    }
    if let Some(reference) = node.0.get("$ref").and_then(Value::as_str) {
        if !visited.insert(reference.to_owned()) {
            return Ok(());
        }
        let pointer = reference.strip_prefix('#').ok_or(AuthoringError::Invalid)?;
        let target = root.pointer(pointer).ok_or(AuthoringError::Invalid)?;
        expand(root, NodeRef(target), depth + 1, visited, output)?;
        visited.remove(reference);
    }
    for keyword in ["allOf", "anyOf", "oneOf"] {
        if let Some(variants) = node.0.get(keyword).and_then(Value::as_array) {
            for variant in variants {
                expand(root, NodeRef(variant), depth + 1, visited, output)?;
            }
        }
    }
    output.push(node);
    Ok(())
}

fn project_node(root: &Value, nodes: &[NodeRef<'_>]) -> Result<AuthoringNode, AuthoringError> {
    let mut expanded = Vec::new();
    for node in nodes {
        expand(root, *node, 0, &mut BTreeSet::new(), &mut expanded)?;
    }
    let description = join(
        expanded
            .iter()
            .filter_map(|node| {
                node.0
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect(),
    );
    Ok(AuthoringNode {
        description,
        value_kinds: value_kinds(&expanded),
        allowed_values: allowed_values(&expanded),
        fields: project_fields(root, &expanded)?,
    })
}

fn project_fields(
    root: &Value,
    nodes: &[NodeRef<'_>],
) -> Result<Vec<AuthoringField>, AuthoringError> {
    #[derive(Default)]
    struct Accumulator {
        descriptions: BTreeSet<String>,
        kinds: BTreeSet<AuthoringValueKind>,
        values: BTreeMap<AuthoringLiteral, BTreeSet<String>>,
        required: usize,
    }

    let objects = nodes
        .iter()
        .filter(|node| {
            node.0
                .get("properties")
                .and_then(Value::as_object)
                .is_some()
        })
        .copied()
        .collect::<Vec<_>>();
    let variants = objects.len();
    let mut fields = BTreeMap::<String, Accumulator>::new();
    for node in objects {
        let properties = node
            .0
            .get("properties")
            .and_then(Value::as_object)
            .ok_or(AuthoringError::Invalid)?;
        let required = node
            .0
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>();
        for (name, value) in properties {
            let entry = fields.entry(name.clone()).or_default();
            entry.required += usize::from(required.contains(name.as_str()));
            if let Some(description) = value.get("description").and_then(Value::as_str) {
                entry.descriptions.insert(description.to_owned());
            }
            let mut expanded = Vec::new();
            expand(root, NodeRef(value), 0, &mut BTreeSet::new(), &mut expanded)?;
            entry.kinds.extend(value_kinds(&expanded));
            for value in allowed_values(&expanded) {
                let descriptions = entry.values.entry(value.value).or_default();
                descriptions.extend(value.description);
            }
        }
    }
    Ok(fields
        .into_iter()
        .map(|(name, field)| AuthoringField {
            name,
            description: join(field.descriptions),
            presence: if variants != 0 && field.required == variants {
                AuthoringFieldPresence::Required
            } else if field.required == 0 {
                AuthoringFieldPresence::Optional
            } else {
                AuthoringFieldPresence::VariantDependent
            },
            value_kinds: field.kinds.into_iter().collect(),
            allowed_values: field
                .values
                .into_iter()
                .map(|(value, descriptions)| AuthoringLiteralCandidate {
                    value,
                    description: join(descriptions),
                })
                .collect(),
        })
        .collect())
}

fn value_kinds(nodes: &[NodeRef<'_>]) -> Vec<AuthoringValueKind> {
    let mut kinds = BTreeSet::new();
    for node in nodes {
        match node.0.get("type") {
            Some(Value::String(kind)) => insert_kind(kind, &mut kinds),
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .for_each(|kind| insert_kind(kind, &mut kinds)),
            _ => {}
        }
        if node.0.get("properties").is_some() {
            kinds.insert(AuthoringValueKind::Table);
        }
        if node.0.get("items").is_some() {
            kinds.insert(AuthoringValueKind::Array);
        }
    }
    kinds.into_iter().collect()
}

fn insert_kind(kind: &str, output: &mut BTreeSet<AuthoringValueKind>) {
    if let Some(kind) = match kind {
        "boolean" => Some(AuthoringValueKind::Boolean),
        "integer" => Some(AuthoringValueKind::Integer),
        "string" => Some(AuthoringValueKind::String),
        "array" => Some(AuthoringValueKind::Array),
        "object" => Some(AuthoringValueKind::Table),
        _ => None,
    } {
        output.insert(kind);
    }
}

fn allowed_values(nodes: &[NodeRef<'_>]) -> Vec<AuthoringLiteralCandidate> {
    let mut values = BTreeMap::<AuthoringLiteral, BTreeSet<String>>::new();
    for node in nodes {
        if let Some(value) = node.0.get("const").and_then(literal) {
            let descriptions = values.entry(value).or_default();
            if let Some(description) = node.0.get("description").and_then(Value::as_str) {
                descriptions.insert(description.to_owned());
            }
        }
        if let Some(members) = node.0.get("enum").and_then(Value::as_array) {
            for value in members.iter().filter_map(literal) {
                values.entry(value).or_default();
            }
        }
    }
    values
        .into_iter()
        .map(|(value, descriptions)| AuthoringLiteralCandidate {
            value,
            description: join(descriptions),
        })
        .collect()
}

fn literal(value: &Value) -> Option<AuthoringLiteral> {
    match value {
        Value::String(value) => Some(AuthoringLiteral::String(value.clone())),
        Value::Number(value) => value.as_i64().map(AuthoringLiteral::Integer),
        Value::Bool(value) => Some(AuthoringLiteral::Boolean(*value)),
        _ => None,
    }
}

fn join(values: BTreeSet<String>) -> Option<String> {
    (!values.is_empty()).then(|| values.into_iter().collect::<Vec<_>>().join("\n\n"))
}
