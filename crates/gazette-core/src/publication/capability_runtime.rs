//! Provider-neutral capability acquisition values and frozen typed results.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize};

use crate::plugin::{PluginPackageId, PluginPackageIdentity};

use super::{CapabilityId, CoreCapabilityCatalog, Key, routing::RoutePath};

/// Provider-local lower-kebab identity; ownership is carried separately by [`ProviderRef`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ProviderId(String);

/// Invalid [`ProviderId`] spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidProviderId;

impl ProviderId {
    /// Exact declared spelling; no owner, capability, plugin or resource is inferred.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ProviderId {
    type Err = InvalidProviderId;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if valid_plain_id(value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(InvalidProviderId)
        }
    }
}

impl<'de> Deserialize<'de> for ProviderId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for ProviderId {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ProviderId".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "Provider-local lower-kebab identity; ownership is carried separately by ProviderRef.",
            "pattern": "^[a-z][a-z0-9]*(?:-[a-z][a-z0-9]*)*$"
        })
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for InvalidProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid ProviderId")
    }
}

impl std::error::Error for InvalidProviderId {}

/// Exact selected provider and its implementation owner.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ProviderRef {
    /// Provider implemented by Gazette itself.
    Gazette {
        /// Provider-local identity in Gazette's native provider catalog.
        provider: ProviderId,
    },
    /// Provider exported by one explicitly activated plugin package.
    Plugin {
        /// Plugin package exporting the provider; this reference does not activate it.
        package: PluginPackageId,
        /// Provider-local identity declared by that package.
        provider: ProviderId,
    },
}

impl ProviderRef {
    /// Provider-local identity without erasing its owner.
    pub fn provider(&self) -> &ProviderId {
        match self {
            Self::Gazette { provider } | Self::Plugin { provider, .. } => provider,
        }
    }
}

/// Exact unavailable state a workspace may permit for an optional selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityAbsence {
    /// No provider runtime was configured for the selected binding.
    Unconfigured,
    /// The selected provider cannot supply this exact request.
    Unsupported,
    /// The selected provider has no current value for this capability.
    Missing,
    /// The selected value fails the accepted freshness policy.
    Stale,
    /// The source owner revoked the selected value or access to it.
    Revoked,
    /// The caller is not authorized to acquire the selected value.
    Unauthorized,
    /// The selected provider requires a retained native acquisition that was not supplied.
    NoNativeAcquisition,
}

impl<'de> Deserialize<'de> for CapabilityAbsence {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match String::deserialize(deserializer)?.as_str() {
            "unconfigured" => Ok(Self::Unconfigured),
            "unsupported" => Ok(Self::Unsupported),
            "missing" => Ok(Self::Missing),
            "stale" => Ok(Self::Stale),
            "revoked" => Ok(Self::Revoked),
            "unauthorized" => Ok(Self::Unauthorized),
            "no-native-acquisition" => Ok(Self::NoNativeAcquisition),
            value => Err(serde::de::Error::unknown_variant(
                value,
                &[
                    "unconfigured",
                    "unsupported",
                    "missing",
                    "stale",
                    "revoked",
                    "unauthorized",
                    "no-native-acquisition",
                ],
            )),
        }
    }
}

/// Exact admitted runtime identity; a package reference alone cannot select executable bytes.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProviderRuntimeIdentity {
    /// Gazette-owned implementation selected by its native identity.
    Gazette {
        /// Provider-local native identity.
        provider: ProviderId,
    },
    /// Plugin implementation bound to one exact admitted package capture.
    Plugin {
        /// Composite source/manifest/tree identity of the activated package.
        package: PluginPackageIdentity,
        /// Provider-local identity exported by that exact package.
        provider: ProviderId,
    },
}

impl ProviderRuntimeIdentity {
    /// Selection identity without erasing whether the runtime is built-in or package-owned.
    pub fn provider_ref(&self) -> ProviderRef {
        match self {
            Self::Gazette { provider } => ProviderRef::Gazette {
                provider: provider.clone(),
            },
            Self::Plugin { package, provider } => ProviderRef::Plugin {
                package: package.package.clone(),
                provider: provider.clone(),
            },
        }
    }
}

/// One complete invocation for one provider group and consuming site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AcquisitionRequest {
    site: Key,
    provider: ProviderRuntimeIdentity,
    capabilities: Vec<CapabilityId>,
}

impl AcquisitionRequest {
    /// Construct the stable application-owned provider group.
    pub fn new(
        site: Key,
        provider: ProviderRuntimeIdentity,
        capabilities: Vec<CapabilityId>,
    ) -> Self {
        Self {
            site,
            provider,
            capabilities,
        }
    }

    /// Exact consuming site; providers never infer it from package or resource selection.
    pub fn site(&self) -> &Key {
        &self.site
    }

    /// Exact selected provider.
    pub fn provider(&self) -> &ProviderRuntimeIdentity {
        &self.provider
    }

    /// Exact requested capability membership in canonical identity order.
    pub fn capabilities(&self) -> &[CapabilityId] {
        &self.capabilities
    }
}

/// Gazette-owned core capability value; raw provider DTOs never cross this boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "contract", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CapabilityValue {
    /// An admitted public client binding, not a Google observation or consent receipt.
    PublicationAnalyticsV1 {
        /// Exact explicitly configured public tag identity.
        measurement_id: super::browser::GoogleMeasurementId,
    },
    /// Read-only page tools; browser registration remains explicit and feature-detected.
    #[serde(rename = "publication-webmcp-v1")]
    PublicationWebMcpV1 {
        /// Exactly selected public operations; no native filesystem or release tools.
        tools: super::browser::WebMcpTools,
    },
    /// The complete Gazette search artifact exposed to presentation.
    PublicationSearchV1 {
        /// Canonical public route whose local output is Gazette-derived.
        index: RoutePath,
        /// Closed Gazette search-document encoding contract.
        format: SearchIndexFormat,
    },
}

/// Closed encoding of the Gazette-owned publication search artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SearchIndexFormat {
    /// Complete ordered Gazette search documents encoded as compact JSON.
    GazetteDocumentsV1,
}

impl CapabilityValue {
    /// Gazette contract implemented by this typed value.
    pub fn capability(&self) -> CapabilityId {
        match self {
            Self::PublicationAnalyticsV1 { .. } => CoreCapabilityCatalog::publication_analytics(),
            Self::PublicationWebMcpV1 { .. } => CoreCapabilityCatalog::publication_webmcp(),
            Self::PublicationSearchV1 { .. } => CoreCapabilityCatalog::publication_search(),
        }
    }
}

/// Opaque provider-owned coordinates retained as private build evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderEvidence {
    /// Exact provider-native source reference after adapter translation.
    pub source: String,
    /// Exact provider-native revision reference, treated as opaque.
    pub revision: String,
}

/// One provider result for one exact requested capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CapabilityResult {
    /// Exact requested capability identity.
    pub capability: CapabilityId,
    /// Typed value/evidence or exact unavailable outcome.
    #[serde(flatten)]
    pub outcome: CapabilityResultOutcome,
}

/// Provider result state before application admission.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum CapabilityResultOutcome {
    /// Complete typed value and private provider coordinates.
    Available {
        /// Gazette-owned value projection.
        value: CapabilityValue,
        /// Provider-native coordinates, never public Theme data.
        evidence: ProviderEvidence,
    },
    /// Explicit absence, never permission to invent a substitute.
    Unavailable {
        /// Exact observed condition.
        reason: CapabilityAbsence,
    },
}

/// One complete translated response for one provider group.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProviderSnapshot {
    provider: ProviderRuntimeIdentity,
    results: Vec<CapabilityResult>,
}

impl ProviderSnapshot {
    /// Construct a response for application-owned exact-membership validation.
    pub fn new(provider: ProviderRuntimeIdentity, results: Vec<CapabilityResult>) -> Self {
        Self { provider, results }
    }

    /// Exact provider claimed by this response.
    pub fn provider(&self) -> &ProviderRuntimeIdentity {
        &self.provider
    }

    /// Results in provider-returned order; the application canonicalizes only after validation.
    pub fn results(&self) -> &[CapabilityResult] {
        &self.results
    }
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
