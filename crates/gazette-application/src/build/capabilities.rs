//! One-shot capability acquisition and deterministic frozen application capture.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::Arc,
};

use gazette_compiler::{toml, workspace::CapabilitySelection};
use gazette_core::publication::{
    CapabilityId, CoreCapabilityCatalog, Key,
    capability_runtime::{
        AcquisitionRequest, CapabilityAbsence, CapabilityResult, CapabilityResultOutcome,
        CapabilityValue, ProviderEvidence, ProviderId, ProviderRef, ProviderRuntimeIdentity,
        ProviderSnapshot, SearchIndexFormat,
    },
};
use gazette_ports::capability::{CapabilityAcquisition, Failure, FailureKind};
use serde::Serialize;

use super::{AcquiredInputs, MetadataError};

/// Bounds for one complete capability acquisition phase.
#[derive(Clone, Copy, Debug)]
pub struct CapabilityAcquisitionLimits {
    /// Maximum results returned by one provider group.
    pub results_per_provider: usize,
    /// Maximum combined provider source/revision evidence bytes per result.
    pub evidence_bytes: usize,
    /// Maximum provider failure-detail bytes accepted at the boundary.
    pub failure_bytes: usize,
}

impl Default for CapabilityAcquisitionLimits {
    fn default() -> Self {
        Self {
            results_per_provider: 256,
            evidence_bytes: 16 << 10,
            failure_bytes: 4 << 10,
        }
    }
}

/// One validated result retained in canonical capability order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FrozenCapabilityResult {
    /// Typed value/evidence or exact unavailable outcome.
    #[serde(flatten)]
    pub outcome: CapabilityResultOutcome,
}

/// One invocation and its canonical exact-membership result set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FrozenProviderCapture {
    /// Complete site/provider/capability request.
    pub request: AcquisitionRequest,
    /// Results in Gazette capability identity order, never provider return order.
    pub results: BTreeMap<CapabilityId, FrozenCapabilityResult>,
}

/// Complete deterministic capability inputs for one site build.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CapabilityCapture {
    groups: BTreeMap<ProviderRef, FrozenProviderCapture>,
}

impl CapabilityCapture {
    /// Frozen provider groups in exact selection identity order.
    pub fn groups(&self) -> &BTreeMap<ProviderRef, FrozenProviderCapture> {
        &self.groups
    }

    pub(super) fn result(
        &self,
        provider: &ProviderRef,
        capability: &CapabilityId,
    ) -> Option<&FrozenCapabilityResult> {
        self.groups.get(provider)?.results.get(capability)
    }
}

const STATIC_SEARCH_PROVIDER: &str = "static-search";
const ANALYTICS_CLIENT_PROVIDER: &str = "google-analytics-client";
const WEBMCP_CLIENT_PROVIDER: &str = "webmcp-client";

fn native_capability(provider: &ProviderId) -> Option<CapabilityId> {
    match provider.as_str() {
        STATIC_SEARCH_PROVIDER => Some(CoreCapabilityCatalog::publication_search()),
        ANALYTICS_CLIENT_PROVIDER => Some(CoreCapabilityCatalog::publication_analytics()),
        WEBMCP_CLIENT_PROVIDER => Some(CoreCapabilityCatalog::publication_webmcp()),
        _ => None,
    }
}

pub(super) fn gazette_provider_known(provider: &ProviderId) -> bool {
    native_capability(provider).is_some()
}

pub(super) fn gazette_provider_implements(
    provider: &ProviderId,
    capability: &CapabilityId,
) -> bool {
    native_capability(provider).as_ref() == Some(capability)
}

struct DirectLocalCapabilityAcquisition {
    values: BTreeMap<CapabilityId, CapabilityValue>,
    configuration_sha256: String,
}

impl CapabilityAcquisition for DirectLocalCapabilityAcquisition {
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Failure> {
        let results = match request.provider() {
            ProviderRuntimeIdentity::Plugin { .. } => request
                .capabilities()
                .iter()
                .cloned()
                .map(|capability| CapabilityResult {
                    capability,
                    outcome: CapabilityResultOutcome::Unavailable {
                        reason: CapabilityAbsence::Unsupported,
                    },
                })
                .collect(),
            ProviderRuntimeIdentity::Gazette { provider }
                if gazette_provider_known(provider)
                    && request
                        .capabilities()
                        .iter()
                        .all(|capability| gazette_provider_implements(provider, capability)) =>
            {
                request
                    .capabilities()
                    .iter()
                    .cloned()
                    .map(|capability| {
                        let outcome = if let Some(value) = self.values.get(&capability) {
                            CapabilityResultOutcome::Available {
                                value: value.clone(),
                                evidence: ProviderEvidence {
                                    source: format!("gazette:{provider}"),
                                    revision: self.configuration_sha256.clone(),
                                },
                            }
                        } else {
                            CapabilityResultOutcome::Unavailable {
                                reason: CapabilityAbsence::Unconfigured,
                            }
                        };
                        CapabilityResult {
                            capability,
                            outcome,
                        }
                    })
                    .collect()
            }
            ProviderRuntimeIdentity::Gazette { .. } => {
                return Err(Failure {
                    kind: FailureKind::Unavailable,
                    detail: "Gazette provider request is outside the admitted native catalog"
                        .into(),
                });
            }
        };
        Ok(ProviderSnapshot::new(request.provider().clone(), results))
    }
}

/// Capability acquisition failed; no partial capture is installed on the inputs.
#[derive(Debug)]
pub enum CapabilityAcquisitionError {
    /// Selected package/provider metadata failed before invocation.
    Metadata(MetadataError),
    /// A capture is immutable and cannot be silently replaced or reacquired.
    AlreadyCaptured,
    /// The selected site unexpectedly lacks its resolved composition.
    Composition(Key),
    /// An admitted provider runtime identity could not be recovered.
    ProviderIdentity(ProviderRef),
    /// The provider failed before returning a complete snapshot.
    Provider {
        /// Exact selected provider group.
        provider: ProviderRef,
        /// Bounded adapter failure.
        failure: Failure,
    },
    /// An adapter returned unbounded or control-bearing failure detail.
    InvalidFailure(ProviderRef),
    /// Provider snapshot identity or exact result membership was invalid.
    Snapshot {
        /// Exact selected provider group.
        provider: ProviderRef,
        /// Stable boundary invariant.
        reason: &'static str,
    },
}

impl CapabilityAcquisitionError {
    /// Stable diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Metadata(error) => error.code(),
            Self::AlreadyCaptured => "capability_capture_exists",
            Self::Composition(_) => "capability_composition_missing",
            Self::ProviderIdentity(_) => "capability_provider_identity",
            Self::Provider { .. } => "capability_provider_failure",
            Self::InvalidFailure(_) => "capability_provider_failure_invalid",
            Self::Snapshot { .. } => "capability_snapshot_invalid",
        }
    }
}

impl fmt::Display for CapabilityAcquisitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata(error) => error.fmt(formatter),
            Self::AlreadyCaptured => formatter.write_str(
                "capability inputs already contain a frozen capture; reacquisition is refused",
            ),
            Self::Composition(site) => {
                write!(
                    formatter,
                    "site {site} lacks resolved capability composition"
                )
            }
            Self::ProviderIdentity(provider) => {
                write!(
                    formatter,
                    "selected provider {provider:?} lacks admitted runtime identity"
                )
            }
            Self::Provider { provider, failure } => write!(
                formatter,
                "selected provider {provider:?} failed ({:?}): {}",
                failure.kind, failure.detail
            ),
            Self::InvalidFailure(provider) => write!(
                formatter,
                "selected provider {provider:?} returned invalid failure detail"
            ),
            Self::Snapshot { provider, reason } => {
                write!(
                    formatter,
                    "selected provider {provider:?} snapshot: {reason}"
                )
            }
        }
    }
}

impl std::error::Error for CapabilityAcquisitionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata(error) => Some(error),
            _ => None,
        }
    }
}

impl AcquiredInputs {
    /// Acquire the application-owned direct-local provider profile exactly once.
    ///
    /// Gazette dispatch is closed over the native catalog. Admitted plugin providers
    /// return typed `unsupported` snapshots without attempting dynamic loading.
    ///
    /// # Errors
    ///
    /// Refuses invalid package metadata, a repeated capture, an unavailable admitted
    /// runtime, or any incomplete, mismatched or unbounded provider snapshot.
    ///
    /// # Panics
    ///
    /// Only if Gazette's internal fixed search route is invalid; caller input cannot
    /// change that route.
    pub fn acquire_direct_local_capabilities(
        &mut self,
        metadata_limits: toml::Limits,
        limits: CapabilityAcquisitionLimits,
    ) -> Result<(), CapabilityAcquisitionError> {
        let search = self
            .compilation()
            .workspace
            .publication_for(self.site())
            .is_some_and(|publication| publication.machine.search.is_some());
        let configuration_sha256 = self.compilation().source_sha256.to_string();
        let mut values = self.configured_browser_bindings();
        if search {
            let value = CapabilityValue::PublicationSearchV1 {
                index: "/search.json"
                    .parse()
                    .expect("Gazette search artifact route"),
                format: SearchIndexFormat::GazetteDocumentsV1,
            };
            values.insert(value.capability(), value);
        }
        self.acquire_capabilities(
            &DirectLocalCapabilityAcquisition {
                values,
                configuration_sha256,
            },
            metadata_limits,
            limits,
        )
    }

    fn configured_browser_bindings(&self) -> BTreeMap<CapabilityId, CapabilityValue> {
        self.compilation()
            .workspace
            .publication_for(self.site())
            .and_then(|publication| publication.processing.as_ref())
            .into_iter()
            .flat_map(|processing| processing.browser_bindings())
            .map(|value| (value.capability(), value))
            .collect()
    }

    /// Acquire every selected provider group exactly once and freeze complete results.
    ///
    /// Plugin providers are invoked only after their independently selected package has
    /// passed metadata admission, and each request carries that exact package identity.
    ///
    /// # Errors
    ///
    /// Refuses invalid package metadata, a repeated capture, a provider failure, or a
    /// snapshot whose identity, membership, typed value or evidence is not exact.
    pub fn acquire_capabilities(
        &mut self,
        acquisition: &dyn CapabilityAcquisition,
        metadata_limits: toml::Limits,
        limits: CapabilityAcquisitionLimits,
    ) -> Result<(), CapabilityAcquisitionError> {
        if self.capabilities.is_some() {
            return Err(CapabilityAcquisitionError::AlreadyCaptured);
        }
        let browser_bindings = self.configured_browser_bindings();
        let captures = {
            let metadata = self
                .check_metadata(metadata_limits)
                .map_err(CapabilityAcquisitionError::Metadata)?;
            let composition = self
                .compilation()
                .workspace
                .composition_for(self.site())
                .ok_or_else(|| CapabilityAcquisitionError::Composition(self.site().clone()))?;
            let mut groups = BTreeMap::<ProviderRef, BTreeSet<CapabilityId>>::new();
            for (capability, selected) in composition.capabilities() {
                if let CapabilitySelection::Enabled { provider, .. } = selected.value() {
                    groups
                        .entry(provider.clone())
                        .or_default()
                        .insert(capability.clone());
                }
            }
            let mut captures = BTreeMap::new();
            for (selected, capabilities) in groups {
                let runtime = metadata
                    .provider_runtime_identity(&selected)
                    .ok_or_else(|| {
                        CapabilityAcquisitionError::ProviderIdentity(selected.clone())
                    })?;
                let request = AcquisitionRequest::new(
                    self.site().clone(),
                    runtime,
                    capabilities.into_iter().collect(),
                );
                let snapshot = match acquisition.acquire(&request) {
                    Ok(snapshot) => snapshot,
                    Err(failure) => {
                        if !valid_text(&failure.detail, limits.failure_bytes) {
                            return Err(CapabilityAcquisitionError::InvalidFailure(selected));
                        }
                        return Err(CapabilityAcquisitionError::Provider {
                            provider: selected,
                            failure,
                        });
                    }
                };
                let frozen =
                    validate_snapshot(&selected, request, snapshot, &browser_bindings, limits)?;
                captures.insert(selected, frozen);
            }
            captures
        };
        self.capabilities = Some(Arc::new(CapabilityCapture { groups: captures }));
        Ok(())
    }
}

fn validate_snapshot(
    selected: &ProviderRef,
    request: AcquisitionRequest,
    snapshot: gazette_core::publication::capability_runtime::ProviderSnapshot,
    browser_bindings: &BTreeMap<CapabilityId, CapabilityValue>,
    limits: CapabilityAcquisitionLimits,
) -> Result<FrozenProviderCapture, CapabilityAcquisitionError> {
    let invalid = |reason| CapabilityAcquisitionError::Snapshot {
        provider: selected.clone(),
        reason,
    };
    if snapshot.provider() != request.provider() {
        return Err(invalid(
            "runtime identity does not match the admitted request",
        ));
    }
    if snapshot.results().len() != request.capabilities().len()
        || snapshot.results().len() > limits.results_per_provider
    {
        return Err(invalid("result count does not match the complete request"));
    }
    let requested = request.capabilities().iter().collect::<BTreeSet<_>>();
    let mut results = BTreeMap::new();
    for result in snapshot.results() {
        if !requested.contains(&result.capability) {
            return Err(invalid("result contains an unrequested capability"));
        }
        if let CapabilityResultOutcome::Available { value, evidence } = &result.outcome {
            if value.capability() != result.capability {
                return Err(invalid(
                    "typed value contract does not match its capability",
                ));
            }
            if matches!(
                value,
                CapabilityValue::PublicationAnalyticsV1 { .. }
                    | CapabilityValue::PublicationWebMcpV1 { .. }
            ) && browser_bindings.get(&result.capability) != Some(value)
            {
                return Err(invalid(
                    "browser client value differs from the exact configured binding",
                ));
            }
            let evidence_len = evidence.source.len().checked_add(evidence.revision.len());
            if evidence_len.is_none_or(|bytes| bytes > limits.evidence_bytes)
                || !valid_text(&evidence.source, limits.evidence_bytes)
                || !valid_text(&evidence.revision, limits.evidence_bytes)
            {
                return Err(invalid(
                    "provider evidence is blank, control-bearing or oversized",
                ));
            }
        }
        if results
            .insert(
                result.capability.clone(),
                FrozenCapabilityResult {
                    outcome: result.outcome.clone(),
                },
            )
            .is_some()
        {
            return Err(invalid("result contains a duplicate capability"));
        }
    }
    if results.len() != requested.len() {
        return Err(invalid("result omits a requested capability"));
    }
    Ok(FrozenProviderCapture { request, results })
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}
