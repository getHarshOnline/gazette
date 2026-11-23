//! Select the native literal implementation rows for one exact type alias.

use crate::{Error, invalid};
use serde_json::Value;
use std::collections::BTreeSet;

pub(super) struct Implementation {
    pub(super) html: String,
    pub(super) is_trait: bool,
}

pub(super) fn select(
    bytes: &[u8],
    current_crate: &str,
    self_path: &str,
    origin: &str,
) -> Result<Vec<Implementation>, Error> {
    let reject = || invalid(origin, "unqualified Rustdoc type implementation row");
    let mut selected = Vec::new();
    for (name, rows) in super::data::read(bytes, super::data::Kind::Type, origin)? {
        for row in rows {
            let Value::Array(mut row) = row else {
                return Err(reject());
            };
            if row.len() < 3 || row[0].as_str().is_none_or(str::is_empty) {
                return Err(reject());
            }
            let is_trait = match &row[1] {
                Value::Number(value) if value.as_u64() == Some(0) => false,
                Value::String(value)
                    if !value.is_empty() && !value.chars().any(char::is_control) =>
                {
                    true
                }
                _ => return Err(reject()),
            };
            let mut paths = BTreeSet::new();
            for path in &row[2..] {
                let path = path
                    .as_str()
                    .filter(|path| {
                        !path.is_empty()
                            && !path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
                    })
                    .ok_or_else(reject)?;
                if !paths.insert(path) {
                    return Err(reject());
                }
            }
            if name == current_crate && paths.contains(self_path) {
                let Value::String(html) = row.swap_remove(0) else {
                    unreachable!()
                };
                selected.push(Implementation { html, is_trait });
            }
        }
    }
    if selected.is_empty() {
        return Err(invalid(
            origin,
            "Rustdoc type implementation data has no exact alias",
        ));
    }
    Ok(selected)
}

#[cfg(test)]
#[path = "../../tests/internal/rustdoc_type_impls.rs"]
mod tests;
