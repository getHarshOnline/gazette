//! Explicit HTTP acquisition outside deterministic compilation and provider-specific meaning.

use gazette_core::http::{Error, Request, Response};

/// One bounded unauthenticated GET, without redirects, retries or ambient credentials.
pub trait Reader {
    /// Validate request permissions/scope before effects and return only a complete response.
    ///
    /// # Errors
    /// Refuses invalid selection, missing permission, exhausted bounds or native failure.
    fn get(&self, request: Request<'_>) -> Result<Response, Error>;
}
