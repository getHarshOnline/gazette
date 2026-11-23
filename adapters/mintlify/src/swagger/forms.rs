//! Preserve every declared form media type and its field serialization.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::fields::{METHODS, resolve};
use crate::{Error, invalid};

pub(super) fn prepare(input: &mut Value) {
    // The provider rejects ssv/pipes outside query parameters before building
    // form content. Gazette restores their exact encoding from original input.
    fn parameter(value: &mut Value) {
        if value["in"] == "formData"
            && value["type"] == "array"
            && matches!(value["collectionFormat"].as_str(), Some("ssv" | "pipes"))
        {
            value["collectionFormat"] = json!("csv");
        }
    }
    if let Some(parameters) = input.get_mut("parameters").and_then(Value::as_object_mut) {
        for value in parameters.values_mut() {
            parameter(value);
        }
    }
    if let Some(paths) = input.get_mut("paths").and_then(Value::as_object_mut) {
        for path in paths.values_mut() {
            if let Some(parameters) = path.get_mut("parameters").and_then(Value::as_array_mut) {
                for value in parameters {
                    parameter(value);
                }
            }
            for method in METHODS {
                if let Some(parameters) = path
                    .get_mut(method)
                    .and_then(|v| v.get_mut("parameters"))
                    .and_then(Value::as_array_mut)
                {
                    for value in parameters {
                        parameter(value);
                    }
                }
            }
        }
    }
}

pub(super) fn restore(original: &Value, converted: &mut Value, context: &str) -> Result<(), Error> {
    let Some(paths) = original.get("paths").and_then(Value::as_object) else {
        return Ok(());
    };
    for (route, path) in paths {
        if !route.starts_with('/') {
            continue;
        }
        let path = resolve(original, path, context)?;
        for method in METHODS {
            let Some(operation) = path.get(method) else {
                continue;
            };
            let parameters = parameters(original, path, operation, context)?;
            let forms: Vec<&Value> = parameters
                .values()
                .copied()
                .filter(|p| p["in"] == "formData")
                .collect();
            if forms.is_empty() {
                continue;
            }
            let request = converted
                .get("paths")
                .and_then(|v| v.get(route))
                .and_then(|v| v.get(method))
                .and_then(|v| v.get("requestBody"))
                .ok_or_else(|| invalid(context, "Swagger conversion lost its form request body"))?;
            let mut request = resolve(converted, request, context)?.clone();
            let content = request
                .get("content")
                .and_then(Value::as_object)
                .ok_or_else(|| invalid(context, "Swagger conversion lost form content"))?;
            let media = content
                .get("multipart/form-data")
                .or_else(|| content.get("application/x-www-form-urlencoded"))
                .ok_or_else(|| invalid(context, "Swagger conversion lost form schema"))?
                .clone();
            let consumes = operation
                .get("consumes")
                .or_else(|| original.get("consumes"));
            let mut types: Vec<&str> = consumes
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            if types.is_empty() {
                types.push("application/x-www-form-urlencoded");
            }
            let mut alternatives = Map::new();
            for media_type in types {
                if !matches!(
                    media_type,
                    "multipart/form-data" | "application/x-www-form-urlencoded"
                ) {
                    return Err(invalid(
                        context,
                        "Swagger form content type cannot be represented faithfully",
                    ));
                }
                let mut representation = media.clone();
                let mut encoding = Map::new();
                for parameter in &forms {
                    let name = parameter["name"]
                        .as_str()
                        .ok_or_else(|| invalid(context, "form parameter has no name"))?;
                    let field = if parameter["type"] == "array" {
                        if media_type == "multipart/form-data" {
                            return Err(invalid(
                                context,
                                "OpenAPI 3.0 multipart encoding cannot preserve Swagger array collectionFormat",
                            ));
                        }
                        match parameter
                            .get("collectionFormat")
                            .and_then(Value::as_str)
                            .unwrap_or("csv")
                        {
                            "csv" => json!({"style":"form", "explode":false}),
                            "multi" => json!({"style":"form", "explode":true}),
                            "ssv" => json!({"style":"spaceDelimited", "explode":false}),
                            "pipes" => json!({"style":"pipeDelimited", "explode":false}),
                            _ => {
                                return Err(invalid(
                                    context,
                                    "Swagger array encoding has no supported OpenAPI equivalent",
                                ));
                            }
                        }
                    } else if parameter["type"] == "file" {
                        json!({"contentType":"application/octet-stream"})
                    } else if media_type == "application/x-www-form-urlencoded" {
                        json!({"style":"form", "explode":true})
                    } else {
                        json!({"contentType":"text/plain"})
                    };
                    encoding.insert(name.into(), field);
                }
                representation["encoding"] = Value::Object(encoding);
                alternatives.insert(media_type.into(), representation);
            }
            request["content"] = Value::Object(alternatives);
            // Detach shared provider request bodies: effective inherited media and
            // field encoding belong to this operation, not another consumer.
            converted["paths"][route][method]["requestBody"] = request;
        }
    }
    Ok(())
}

fn parameters<'a>(
    document: &'a Value,
    path: &'a Value,
    operation: &'a Value,
    context: &str,
) -> Result<BTreeMap<(&'a str, &'a str), &'a Value>, Error> {
    let mut parameters = BTreeMap::new();
    for owner in [path, operation] {
        if let Some(values) = owner.get("parameters").and_then(Value::as_array) {
            for value in values {
                let value = resolve(document, value, context)?;
                let name = value["name"]
                    .as_str()
                    .ok_or_else(|| invalid(context, "Swagger parameter name is missing"))?;
                let location = value["in"]
                    .as_str()
                    .ok_or_else(|| invalid(context, "Swagger parameter location is missing"))?;
                parameters.insert((name, location), value);
            }
        }
    }
    Ok(parameters)
}
