//! Observed entry names, distinct from selected subdirectories and URL routes.

use std::borrow::Borrow;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::Serialize;

use super::{Error, entry_name};

/// A nonempty root-relative UTF-8 entry path with literal, validated components.
///
/// Unlike a selected subdirectory, observed names retain literal percent signs
/// and colons. They are never decoded or treated as URLs or platform prefixes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct EntryPath(String);

impl schemars::JsonSchema for EntryPath {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "EntryPath".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        // Cc matches Rust's control check. The explicit end assertion avoids
        // ECMAScript `$` accepting a position before a final line separator.
        // Scope negation to strings: Option<Self> may add null to the outer type.
        // A pattern alone succeeds for null, so untyped negation would reject it.
        schemars::json_schema!({
            "type": "string",
            "description": "Literal nonempty root-relative entry path; no empty/dot components, backslash or controls. Percent signs, colons and Unicode are not decoded or normalized.",
            "pattern": r"^[^/]+(?:/[^/]+)*(?![\s\S])",
            "not": {"type": "string", "anyOf": [
                {"pattern": r"[\\\u0000-\u001f\u007f-\u009f]"},
                {"pattern": r"(^|/)\.{1,2}(/|(?![\s\S]))"}
            ]}
        })
    }
}

impl FromStr for EntryPath {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        for component in value.split('/') {
            entry_name(component.as_bytes(), Path::new(value))?;
        }
        Ok(Self(value.to_owned()))
    }
}

impl<'de> serde::Deserialize<'de> for EntryPath {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        <String as serde::Deserialize>::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl EntryPath {
    /// Borrow the exact relative spelling, without normalization.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for EntryPath {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for EntryPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
