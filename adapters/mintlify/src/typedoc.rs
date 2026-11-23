//! Validate the documented `TypeDoc` input boundary without re-rendering symbols.

use std::collections::BTreeSet;

use serde_json::{Map, Number, Value};

use crate::{Error, invalid};
use gazette_core::documentation::projection::{LinkProjection as SiteLinks, PageKind};
use gazette_core::source::Subdirectory;

pub(crate) fn validate(
    bytes: &[u8],
    context: &str,
    route: &str,
    site_links: &mut SiteLinks,
    catalog: crate::catalog::Scope<'_>,
) -> Result<(), Error> {
    let root = gazette_core::json::parse(bytes)?;
    if root.get("variant").and_then(Value::as_str) != Some("project")
        || root
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        || root.get("schemaVersion").and_then(Value::as_str) != Some("2.0")
    {
        return Err(invalid(
            context,
            "expected a versioned TypeDoc project JSON artifact",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut targets = BTreeSet::new();
    inspect(&root, &mut ids, &mut targets, context)?;
    if !targets.is_subset(&ids) {
        return Err(invalid(
            context,
            "TypeDoc local reflection target is absent",
        ));
    }
    site_links.register_provider(route, PageKind::QualifiedVirtual, None)?;
    catalog.virtual_page(
        site_links,
        route,
        catalog.title(root.get("name").and_then(Value::as_str))?,
        None,
        &[context],
    )
}

fn inspect(
    value: &Value,
    ids: &mut BTreeSet<u64>,
    targets: &mut BTreeSet<u64>,
    context: &str,
) -> Result<(), Error> {
    match value {
        Value::Object(object) => {
            // Reflection identity belongs to the documented `variant` records,
            // not arbitrary nested objects which happen to contain an `id`.
            if object.get("variant").is_some_and(Value::is_string) {
                let id = object
                    .get("id")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| invalid(context, "TypeDoc reflection lacks a nonnegative id"))?;
                if !ids.insert(id) {
                    return Err(invalid(context, "duplicate TypeDoc reflection id"));
                }
            }
            inspect_reference(object, targets, context)?;
            for key in ["children", "childrenIncludingDocuments"] {
                if let Some(children) = object.get(key).and_then(Value::as_array) {
                    for child in children {
                        if let Some(number) = child.as_number() {
                            local_target(number, targets, context)?;
                        }
                    }
                }
            }
            for child in object.values() {
                inspect(child, ids, targets, context)?;
            }
        }
        Value::Array(array) => {
            for child in array {
                inspect(child, ids, targets, context)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn inspect_reference(
    object: &Map<String, Value>,
    targets: &mut BTreeSet<u64>,
    context: &str,
) -> Result<(), Error> {
    if object.get("type").and_then(Value::as_str) == Some("reference") {
        match object.get("target") {
            // TypeDoc 0.28 uses -1 for intentionally unbound or removed types,
            // including inherited members. It is not a dangling local ID.
            Some(Value::Number(number)) if number.as_i64() == Some(-1) => {}
            Some(Value::Number(number)) => local_target(number, targets, context)?,
            Some(Value::Object(symbol)) => symbol_target(symbol, context)?,
            _ => return Err(invalid(context, "invalid TypeDoc type-reference target")),
        }
        if let Some(external) = object.get("externalUrl") {
            external_url(external, context)?;
        }
    } else if object.get("variant").and_then(Value::as_str) == Some("reference") {
        match object.get("target") {
            // A reference reflection also uses -1 when its symbol is absent.
            Some(Value::Number(number)) if number.as_i64() == Some(-1) => {}
            Some(Value::Number(number)) => local_target(number, targets, context)?,
            _ => {
                return Err(invalid(
                    context,
                    "invalid TypeDoc reflection-reference target",
                ));
            }
        }
    } else if object.get("kind").and_then(Value::as_str) == Some("inline-tag") {
        match object.get("target") {
            None => {} // An unresolved comment may retain only its authored text.
            Some(Value::Number(number)) => local_target(number, targets, context)?,
            Some(Value::Object(symbol)) => symbol_target(symbol, context)?,
            Some(value @ Value::String(_)) => external_url(value, context)?,
            _ => return Err(invalid(context, "invalid TypeDoc inline-link target")),
        }
    }
    Ok(())
}

fn local_target(number: &Number, targets: &mut BTreeSet<u64>, context: &str) -> Result<(), Error> {
    let id = number
        .as_u64()
        .ok_or_else(|| invalid(context, "TypeDoc unresolved numeric reference target"))?;
    targets.insert(id);
    Ok(())
}

fn symbol_target(symbol: &Map<String, Value>, context: &str) -> Result<(), Error> {
    let text = |key| {
        symbol
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty() && !value.chars().any(char::is_control))
    };
    let (Some(package), Some(path), Some(_name)) = (
        text("packageName"),
        text("packagePath"),
        text("qualifiedName"),
    ) else {
        return Err(invalid(context, "invalid TypeDoc symbol identity"));
    };
    // These are opaque identities, never filesystem reads or inferred links.
    // The supported profile excludes TypeDoc's unknown/absolute-path fallback.
    if symbol.len() != 3
        || package == "<unknown>"
        || package.parse::<Subdirectory>().is_err()
        || path.parse::<Subdirectory>().is_err()
    {
        return Err(invalid(context, "unsupported TypeDoc symbol identity path"));
    }
    Ok(())
}

fn external_url(value: &Value, context: &str) -> Result<(), Error> {
    let target = value
        .as_str()
        .and_then(|value| url::Url::parse(value).ok())
        .ok_or_else(|| invalid(context, "TypeDoc external target is not an absolute URL"))?;
    if !matches!(target.scheme(), "http" | "https")
        || target.host_str().is_none()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err(invalid(
            context,
            "TypeDoc external target must be an uncredentialed HTTP URL",
        ));
    }
    Ok(())
}
