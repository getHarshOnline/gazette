//! Provider-neutral capability acquisition; adapters never receive Theme or Tera handles.

use gazette_core::publication::capability_runtime::{AcquisitionRequest, ProviderSnapshot};

/// A complete provider invocation failed before any snapshot could be admitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// The selected provider runtime is not available to this application composition.
    Unavailable,
    /// Transport to the selected provider failed without an admitted response.
    Transport,
    /// Provider execution failed without an admitted response.
    Execution,
    /// Adapter translation could not produce the Gazette-owned typed contract.
    Translation,
}

/// Bounded stable provider failure detail; never a partial result or retry instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// Stable failure class.
    pub kind: FailureKind,
    /// Adapter-owned bounded diagnostic with no secret-bearing response payload.
    pub detail: String,
}

/// Caller-side acquisition port for one complete selected provider group.
pub trait CapabilityAcquisition {
    /// Invoke exactly one admitted runtime identity for the complete request.
    ///
    /// Success may contain typed unavailable outcomes, but must have exact request
    /// membership. The application validates, canonicalizes and freezes it.
    ///
    /// # Errors
    ///
    /// Returns a bounded failure only when the selected runtime cannot produce a
    /// complete snapshot. Typed capability absence belongs in a successful snapshot.
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Failure>;
}
