//! Exact local names for selected Theme components.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// ASCII letter followed by ASCII letters, digits, hyphens or underscores.
/// Names retain their spelling and do not identify publication entities.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ComponentName(String);

impl schemars::JsonSchema for ComponentName {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ComponentName".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Exclusion checks the entire string, including trailing line terminators;
        // JSON Schema's `$` anchor alone can accept a final newline.
        schemars::json_schema!({
            "type": "string",
            "description": "Exact Theme-local component name, not a publication identity.",
            "pattern": "^[A-Za-z]",
            "not": {"pattern": "[^A-Za-z0-9_-]"}
        })
    }
}

/// A local component name does not satisfy the ASCII name grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidComponentName;

impl fmt::Display for InvalidComponentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "expected an ASCII letter followed by ASCII letters, digits, hyphens or underscores",
        )
    }
}

impl std::error::Error for InvalidComponentName {}

impl ComponentName {
    /// Exact declared spelling, without case folding or normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ComponentName {
    type Error = InvalidComponentName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut bytes = value.bytes();
        if !bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(InvalidComponentName);
        }
        Ok(Self(value))
    }
}

impl FromStr for ComponentName {
    type Err = InvalidComponentName;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for ComponentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
