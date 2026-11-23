//! Qualify retained native trait data that contributes no browser-generated API.

use crate::{Error, invalid};
use serde_json::Value;

pub(super) fn validate(bytes: &[u8], current_crate: &str, origin: &str) -> Result<(), Error> {
    let reject = || invalid(origin, "unqualified Rustdoc trait implementor data");
    for (name, implementations) in super::data::read(bytes, super::data::Kind::Trait, origin)? {
        for implementation in &implementations {
            if !implementation_shape(implementation) {
                return Err(reject());
            }
        }
        // Native register_implementors skips currentCrate before consuming rows;
        // its implementations already reside in the retained static HTML pages.
        // Empty foreign lists also contribute nothing. Ignore-list metadata is
        // deliberately not used to admit nonempty foreign rows in this profile.
        if name != current_crate && !implementations.is_empty() {
            return Err(invalid(
                origin,
                "foreign Rustdoc trait implementors require qualified static-data adaptation",
            ));
        }
    }
    Ok(())
}

fn implementation_shape(value: &Value) -> bool {
    let Some(row) = value.as_array() else {
        return false;
    };
    if !matches!(row.len(), 2 | 4)
        || row[0].as_str().is_none_or(|text| text.is_empty())
        || !matches!(row[1].as_u64(), Some(0 | 1))
    {
        return false;
    }
    row.len() == 2
        || (row[2].as_u64() == Some(1)
            && row[3]
                .as_array()
                .is_some_and(|aliases| aliases.iter().all(Value::is_string)))
}

#[cfg(test)]
#[path = "../../tests/internal/rustdoc_implementors.rs"]
mod tests;
