//! Exact native literal-data envelopes shared by Rustdoc implementation projections.

use crate::{Error, invalid};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) enum Kind {
    Trait,
    Type,
}

const TRAIT_PREFIX: &str = "(function() {\n    const implementors = Object.fromEntries(";
const TRAIT_REGISTRATION: &str = concat!(
    ");\n",
    "    if (window.register_implementors) {\n",
    "        window.register_implementors(implementors);\n",
    "    } else {\n",
    "        window.pending_implementors = implementors;\n",
    "    }\n",
    "})()"
);

const TYPE_PREFIX: &str = "(function() {\n    var type_impls = Object.fromEntries(";
const TYPE_REGISTRATION: &str = concat!(
    ");\n",
    "    if (window.register_type_impls) {\n",
    "        window.register_type_impls(type_impls);\n",
    "    } else {\n",
    "        window.pending_type_impls = type_impls;\n",
    "    }\n",
    "})()"
);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fragments {
    start: usize,
    fragment_lengths: Vec<usize>,
}

pub(super) fn read(
    bytes: &[u8],
    kind: Kind,
    origin: &str,
) -> Result<BTreeMap<String, Vec<Value>>, Error> {
    let (prefix, registration, reason) = match kind {
        Kind::Trait => (
            TRAIT_PREFIX,
            TRAIT_REGISTRATION,
            "unqualified Rustdoc trait implementor data",
        ),
        Kind::Type => (
            TYPE_PREFIX,
            TYPE_REGISTRATION,
            "unqualified Rustdoc type implementation data",
        ),
    };
    let reject = || invalid(origin, reason);
    let source = std::str::from_utf8(bytes).map_err(|_| reject())?;
    let (script, metadata) = source.rsplit_once("\n//").ok_or_else(reject)?;
    let literal = script
        .strip_prefix(prefix)
        .and_then(|body| body.strip_suffix(registration))
        .ok_or_else(reject)?;
    let value = gazette_core::json::parse(literal.as_bytes()).map_err(|_| reject())?;
    let entries = value.as_array().ok_or_else(reject)?;
    let metadata = gazette_core::json::parse(metadata.as_bytes()).map_err(|_| reject())?;
    let fragments: Fragments = serde_json::from_value(metadata).map_err(|_| reject())?;
    if fragments.start != prefix.len() + 1 || fragments.fragment_lengths.len() != entries.len() {
        return Err(reject());
    }
    let body = literal
        .strip_prefix('[')
        .and_then(|body| body.strip_suffix(']'))
        .ok_or_else(reject)?;
    let mut offset = 0_usize;
    let mut crates = BTreeSet::new();
    for (index, (entry, length)) in entries.iter().zip(&fragments.fragment_lengths).enumerate() {
        let end = offset.checked_add(*length).ok_or_else(reject)?;
        let fragment = body.get(offset..end).ok_or_else(reject)?;
        let fragment = if index == 0 {
            fragment
        } else {
            fragment.strip_prefix(',').ok_or_else(reject)?
        };
        if gazette_core::json::parse(fragment.as_bytes()).map_err(|_| reject())? != *entry {
            return Err(reject());
        }
        offset = end;
        let pair = entry
            .as_array()
            .filter(|pair| pair.len() == 2)
            .ok_or_else(reject)?;
        let name = pair[0]
            .as_str()
            .filter(|name| !name.is_empty() && !name.chars().any(char::is_control))
            .ok_or_else(reject)?;
        if !crates.insert(name) {
            // Object.fromEntries would silently replace a prior crate entry.
            return Err(reject());
        }
        pair[1].as_array().ok_or_else(reject)?;
    }
    if offset != body.len() {
        return Err(reject());
    }
    let entries: Vec<(String, Vec<Value>)> = serde_json::from_value(value).map_err(|_| reject())?;
    Ok(entries.into_iter().collect())
}
