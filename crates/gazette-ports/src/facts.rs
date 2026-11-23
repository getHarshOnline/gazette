//! External publication facts acquired before deterministic compilation.

use gazette_core::publication::facts::{AcquisitionRequest, ProviderSnapshot, RequestedFact};

/// Provider-side reason that no complete snapshot was returned.
///
/// These are availability outcomes, not permission to substitute local literals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableKind {
    /// No provider was selected for the requested binding.
    Unconfigured,
    /// The selected provider cannot supply this request.
    Unsupported,
    /// The selected provider has no current value for this request.
    Missing,
    /// The selected value fails the caller's accepted freshness policy.
    Stale,
    /// The source owner revoked the selected value or access to it.
    Revoked,
    /// The caller is not authorized to acquire the selected value.
    Unauthorized,
}

/// Exact failed request item and its explicit provider availability outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unavailable {
    /// Selected request item; the application verifies membership in the complete request.
    pub requested: RequestedFact,
    /// Explicit reason; no variant authorizes fallback or retry.
    pub kind: UnavailableKind,
}

/// Caller-side acquisition port for a complete external-fact request.
pub trait ExternalFacts {
    /// Acquire and translate one complete provider snapshot.
    ///
    /// The adapter owns provider DTO and transport translation. Success must cover the
    /// complete request; the application validates exact membership before freezing it.
    ///
    /// # Errors
    /// Returns one exact explicit unavailable outcome without a partial snapshot.
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable>;
}
