//! Public browser-client bindings, never consent, provider readiness or native effects.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Explicit GA4 tag identity with bounded literal spelling, not a credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct GoogleMeasurementId(String);

/// A tag identity is not a bounded `G-` prefixed uppercase alphanumeric literal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidGoogleMeasurementId;

impl GoogleMeasurementId {
    /// Borrow the exact configured spelling without guessing a property or owner.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for GoogleMeasurementId {
    type Err = InvalidGoogleMeasurementId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() <= 64
            && value.strip_prefix("G-").is_some_and(|suffix| {
                !suffix.is_empty()
                    && suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
            })
        {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidGoogleMeasurementId)
        }
    }
}

impl TryFrom<String> for GoogleMeasurementId {
    type Error = InvalidGoogleMeasurementId;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl fmt::Display for InvalidGoogleMeasurementId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("expected a bounded literal GA4 measurement ID (G- followed by uppercase letters or digits)")
    }
}

impl std::error::Error for InvalidGoogleMeasurementId {}

impl schemars::JsonSchema for GoogleMeasurementId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "GoogleMeasurementId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string", "pattern": "^G-[A-Z0-9]+$", "maxLength": 64,
            "not": {"pattern": "[^A-Z0-9-]"},
            "description": "Explicit public GA4 tag identity; not proof of property readiness or consent."
        })
    }
}

/// Closed read-only public-page operations; Gazette's native MCP effects are excluded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WebMcpTool {
    /// Return the already admitted current-page projection.
    CurrentPage,
    /// Query the site's explicitly selected public search corpus.
    Search,
}

/// Explicit nonempty, unique tool selection in caller-declared order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<WebMcpTool>")]
pub struct WebMcpTools(Vec<WebMcpTool>);

/// Tool selection is empty or repeats an operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidWebMcpTools;

impl WebMcpTools {
    /// Borrow the exact operation selection; no tools are enabled by default.
    pub fn as_slice(&self) -> &[WebMcpTool] {
        &self.0
    }
}

impl TryFrom<Vec<WebMcpTool>> for WebMcpTools {
    type Error = InvalidWebMcpTools;

    fn try_from(value: Vec<WebMcpTool>) -> Result<Self, Self::Error> {
        if value.is_empty() || value.len() > 2 || value.len() == 2 && value[0] == value[1] {
            Err(InvalidWebMcpTools)
        } else {
            Ok(Self(value))
        }
    }
}

impl fmt::Display for InvalidWebMcpTools {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("select a nonempty unique subset of current_page and search")
    }
}

impl std::error::Error for InvalidWebMcpTools {}

impl schemars::JsonSchema for WebMcpTools {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "WebMcpTools".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "array", "items": generator.subschema_for::<WebMcpTool>(),
            "minItems": 1, "maxItems": 2, "uniqueItems": true,
            "description": "Explicit read-only public-page tool selection; no native MCP effects."
        })
    }
}
