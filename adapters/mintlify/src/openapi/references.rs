//! Typed locations, not arbitrary same-word JSON keys, define reference meaning.

use std::collections::{BTreeMap, BTreeSet};

use percent_encoding::percent_decode_str;
use serde_json::Value;

use super::Version;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Components,
    PathItem,
    Operation,
    Schema,
    Parameter,
    Header,
    RequestBody,
    MediaType,
    Encoding,
    Response,
    Callback,
    Example,
    Link,
    SecurityScheme,
}

struct Reference {
    from: String,
    target: String,
    kind: Kind,
    chain: bool,
}

struct Index<'a> {
    locations: BTreeMap<String, Kind>,
    references: Vec<Reference>,
    operations: BTreeSet<&'a str>,
    operation_links: Vec<&'a str>,
    security: Vec<&'a str>,
    schema_validator: &'a jsonschema::Validator,
    schema_depth: usize,
    version: Version,
}

pub(super) fn validate(
    document: &Value,
    schema_validator: &jsonschema::Validator,
    version: Version,
) -> Result<()> {
    let mut index = Index {
        locations: BTreeMap::new(),
        references: Vec::new(),
        operations: BTreeSet::new(),
        operation_links: Vec::new(),
        security: Vec::new(),
        schema_validator,
        schema_depth: 0,
        version,
    };
    index.visit(Kind::Document, document, String::new())?;
    let mut chains = BTreeMap::new();
    for reference in &index.references {
        if index.locations.get(&reference.target).copied() != Some(reference.kind) {
            return Err("OpenAPI reference target is absent or has the wrong semantic kind");
        }
        if reference.chain {
            chains.insert(reference.from.as_str(), reference.target.as_str());
        }
    }
    // Each location is settled once. References to concrete recursive schemas
    // terminate here; 3.0 reference-only schema aliases must also terminate.
    let mut settled = BTreeSet::new();
    for from in chains.keys() {
        let mut current = *from;
        let mut active = BTreeSet::new();
        while !settled.contains(current) {
            if !active.insert(current) {
                return Err("OpenAPI Reference Object cycle has no concrete target");
            }
            let Some(next) = chains.get(current) else {
                break;
            };
            current = next;
        }
        settled.extend(active);
    }
    if index
        .operation_links
        .iter()
        .any(|id| !index.operations.contains(id))
    {
        return Err("OpenAPI link operationId has no declared operation");
    }
    for name in &index.security {
        let path = child("/components/securitySchemes", name);
        if index.locations.get(&path).copied() != Some(Kind::SecurityScheme) {
            return Err("OpenAPI security requirement has no declared security scheme");
        }
    }
    Ok(())
}

impl<'a> Index<'a> {
    fn visit(&mut self, kind: Kind, value: &'a Value, path: String) -> Result<()> {
        self.locations.insert(path.clone(), kind);
        if let Some(reference) = value.get("$ref").and_then(Value::as_str) {
            if kind == Kind::PathItem && value.as_object().is_some_and(|object| object.len() != 1) {
                return Err(
                    "OpenAPI Path Item $ref siblings are outside the supported target profile; resolve their meaning in the source",
                );
            }
            // In 3.0 this is a Reference Object even at a schema position:
            // siblings are ignored by the specification, not schema locations.
            // In 3.1 a Schema Object's $ref retains its sibling constraints.
            let chain = kind != Kind::Schema || self.version == Version::V3_0;
            self.references.push(Reference {
                from: path.clone(),
                target: pointer(reference)?,
                kind,
                chain,
            });
            if chain {
                return Ok(());
            }
        }
        match kind {
            Kind::Document => {
                self.map(value, &path, "paths", Kind::PathItem, true)?;
                self.map(value, &path, "webhooks", Kind::PathItem, false)?;
                self.field(value, &path, "components", Kind::Components)?;
                self.security(value);
            }
            Kind::Components => {
                for (key, kind) in [
                    ("schemas", Kind::Schema),
                    ("responses", Kind::Response),
                    ("parameters", Kind::Parameter),
                    ("examples", Kind::Example),
                    ("requestBodies", Kind::RequestBody),
                    ("headers", Kind::Header),
                    ("securitySchemes", Kind::SecurityScheme),
                    ("links", Kind::Link),
                    ("callbacks", Kind::Callback),
                    ("pathItems", Kind::PathItem),
                ] {
                    self.map(value, &path, key, kind, false)?;
                }
            }
            Kind::PathItem => {
                for method in [
                    "get", "put", "post", "delete", "options", "head", "patch", "trace",
                ] {
                    self.field(value, &path, method, Kind::Operation)?;
                }
                self.array(value, &path, "parameters", Kind::Parameter)?;
            }
            Kind::Operation => {
                if let Some(id) = value.get("operationId").and_then(Value::as_str)
                    && !self.operations.insert(id)
                {
                    return Err("OpenAPI operationId is duplicated");
                }
                self.array(value, &path, "parameters", Kind::Parameter)?;
                self.field(value, &path, "requestBody", Kind::RequestBody)?;
                self.map(value, &path, "responses", Kind::Response, true)?;
                self.map(value, &path, "callbacks", Kind::Callback, false)?;
                self.security(value);
            }
            Kind::Parameter | Kind::Header | Kind::MediaType => {
                self.field(value, &path, "schema", Kind::Schema)?;
                self.map(value, &path, "content", Kind::MediaType, false)?;
                self.map(value, &path, "examples", Kind::Example, false)?;
                if kind == Kind::MediaType {
                    self.map(value, &path, "encoding", Kind::Encoding, false)?;
                }
            }
            Kind::RequestBody => self.map(value, &path, "content", Kind::MediaType, false)?,
            Kind::Response => {
                self.map(value, &path, "content", Kind::MediaType, false)?;
                self.map(value, &path, "headers", Kind::Header, false)?;
                self.map(value, &path, "links", Kind::Link, false)?;
            }
            Kind::Encoding => self.map(value, &path, "headers", Kind::Header, false)?,
            Kind::Callback => {
                if let Some(object) = value.as_object() {
                    for (key, value) in object {
                        if !key.starts_with("x-") {
                            self.visit(Kind::PathItem, value, child(&path, key))?;
                        }
                    }
                }
            }
            Kind::Link => {
                if let Some(reference) = value.get("operationRef").and_then(Value::as_str) {
                    self.references.push(Reference {
                        from: path.clone(),
                        target: pointer(reference)?,
                        kind: Kind::Operation,
                        chain: false,
                    });
                }
                if let Some(id) = value.get("operationId").and_then(Value::as_str) {
                    self.operation_links.push(id);
                }
            }
            Kind::Schema => self.schema(value, &path)?,
            Kind::Example | Kind::SecurityScheme => (),
        }
        Ok(())
    }

    fn schema(&mut self, value: &'a Value, path: &str) -> Result<()> {
        if self.schema_depth == 0 && !self.schema_validator.is_valid(value) {
            return Err("OpenAPI Schema Object does not conform to the pinned OAS dialect");
        }
        if self.version == Version::V3_1
            && value
                .get("$schema")
                .is_some_and(|dialect| dialect.as_str() != Some(super::RESOURCES[1].0))
        {
            return Err("unsupported OpenAPI Schema Object dialect");
        }
        // The dialect recursively validates each outer schema once. Nested
        // locations still receive address checks, without repeated full validation.
        self.schema_depth += 1;
        if value.get("$recursiveRef").is_some() || value.get("$recursiveAnchor").is_some() {
            return Err(
                "OpenAPI recursive addressing is outside the supported local-pointer profile",
            );
        }
        if ["$id", "$anchor", "$dynamicAnchor", "$dynamicRef"]
            .iter()
            .any(|key| value.get(key).is_some())
        {
            return Err(
                "OpenAPI schema bases, anchors and dynamic references are outside the supported local-pointer profile",
            );
        }
        for key in [
            "$defs",
            "definitions",
            "properties",
            "patternProperties",
            "dependentSchemas",
        ] {
            self.map(value, path, key, Kind::Schema, false)?;
        }
        for key in ["allOf", "anyOf", "oneOf", "prefixItems"] {
            self.array(value, path, key, Kind::Schema)?;
        }
        for key in [
            "items",
            "contains",
            "additionalProperties",
            "unevaluatedProperties",
            "unevaluatedItems",
            "propertyNames",
            "not",
            "if",
            "then",
            "else",
            "contentSchema",
        ] {
            // In 3.0 a boolean additionalProperties is a switch, not an
            // independently referenceable Schema Object (unlike 3.1).
            if self.version == Version::V3_0
                && key == "additionalProperties"
                && value.get(key).is_some_and(Value::is_boolean)
            {
                continue;
            }
            self.field(value, path, key, Kind::Schema)?;
        }
        if let Some(dependencies) = value.get("dependencies").and_then(Value::as_object) {
            let container = child(path, "dependencies");
            for (key, value) in dependencies {
                if !value.is_array() {
                    self.visit(Kind::Schema, value, child(&container, key))?;
                }
            }
        }
        if let Some(mapping) = value
            .get("discriminator")
            .and_then(|v| v.get("mapping"))
            .and_then(Value::as_object)
        {
            for target in mapping.values().filter_map(Value::as_str) {
                let target = if target
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
                    && !target.is_empty()
                {
                    child("/components/schemas", target)
                } else {
                    pointer(target)?
                };
                self.references.push(Reference {
                    from: path.into(),
                    target,
                    kind: Kind::Schema,
                    chain: false,
                });
            }
        }
        self.schema_depth -= 1;
        Ok(())
    }

    fn field(&mut self, value: &'a Value, path: &str, key: &str, kind: Kind) -> Result<()> {
        if let Some(value) = value.get(key) {
            self.visit(kind, value, child(path, key))?;
        }
        Ok(())
    }

    fn map(
        &mut self,
        value: &'a Value,
        path: &str,
        key: &str,
        kind: Kind,
        extensions: bool,
    ) -> Result<()> {
        if let Some(values) = value.get(key).and_then(Value::as_object) {
            let path = child(path, key);
            for (key, value) in values {
                if !extensions || !key.starts_with("x-") {
                    self.visit(kind, value, child(&path, key))?;
                }
            }
        }
        Ok(())
    }

    fn array(&mut self, value: &'a Value, path: &str, key: &str, kind: Kind) -> Result<()> {
        if let Some(values) = value.get(key).and_then(Value::as_array) {
            let path = child(path, key);
            for (index, value) in values.iter().enumerate() {
                self.visit(kind, value, child(&path, &index.to_string()))?;
            }
        }
        Ok(())
    }

    fn security(&mut self, value: &'a Value) {
        if let Some(values) = value.get("security").and_then(Value::as_array) {
            for value in values {
                if let Some(object) = value.as_object() {
                    self.security.extend(object.keys().map(String::as_str));
                }
            }
        }
    }
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn pointer(reference: &str) -> Result<String> {
    let fragment = reference
        .strip_prefix('#')
        .ok_or("Mintlify OpenAPI references must remain within the same retained document")?;
    // The provider exposes an absolute URI parser. A fixed opaque base lets it
    // validate the original fragment grammar without resolution or retrieval.
    // In particular, raw spaces/braces and malformed percent escapes are invalid.
    jsonschema::Uri::parse(format!("urn:gazette:openapi{reference}"))
        .map_err(|_| "OpenAPI reference is not a valid URI fragment")?;
    let decoded = percent_decode_str(fragment)
        .decode_utf8()
        .map_err(|_| "OpenAPI reference fragment is not UTF-8")?;
    if decoded.is_empty() {
        return Ok(String::new());
    }
    let mut path = String::new();
    for token in decoded
        .strip_prefix('/')
        .ok_or("OpenAPI anchors are outside the supported local JSON-pointer profile")?
        .split('/')
    {
        let mut chars = token.chars();
        let mut key = String::new();
        while let Some(c) = chars.next() {
            key.push(if c == '~' {
                match chars.next() {
                    Some('0') => '~',
                    Some('1') => '/',
                    _ => return Err("OpenAPI reference has an invalid JSON-pointer escape"),
                }
            } else {
                c
            });
        }
        path = child(&path, &key);
    }
    Ok(path)
}
