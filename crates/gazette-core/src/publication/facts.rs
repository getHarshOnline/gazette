//! Publication fact selection and provider-neutral acquisition contracts.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::Key;

/// Gazette-owned field selection; identity and authority fields are not delegated here.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Target {
    /// Display name of an already declared principal, not a new identity record.
    PrincipalName {
        /// Exact existing principal key, never inferred from a label or provider record.
        principal: Key,
    },
}

impl Target {
    /// Native principal selected by this publication field.
    pub fn principal(&self) -> &Key {
        match self {
            Self::PrincipalName { principal } => principal,
        }
    }
}

/// A selected source, not proof that its inputs are available, current or admitted.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Source {
    /// Use the existing principal's name field; no duplicate literal is stored here.
    Local,
    /// Require an explicitly named external input binding; no transport is inferred.
    External {
        /// Gazette configuration reference, not a provider record, revision or API address.
        binding: Key,
    },
}

/// One source choice per native field; this metadata never carries a second value.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Existing publication field whose source is selected.
    pub target: Target,
    /// Explicit source choice; omission of the complete selection retains local declarations.
    pub source: Source,
}

/// One external fact requested from a caller-selected provider binding.
///
/// The binding is Gazette configuration, not a provider address or record identity.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RequestedFact {
    /// Exact Gazette publication field to resolve.
    pub target: Target,
    /// Exact Gazette-owned binding selection; adapters decide how to translate it.
    pub binding: Key,
}

/// Complete external-fact request derived from one resolved workspace selection set.
///
/// This is not a provider query DTO, freshness promise or authorization grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct AcquisitionRequest {
    facts: Vec<RequestedFact>,
}

impl AcquisitionRequest {
    /// Project external selectors into a stable complete request.
    ///
    /// Returns `None` for a fully local workspace selection set, so an empty request
    /// cannot be constructed and accidentally sent to a provider.
    pub fn from_selections(selections: &[Selection]) -> Option<Self> {
        let facts = selections
            .iter()
            .filter_map(|selection| match &selection.source {
                Source::External { binding } => Some(RequestedFact {
                    target: selection.target.clone(),
                    binding: binding.clone(),
                }),
                Source::Local => None,
            })
            .collect::<Vec<_>>();
        (!facts.is_empty()).then_some(Self { facts })
    }

    /// Exact requested facts in stable workspace order.
    pub fn facts(&self) -> &[RequestedFact] {
        &self.facts
    }
}

/// Gazette's provider-neutral value projection for one selected publication fact.
///
/// A provider adapter translates its native record into this type. The type neither
/// exposes provider objects nor establishes identity, rights or publication approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Value {
    /// Display name for the exact principal selected by [`Target::PrincipalName`].
    PrincipalName {
        /// Exact acquired display value; no local fallback or key-derived default.
        name: String,
    },
}

/// Opaque provider-owned source coordinates retained without interpreting them.
///
/// Gazette compares neither spelling nor ordering of revisions. A provider adapter
/// must supply exact coordinates from its admitted contract; this type adds no
/// transport, atomic-snapshot or freshness semantics.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderEvidence {
    /// Exact provider-native source reference after caller-side translation.
    pub source: String,
    /// Exact provider-native revision reference, treated as an opaque value.
    pub revision: String,
}

/// One translated provider observation for one exact Gazette request item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFact {
    /// Exact request item satisfied by this observation.
    pub requested: RequestedFact,
    /// Gazette-owned value projection, not a raw provider record.
    pub value: Value,
    /// Opaque provider source coordinates preserved for later build evidence.
    pub evidence: ProviderEvidence,
}

/// Complete provider response before Gazette validates and freezes it.
///
/// This contains no wall clock, mutable remote handle or provider transport object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct ProviderSnapshot {
    facts: Vec<SnapshotFact>,
}

impl ProviderSnapshot {
    /// Construct a provider response for validation by the application boundary.
    pub fn new(facts: Vec<SnapshotFact>) -> Self {
        Self { facts }
    }

    /// Translated observations in provider-returned order.
    pub fn facts(&self) -> &[SnapshotFact] {
        &self.facts
    }
}

/// Frozen Gazette capture joining exact selection intent to a validated snapshot.
///
/// This is retained build input evidence, not a mutable provider session, a compiler
/// overlay or publication admission. Until the provider contract is frozen, the
/// deterministic compiler continues to refuse external selectors rather than consume it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Capture {
    request: AcquisitionRequest,
    snapshot: ProviderSnapshot,
}

impl Capture {
    /// Freeze a request and its already validated provider snapshot.
    pub fn new(request: AcquisitionRequest, snapshot: ProviderSnapshot) -> Self {
        Self { request, snapshot }
    }

    /// Exact Gazette acquisition intent.
    pub fn request(&self) -> &AcquisitionRequest {
        &self.request
    }

    /// Complete translated provider snapshot retained for later build evidence.
    pub fn snapshot(&self) -> &ProviderSnapshot {
        &self.snapshot
    }
}

/// External facts have no admitted resolution path into deterministic compilation.
/// A separate acquisition capture does not imply compiler consumption, stale fallback
/// or publication approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Unavailable {
    /// Exact unresolved publication field.
    pub target: Target,
    /// Exact required configuration binding.
    pub binding: Key,
}

impl Unavailable {
    /// Stable shared refusal class; the typed value retains the exact selected field/binding.
    pub fn code(&self) -> &'static str {
        "publication_facts_unavailable"
    }
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "principal {} name requires external binding {}; deterministic compilation has no admitted frozen capture and no local value is substituted",
            self.target.principal(),
            self.binding
        )
    }
}

impl std::error::Error for Unavailable {}
