//! Target-safe security identities and exact native OAuth endpoint values.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use super::fields::METHODS;
use crate::{Error, invalid};

pub(super) fn prepare(input: &mut Value, context: &str) -> Result<BTreeMap<String, String>, Error> {
    let Some(schemes) = input.get("securityDefinitions").and_then(Value::as_object) else {
        return Ok(BTreeMap::new());
    };
    let mut occupied: BTreeSet<String> = schemes.keys().cloned().collect();
    let mut names = BTreeMap::new();
    for name in schemes.keys().collect::<BTreeSet<_>>() {
        let safe = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte));
        let target = if safe {
            name.clone()
        } else {
            let mut index = names.len();
            loop {
                let candidate = format!("gazette-security-{index}");
                if occupied.insert(candidate.clone()) {
                    break candidate;
                }
                index += 1;
            }
        };
        names.insert(name.clone(), target);
    }
    let mut converted = Map::new();
    for (name, target) in &names {
        let mut scheme = schemes[name].clone();
        if name != target {
            if scheme.get("x-gazette-original-name").is_some() {
                return Err(invalid(
                    context,
                    "reserved Gazette security-name extension is already declared",
                ));
            }
            scheme["x-gazette-original-name"] = Value::String(name.clone());
        }
        converted.insert(target.clone(), scheme);
    }
    input["securityDefinitions"] = Value::Object(converted);
    requirements(input.get_mut("security"), &names, context)?;
    if let Some(paths) = input.get_mut("paths").and_then(Value::as_object_mut) {
        for path in paths.values_mut() {
            for method in METHODS {
                if let Some(operation) = path.get_mut(method) {
                    requirements(operation.get_mut("security"), &names, context)?;
                }
            }
        }
    }
    Ok(names)
}

fn requirements(
    value: Option<&mut Value>,
    names: &BTreeMap<String, String>,
    context: &str,
) -> Result<(), Error> {
    let Some(values) = value.and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for requirement in values {
        let mut updated = Map::new();
        if let Some(map) = requirement.as_object() {
            for (name, scopes) in map {
                let target = names
                    .get(name)
                    .ok_or_else(|| invalid(context, "unknown Swagger security requirement"))?;
                updated.insert(target.clone(), scopes.clone());
            }
        }
        *requirement = Value::Object(updated);
    }
    Ok(())
}

pub(super) fn restore(
    original: &Value,
    converted: &mut Value,
    names: &BTreeMap<String, String>,
    context: &str,
) -> Result<(), Error> {
    for (name, target) in names {
        let scheme = &original["securityDefinitions"][name];
        if scheme["type"] != "oauth2" {
            continue;
        }
        let flow = match scheme["flow"].as_str() {
            Some("application") => "clientCredentials",
            Some("accessCode") => "authorizationCode",
            Some("implicit") => "implicit",
            Some("password") => "password",
            _ => return Err(invalid(context, "invalid Swagger OAuth flow")),
        };
        let flow = converted
            .get_mut("components")
            .and_then(|v| v.get_mut("securitySchemes"))
            .and_then(|v| v.get_mut(target))
            .and_then(|v| v.get_mut("flows"))
            .and_then(|v| v.get_mut(flow))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid(context, "Swagger conversion lost its OAuth flow"))?;
        for field in ["authorizationUrl", "tokenUrl"] {
            if let Some(value) = scheme.get(field) {
                flow.insert(field.into(), value.clone());
            }
        }
    }
    Ok(())
}
