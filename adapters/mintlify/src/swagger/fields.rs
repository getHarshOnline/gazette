//! Reference positions and opaque payloads are different parts of Swagger.

use std::collections::BTreeSet;

use serde_json::{Value, json};

use crate::{Error, invalid};

pub(super) const METHODS: [&str; 7] = ["get", "put", "post", "delete", "options", "head", "patch"];

#[derive(Clone, Copy)]
enum Kind {
    Root,
    Path,
    Operation,
    Parameter,
    Response,
    Schema,
    Items,
    Other,
}

pub(super) struct Literals {
    key: String,
    values: Vec<Value>,
}

pub(super) fn prepare(
    original: &Value,
    input: &mut Value,
    context: &str,
) -> Result<Literals, Error> {
    let mut key = "x-gazette-retained-literal".to_owned();
    while contains_key(original, &key) {
        key.push('_');
    }
    let mut literals = Literals {
        key,
        values: Vec::new(),
    };
    visit(input, Kind::Root, original, &mut literals, context)?;
    Ok(literals)
}

fn contains_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(map) => map.contains_key(key) || map.values().any(|v| contains_key(v, key)),
        Value::Array(values) => values.iter().any(|v| contains_key(v, key)),
        _ => false,
    }
}

impl Literals {
    fn retain(&mut self, value: &mut Value) {
        let index = self.values.len();
        self.values
            .push(std::mem::replace(value, json!({self.key.clone(): index})));
    }

    pub(super) fn restore(&self, value: &mut Value) -> Result<(), Error> {
        if let Some(index) = value.get(&self.key).and_then(Value::as_u64) {
            let index = usize::try_from(index)
                .map_err(|_| invalid("Swagger literal", "invalid retained literal index"))?;
            *value = self
                .values
                .get(index)
                .ok_or_else(|| invalid("Swagger literal", "unknown retained literal"))?
                .clone();
            return Ok(());
        }
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    self.restore(child)?;
                }
            }
            Value::Array(values) => {
                for child in values {
                    self.restore(child)?;
                }
            }
            _ => (),
        }
        Ok(())
    }
}

fn visit(
    value: &mut Value,
    kind: Kind,
    original: &Value,
    literals: &mut Literals,
    context: &str,
) -> Result<(), Error> {
    // The source schema admits tuples, but the selected OpenAPI 3.0 target
    // cannot express them. Reject before conversion can collapse their items.
    if matches!(kind, Kind::Schema) && value.get("items").is_some_and(Value::is_array) {
        return Err(invalid(
            context,
            "Swagger tuple items cannot be preserved by the OpenAPI 3.0 target",
        ));
    }
    if matches!(
        kind,
        Kind::Path | Kind::Parameter | Kind::Response | Kind::Schema
    ) && let Some(reference) = value.get("$ref")
    {
        let reference = reference
            .as_str()
            .ok_or_else(|| invalid(context, "Swagger reference must be a string"))?;
        target(original, reference, context)?;
    }
    let Some(map) = value.as_object_mut() else {
        return Ok(());
    };
    for (name, child) in map {
        if name.starts_with("x-") {
            literals.retain(child);
            continue;
        }
        let literal = matches!(kind, Kind::Schema | Kind::Parameter | Kind::Items)
            && matches!(name.as_str(), "example" | "default");
        if literal {
            literals.retain(child);
            continue;
        }
        if matches!(kind, Kind::Schema | Kind::Parameter | Kind::Items) && name == "enum" {
            if let Some(values) = child.as_array_mut() {
                for value in values {
                    literals.retain(value);
                }
            }
            continue;
        }
        if matches!(kind, Kind::Response) && name == "examples" {
            if let Some(values) = child.as_object_mut() {
                for value in values.values_mut() {
                    literals.retain(value);
                }
            }
            continue;
        }
        let collection = match (kind, name.as_str()) {
            (Kind::Root, "definitions") | (Kind::Schema, "properties") => Some(Kind::Schema),
            (Kind::Root, "parameters") => Some(Kind::Parameter),
            (Kind::Root, "responses") | (Kind::Operation, "responses") => Some(Kind::Response),
            (Kind::Root, "paths") => Some(Kind::Path),
            (Kind::Response, "headers") => Some(Kind::Items),
            (Kind::Root, "securityDefinitions") => Some(Kind::Other),
            _ => None,
        };
        if let Some(kind) = collection {
            if let Some(values) = child.as_object_mut() {
                for (key, value) in values {
                    if key.starts_with("x-") && matches!(kind, Kind::Path | Kind::Response) {
                        literals.retain(value);
                    } else {
                        visit(value, kind, original, literals, context)?;
                    }
                }
            }
            continue;
        }
        let array = match (kind, name.as_str()) {
            (Kind::Path | Kind::Operation, "parameters") => Some(Kind::Parameter),
            (Kind::Schema, "allOf") => Some(Kind::Schema),
            (Kind::Root, "tags") => Some(Kind::Other),
            _ => None,
        };
        if let Some(kind) = array {
            if let Some(values) = child.as_array_mut() {
                for value in values {
                    visit(value, kind, original, literals, context)?;
                }
            }
            continue;
        }
        let nested = match (kind, name.as_str()) {
            (Kind::Path, method) if METHODS.contains(&method) => Some(Kind::Operation),
            (Kind::Parameter | Kind::Response, "schema")
            | (Kind::Schema, "items" | "additionalProperties") => Some(Kind::Schema),
            (Kind::Parameter | Kind::Items, "items") => Some(Kind::Items),
            (_, "info" | "contact" | "license" | "externalDocs" | "xml") => Some(Kind::Other),
            _ => None,
        };
        if let Some(kind) = nested {
            visit(child, kind, original, literals, context)?;
        }
    }
    Ok(())
}

fn target<'a>(document: &'a Value, reference: &str, context: &str) -> Result<&'a Value, Error> {
    let fragment = reference.strip_prefix('#').ok_or_else(|| {
        invalid(
            context,
            "external Swagger references require a separately captured local document",
        )
    })?;
    let pointer = percent_encoding::percent_decode_str(fragment)
        .decode_utf8()
        .map_err(|_| invalid(context, "invalid Swagger reference encoding"))?;
    document
        .pointer(&pointer)
        .ok_or_else(|| invalid(context, "unresolved local Swagger reference"))
}

pub(super) fn resolve<'a>(
    document: &'a Value,
    mut value: &'a Value,
    context: &str,
) -> Result<&'a Value, Error> {
    let mut seen = BTreeSet::new();
    while let Some(reference) = value.get("$ref").and_then(Value::as_str) {
        if !seen.insert(reference) {
            return Err(invalid(
                context,
                "cyclic Swagger reference at a concrete operation input",
            ));
        }
        value = target(document, reference, context)?;
    }
    Ok(value)
}
