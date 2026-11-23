//! Gazette-owned capability contract identities, independent of provider selection.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};

/// A typed, revisioned semantic promise that a provider may implement and a Theme may consume.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct CapabilityId(String);

/// A capability identity lacks a namespaced contract or explicit numeric revision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCapabilityId;

/// Gazette-owned registry of core capability contracts.
///
/// Plugin-owned extension contracts remain manifest-declared and package-namespaced;
/// workspace selection cannot add entries to this registry or authorize a typo.
#[derive(Clone, Copy, Debug, Default)]
pub struct CoreCapabilityCatalog;

impl CoreCapabilityCatalog {
    /// Stable core contract identities understood by this Gazette build.
    pub const IDS: &'static [&'static str] = &[
        "commerce.catalog/v1",
        "commerce.inventory/v1",
        "commerce.selector/v1",
        "publication.analytics/v1",
        "publication.search/v1",
        "publication.webmcp/v1",
    ];

    /// Whether the exact revisioned identity is a Gazette core contract.
    pub fn contains(capability: &CapabilityId) -> bool {
        Self::IDS.binary_search(&capability.as_str()).is_ok()
    }

    /// Gazette's native publication-search contract identity.
    pub fn publication_search() -> CapabilityId {
        CapabilityId("publication.search/v1".to_owned())
    }

    /// Gazette's public analytics-client binding contract, not runtime consent.
    pub fn publication_analytics() -> CapabilityId {
        CapabilityId("publication.analytics/v1".to_owned())
    }

    /// Gazette's selected read-only browser-tool binding contract.
    pub fn publication_webmcp() -> CapabilityId {
        CapabilityId("publication.webmcp/v1".to_owned())
    }
}

impl CapabilityId {
    /// Exact declared spelling; no provider or package identity is inferred from it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for CapabilityId {
    type Err = InvalidCapabilityId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if valid(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidCapabilityId)
        }
    }
}

impl<'de> Deserialize<'de> for CapabilityId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for CapabilityId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "CapabilityId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Namespaced Gazette capability contract and numeric revision, such as commerce.catalog/v1.",
            "pattern": "^[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*(?:\\.[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*)+/v[0-9]+$"
        })
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidCapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid CapabilityId")
    }
}

impl std::error::Error for InvalidCapabilityId {}

fn valid(value: &str) -> bool {
    let mut parts = value.split('/');
    let Some(contract) = parts.next() else {
        return false;
    };
    let Some(revision) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && contract.contains('.')
        && contract.split('.').all(valid_plain_id)
        && revision.strip_prefix('v').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

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
