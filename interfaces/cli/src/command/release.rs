//! Release-verification commands shared by CLI and MCP.

use std::fmt;

use gazette_sdk::release;

use super::{Command, Kernel};

/// Stable transport-neutral release-verification failure.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }

    /// Stable redacted failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Operator-facing diagnostic without input payload disclosure.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Verify exact authorization bytes against independently supplied binding bytes.
pub struct VerifyAuthorization {
    /// Complete original authorization JSON bytes.
    pub authorization_json: Vec<u8>,
    /// Complete independently supplied binding JSON bytes.
    pub binding_json: Vec<u8>,
    /// Native parsing and verification bounds.
    pub limits: release::Limits,
}

impl Command for VerifyAuthorization {
    type Outcome = release::VerificationReport;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let binding = release::parse_binding(&self.binding_json, self.limits)
            .map_err(|error| Failure::new(error.code(), error))?;
        release::verify(&self.authorization_json, &binding, self.limits)
            .map(release::VerifiedAuthorization::into_report)
            .map_err(|error| Failure::new(error.code(), error))
    }
}
