//! Plugin package identity, independent of capture, activation, providers and resources.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};

use crate::source::Sha256Digest;

/// Lower-kebab plugin package identity; exact captured digests complete its identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PluginPackageId(String);

/// Invalid [`PluginPackageId`] spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidPluginPackageId;

/// One immutable captured plugin identity; mutable versions or compatibility aliases are absent.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageIdentity {
    /// Exact package declaration.
    pub package: PluginPackageId,
    /// Exact selected source-binding bytes or coordinate record.
    pub source: Sha256Digest,
    /// Exact retained manifest bytes.
    pub manifest: Sha256Digest,
    /// Exact captured package-tree inventory and bytes.
    pub tree: Sha256Digest,
}

impl PluginPackageId {
    /// Exact declared spelling; no provider, resource or activation is inferred.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for PluginPackageId {
    type Err = InvalidPluginPackageId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if valid_plain_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidPluginPackageId)
        }
    }
}

impl<'de> Deserialize<'de> for PluginPackageId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for PluginPackageId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PluginPackageId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Lower-kebab plugin package identity; exact captured digests complete its identity.",
            "pattern": "^[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*$"
        })
    }
}

impl fmt::Display for PluginPackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidPluginPackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid PluginPackageId")
    }
}

impl std::error::Error for InvalidPluginPackageId {}

fn valid_plain_id(value: &str) -> bool {
    let mut segments = value.split('-');
    segments.next().is_some_and(valid_segment) && segments.all(valid_segment)
}

fn valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}
