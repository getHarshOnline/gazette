//! Explicit local external-fact availability; no provider is inferred or discovered.

pub use gazette_application::facts::{Error, Limits, acquire, request};
use gazette_core::publication::facts::{AcquisitionRequest, ProviderSnapshot};
use gazette_ports::facts::{ExternalFacts, Unavailable, UnavailableKind};

/// Local composition with no admitted external-fact provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalProvider {
    /// No provider was selected for the binding.
    Unconfigured,
    /// Local Gazette deliberately does not support the selected binding.
    Unsupported,
}

impl ExternalFacts for LocalProvider {
    fn acquire(&self, request: &AcquisitionRequest) -> Result<ProviderSnapshot, Unavailable> {
        let Some(requested) = request.facts().first().cloned() else {
            return Ok(ProviderSnapshot::new(Vec::new()));
        };
        Err(Unavailable {
            requested,
            kind: match self {
                Self::Unconfigured => UnavailableKind::Unconfigured,
                Self::Unsupported => UnavailableKind::Unsupported,
            },
        })
    }
}
