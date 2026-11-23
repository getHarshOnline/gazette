//! Pure provider-owned deployment files; no delivery or account operations.

use gazette_core::{hosting::HostingProvider, publication::routing::PublicBaseUrl, source};

/// Generate the selected provider's complete files from the production address.
pub trait HostFiles {
    /// Return a bounded tree without reading sources, accounts, clocks or networks.
    ///
    /// # Errors
    /// Reject unsupported providers, invalid production addresses and exhausted limits.
    fn generate(
        &self,
        provider: HostingProvider,
        public_base: &PublicBaseUrl,
        limits: source::Limits,
    ) -> Result<source::CapturedSource, source::Error>;
}
