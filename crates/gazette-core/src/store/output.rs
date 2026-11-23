//! Failed private store output retains its exact location, never silent cleanup.

use super::Error;
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf};

/// Maximum native directory bytes retained in any successful or partial output.
///
/// This bounds the resolved Unix path, not its JSON encoding or the database bytes.
/// Interfaces can reserve lossless locator delivery before requesting creation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OutputLimits {
    /// Positive resolved directory-byte ceiling, including a generated child name.
    #[schemars(range(min = 1, max = 1_048_576))]
    pub directory_bytes: usize,
}

impl Default for OutputLimits {
    fn default() -> Self {
        Self {
            directory_bytes: 16 * 1024,
        }
    }
}

impl OutputLimits {
    /// Validate a bounded, representable output-locator allowance.
    ///
    /// # Errors
    /// Refuses zero or a ceiling above one MiB before native output creation.
    pub fn validate(self) -> Result<(), Error> {
        if !(1..=1024 * 1024).contains(&self.directory_bytes) {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }

    /// Check the exact resolved directory length before creating output.
    ///
    /// # Errors
    /// Refuses an invalid allowance or a native path exceeding that allowance.
    pub fn check_directory_bytes(self, bytes: usize) -> Result<(), Error> {
        self.validate()?;
        if bytes > self.directory_bytes {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Failed initialization, backup or recovery preparation with new output preserved.
#[derive(Debug)]
pub struct OutputError {
    /// `None` means no new output was created. `Some` locates unverified residue,
    /// even if it contains files resembling complete output.
    pub directory: Option<PathBuf>,
    /// Exact failed native/validation boundary.
    pub cause: Error,
}

impl From<Error> for OutputError {
    fn from(cause: Error) -> Self {
        Self {
            directory: None,
            cause,
        }
    }
}

impl fmt::Display for OutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.cause)?;
        match &self.directory {
            Some(path) => write!(
                f,
                "; incomplete or unverified store output retained at {path:?}; do not adopt, overwrite or activate it"
            ),
            None => f.write_str("; no new store output created"),
        }
    }
}

impl std::error::Error for OutputError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
