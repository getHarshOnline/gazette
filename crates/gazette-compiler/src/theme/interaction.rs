//! Theme-owned interaction intent, not compiled-module or browser-execution evidence.

use std::{fmt, str::FromStr};

use gazette_core::resource::ResourceId;
use serde::{Deserialize, Deserializer, Serialize};

/// Selected module export and mounting convention for an otherwise static component.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentInteraction {
    /// Logical module selected by the current workspace/site, never a native path or URL.
    pub module: ResourceId,
    /// Exact export to invoke; no first-export, default-export or framework fallback.
    pub export: ComponentExport,
    /// Explicit supported mounting behavior; omission never invents a convention.
    pub mount: ComponentMount,
}

/// Supported component enhancement timing; this does not imply framework hydration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ComponentMount {
    /// Mount after document load while retaining the static fallback.
    Load,
}

/// Exact ASCII export identifier, bounded to 128 bytes, without normalization.
/// An explicitly named `default` is a selection, never an implicit fallback.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct ComponentExport(String);

impl schemars::JsonSchema for ComponentExport {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ComponentExport".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "maxLength": 128,
            "pattern": "^[A-Za-z_$]",
            "not": {"pattern": "[^A-Za-z0-9_$]"}
        })
    }
}

/// Unsupported spelling or excessive length of an exact component export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidComponentExport;

impl fmt::Display for InvalidComponentExport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected an ASCII export identifier of 1–128 bytes")
    }
}
impl std::error::Error for InvalidComponentExport {}

impl ComponentExport {
    /// Exact selected export spelling.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ComponentExport {
    type Error = InvalidComponentExport;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let mut bytes = value.bytes();
        if value.len() > 128
            || !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
        {
            return Err(InvalidComponentExport);
        }
        Ok(Self(value))
    }
}

impl FromStr for ComponentExport {
    type Err = InvalidComponentExport;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl fmt::Display for ComponentExport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub(super) fn selected<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<ComponentInteraction>, D::Error> {
    crate::tables::one(deserializer).map(Some)
}
