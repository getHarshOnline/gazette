//! Explicit local generation effects, separate from publisher and release authority.

use super::{Error, SessionOptions};
use std::fmt;

/// Permissions and per-connection bounds for local journal generation acquisition.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct WriterOptions {
    /// Permit one compare-and-increment of the exact expected journal generation.
    /// This does not authorize any other SQL mutation or external effect.
    pub allow_generation_advance: bool,
    /// Local reservation permission and bounded native connection policy.
    pub session: SessionOptions,
}

impl WriterOptions {
    /// Check every effect permission and resource bound before native access.
    ///
    /// # Errors
    /// Refuses missing generation/reservation/coordination permission or invalid limits.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_generation_advance {
            return Err(Error::WriterPermission);
        }
        self.session.validate()
    }
}

/// Conservative generation-write evidence after failed acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, schemars::JsonSchema)]
pub enum GenerationEffect {
    /// This operation never attempted its generation update.
    NotAttempted,
    /// The update was attempted; commit/read-back did not produce successful acquisition.
    /// Inspect retained state before deciding anything; never blindly retry.
    Unverified,
}

/// Failed acquisition never returns a held writer or implies rollback from cancellation.
#[derive(Debug)]
pub struct WriterAcquireError {
    /// Native or policy refusal; SQL and stored row contents remain private.
    pub cause: Error,
    /// Whether this operation reached its generation-write attempt.
    pub generation: GenerationEffect,
}

impl From<Error> for WriterAcquireError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            generation: GenerationEffect::NotAttempted,
        }
    }
}

impl fmt::Display for WriterAcquireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; generation effect {:?}; no writer acquired; preserve and inspect the journal before retrying",
            self.cause, self.generation
        )
    }
}

impl std::error::Error for WriterAcquireError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
