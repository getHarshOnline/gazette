//! Offline `OpenAPI` target profile; original descriptions are never rewritten.

mod references;
mod yaml;

use std::sync::OnceLock;

use gazette_core::json;
use serde_json::Value;

use crate::{Error, invalid};
use gazette_core::documentation::projection::{LinkProjection as SiteLinks, PageKind};

const RESOURCES: [(&str, &str); 5] = [
    (
        "https://spec.openapis.org/oas/3.1/schema/2025-11-23",
        include_str!("openapi/schema.json"),
    ),
    (
        "https://spec.openapis.org/oas/3.1/dialect/2024-11-10",
        include_str!("openapi/dialect.json"),
    ),
    (
        "https://spec.openapis.org/oas/3.1/meta/2024-11-10",
        include_str!("openapi/meta.json"),
    ),
    (
        "https://spec.openapis.org/oas/3.0/schema/2024-10-18",
        include_str!("openapi/schema-3.0.json"),
    ),
    (
        "http://swagger.io/v2/schema.json",
        include_str!("openapi/schema-2.0.json"),
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Version {
    V3_0,
    V3_1,
}

pub(super) fn validate(bytes: &[u8], context: &str) -> Result<Value, Error> {
    let value = parse(bytes, context, false)?;
    validate_value(&value, context)?;
    Ok(value)
}

pub(super) fn validate_yaml(bytes: &[u8], context: &str) -> Result<Value, Error> {
    let value = parse(bytes, context, true)?;
    validate_value(&value, context)?;
    Ok(value)
}

pub(super) fn register(route: &str, site_links: &mut SiteLinks) -> Result<(), Error> {
    site_links.register_provider(route, PageKind::QualifiedVirtual, None)
}

pub(super) fn parse(bytes: &[u8], context: &str, yaml: bool) -> Result<Value, Error> {
    let value = if yaml {
        self::yaml::parse(bytes).map_err(|reason| invalid(context, reason))?
    } else {
        json::parse(bytes).map_err(|_| invalid(context, "invalid or ambiguous OpenAPI JSON"))?
    };
    let mut remaining = 100_000;
    count(&value, &mut remaining).map_err(|reason| invalid(context, reason))?;
    Ok(value)
}

pub(super) fn validate_swagger(value: &Value, context: &str) -> Result<(), Error> {
    let mut remaining = 100_000;
    count(value, &mut remaining).map_err(|reason| invalid(context, reason))?;
    static VALIDATOR: OnceLock<Result<jsonschema::Validator, String>> = OnceLock::new();
    let validator = VALIDATOR
        .get_or_init(|| {
            let schema: Value =
                serde_json::from_str(RESOURCES[4].1).map_err(|error| error.to_string())?;
            jsonschema::draft4::options()
                .with_retriever(BundledOnly)
                .build(&schema)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|_| invalid(context, "bundled Swagger 2 validator unavailable"))?;
    if !validator.is_valid(value) {
        return Err(invalid(
            context,
            "Swagger 2 input does not conform to its pinned schema",
        ));
    }
    Ok(())
}

pub(super) fn validate_value(value: &Value, context: &str) -> Result<(), Error> {
    let mut remaining = 100_000;
    count(value, &mut remaining).map_err(|reason| invalid(context, reason))?;
    let version = match value.get("openapi").and_then(Value::as_str) {
        Some("3.0.0" | "3.0.1" | "3.0.2" | "3.0.3" | "3.0.4") => Version::V3_0,
        Some("3.1.0" | "3.1.1" | "3.1.2") => Version::V3_1,
        _ => {
            return Err(invalid(
                context,
                "supported OpenAPI versions are 3.0.0 through 3.0.4 and 3.1.0 through 3.1.2; no conversion performed",
            ));
        }
    };
    let schema = validator(version).map_err(|reason| {
        invalid(
            format!("{context} (bundled validator: {reason})"),
            "bundled OpenAPI validator unavailable; no artifact accepted",
        )
    })?;
    if !schema.document.is_valid(value) {
        return Err(invalid(
            context,
            "OpenAPI document or Schema Object does not conform to its pinned version profile",
        ));
    }
    if version == Version::V3_1
        && value
            .get("jsonSchemaDialect")
            .is_some_and(|dialect| dialect.as_str() != Some(RESOURCES[1].0))
    {
        return Err(invalid(context, "unsupported OpenAPI JSON Schema dialect"));
    }
    references::validate(value, &schema.schema, version).map_err(|reason| invalid(context, reason))
}

fn count(value: &Value, remaining: &mut usize) -> Result<(), &'static str> {
    *remaining = remaining
        .checked_sub(1)
        .ok_or("OpenAPI JSON exceeds 100000 values; narrow the artifact")?;
    match value {
        Value::Array(values) => {
            for value in values {
                count(value, remaining)?;
            }
        }
        Value::Object(values) => {
            for value in values.values() {
                count(value, remaining)?;
            }
        }
        _ => (),
    }
    Ok(())
}

struct Validators {
    document: jsonschema::Validator,
    schema: jsonschema::Validator,
}

fn validator(version: Version) -> Result<&'static Validators, &'static str> {
    static V3_0: OnceLock<Result<Validators, String>> = OnceLock::new();
    static V3_1: OnceLock<Result<Validators, String>> = OnceLock::new();
    let validator = match version {
        Version::V3_0 => &V3_0,
        Version::V3_1 => &V3_1,
    };
    validator
        .get_or_init(|| {
            if version == Version::V3_0 {
                let document: Value =
                    serde_json::from_str(RESOURCES[3].1).map_err(|error| error.to_string())?;
                // Use the upstream definition in its original resource scope,
                // not a detached copy whose local references change meaning.
                let schema =
                    serde_json::json!({"$ref": format!("{}#/definitions/Schema", RESOURCES[3].0)});
                let compile = |value: &Value| {
                    jsonschema::draft4::options()
                        .with_retriever(BundledOnly)
                        .build(value)
                        .map_err(|error| error.to_string())
                };
                return Ok(Validators {
                    document: compile(&document)?,
                    schema: compile(&schema)?,
                });
            }
            // Separate standard validators avoid the selected provider's
            // cross-resource dynamic-scope failure in schema-base. No upstream
            // schema is rewritten and each typed Schema Object is still checked.
            let compile = |bytes: &str| {
                let value: Value =
                    serde_json::from_str(bytes).map_err(|error| error.to_string())?;
                jsonschema::draft202012::options()
                    .with_retriever(BundledOnly)
                    .build(&value)
                    .map_err(|error| error.to_string())
            };
            Ok(Validators {
                document: compile(RESOURCES[0].1)?,
                schema: compile(RESOURCES[1].1)?,
            })
        })
        .as_ref()
        .map_err(String::as_str)
}

struct BundledOnly;
impl jsonschema::Retrieve for BundledOnly {
    fn retrieve(
        &self,
        uri: &jsonschema::Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let (_, bytes) = RESOURCES
            .iter()
            .find(|(id, _)| *id == uri.as_str())
            .ok_or("unbundled OpenAPI schema retrieval forbidden")?;
        Ok(serde_json::from_str(bytes)?)
    }
}
