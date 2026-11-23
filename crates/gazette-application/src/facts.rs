//! External publication-fact orchestration before deterministic compilation.

use std::{collections::BTreeSet, fmt};

use gazette_compiler::workspace::Workspace;
use gazette_core::publication::facts::{
    AcquisitionRequest, Capture, ProviderSnapshot, RequestedFact, Value,
};
use gazette_ports::facts::{ExternalFacts, Unavailable, UnavailableKind};

/// Independent bounds for one provider-returned snapshot.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Maximum selected external facts.
    pub facts: usize,
    /// Maximum aggregate UTF-8 bytes across acquired values and opaque evidence.
    pub text_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            facts: 1_024,
            text_bytes: 4 * 1024 * 1024,
        }
    }
}

/// External fact acquisition was refused or returned an invalid complete snapshot.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// No provider was selected for this exact request.
    Unconfigured(RequestedFact),
    /// The selected provider cannot supply this exact request.
    Unsupported(RequestedFact),
    /// The provider has no current value for this exact request.
    Missing(RequestedFact),
    /// The returned value violates the selected freshness policy.
    Stale(RequestedFact),
    /// The source owner revoked this value or access to it.
    Revoked(RequestedFact),
    /// The caller is not authorized to acquire this value.
    Unauthorized(RequestedFact),
    /// Provider failure named a fact outside the complete request.
    ForeignFailure,
    /// Provider snapshot membership differs from the complete request.
    SnapshotMembership,
    /// Provider snapshot contains empty/control-bearing values or evidence.
    SnapshotText,
    /// Provider snapshot exceeds the selected complete-set bound.
    Limit(&'static str),
}

impl Error {
    /// Stable application failure class without provider-specific transport detail.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unconfigured(_) => "publication_facts_unconfigured",
            Self::Unsupported(_) => "publication_facts_unsupported",
            Self::Missing(_) => "publication_facts_missing",
            Self::Stale(_) => "publication_facts_stale",
            Self::Revoked(_) => "publication_facts_revoked",
            Self::Unauthorized(_) => "publication_facts_unauthorized",
            Self::ForeignFailure => "publication_facts_foreign_failure",
            Self::SnapshotMembership => "publication_facts_snapshot_membership",
            Self::SnapshotText => "publication_facts_snapshot_text",
            Self::Limit(_) => "publication_facts_limit",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unconfigured(fact) => {
                write!(
                    f,
                    "external fact binding {} is not configured",
                    fact.binding
                )
            }
            Self::Unsupported(fact) => {
                write!(f, "external fact binding {} is unsupported", fact.binding)
            }
            Self::Missing(fact) => {
                write!(
                    f,
                    "external fact binding {} has no current value",
                    fact.binding
                )
            }
            Self::Stale(fact) => write!(f, "external fact binding {} is stale", fact.binding),
            Self::Revoked(fact) => write!(f, "external fact binding {} is revoked", fact.binding),
            Self::Unauthorized(fact) => {
                write!(f, "external fact binding {} is unauthorized", fact.binding)
            }
            Self::ForeignFailure => {
                f.write_str("provider failure does not belong to the complete request")
            }
            Self::SnapshotMembership => {
                f.write_str("provider snapshot does not exactly cover the complete request")
            }
            Self::SnapshotText => {
                f.write_str("provider snapshot contains invalid text or evidence")
            }
            Self::Limit(resource) => write!(f, "provider snapshot exceeds {resource} limit"),
        }
    }
}

impl std::error::Error for Error {}

/// Derive the complete external-fact request from Gazette-owned selectors.
///
/// `None` means the workspace is fully local and no provider call is required.
pub fn request(workspace: &Workspace) -> Option<AcquisitionRequest> {
    AcquisitionRequest::from_selections(workspace.fact_sources())
}

/// Invoke one caller-selected provider and freeze an exact validated snapshot.
///
/// A fully local workspace returns `Ok(None)` without calling the port. External
/// selectors never fall back to local literals. This operation does not apply the
/// captured values to the compiler; the existing deterministic build guard remains.
///
/// # Errors
/// Preserves exact provider availability outcomes and rejects partial, foreign,
/// malformed or over-limit snapshots without a capture.
pub fn acquire(
    provider: &impl ExternalFacts,
    workspace: &Workspace,
    limits: Limits,
) -> Result<Option<Capture>, Error> {
    let Some(request) = request(workspace) else {
        return Ok(None);
    };
    if request.facts().len() > limits.facts {
        return Err(Error::Limit("fact count"));
    }
    let snapshot = provider
        .acquire(&request)
        .map_err(|failure| unavailable(&request, failure))?;
    validate(&request, &snapshot, limits)?;
    Ok(Some(Capture::new(request, snapshot)))
}

fn unavailable(request: &AcquisitionRequest, failure: Unavailable) -> Error {
    if !request.facts().contains(&failure.requested) {
        return Error::ForeignFailure;
    }
    match failure.kind {
        UnavailableKind::Unconfigured => Error::Unconfigured(failure.requested),
        UnavailableKind::Unsupported => Error::Unsupported(failure.requested),
        UnavailableKind::Missing => Error::Missing(failure.requested),
        UnavailableKind::Stale => Error::Stale(failure.requested),
        UnavailableKind::Revoked => Error::Revoked(failure.requested),
        UnavailableKind::Unauthorized => Error::Unauthorized(failure.requested),
    }
}

fn validate(
    request: &AcquisitionRequest,
    snapshot: &ProviderSnapshot,
    limits: Limits,
) -> Result<(), Error> {
    if snapshot.facts().len() != request.facts().len() {
        return Err(Error::SnapshotMembership);
    }
    let expected = request.facts().iter().cloned().collect::<BTreeSet<_>>();
    let actual = snapshot
        .facts()
        .iter()
        .map(|fact| fact.requested.clone())
        .collect::<BTreeSet<_>>();
    if actual.len() != snapshot.facts().len() || actual != expected {
        return Err(Error::SnapshotMembership);
    }
    let mut bytes = 0usize;
    for fact in snapshot.facts() {
        let Value::PrincipalName { name } = &fact.value;
        for value in [name, &fact.evidence.source, &fact.evidence.revision] {
            if value.trim().is_empty() || value.chars().any(char::is_control) {
                return Err(Error::SnapshotText);
            }
            bytes = bytes
                .checked_add(value.len())
                .filter(|&total| total <= limits.text_bytes)
                .ok_or(Error::Limit("text bytes"))?;
        }
    }
    Ok(())
}
