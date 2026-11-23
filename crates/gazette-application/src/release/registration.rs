//! Register verified pending authorization without consuming it or enabling dispatch.

use super::{
    PreparedAuthorizationError, PreparedAuthorizationOptions, PreparedAuthorizationRequest,
};
use gazette_core::store::{
    Error, ReleaseAuthorizationInput, ReleaseAuthorizationReceipt, ReleaseAuthorizationWriteError,
    ReleaseAuthorizationWriteOptions,
};
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    store::{PreparationReader, ReleaseAuthorizationWriter},
};
use std::fmt;

/// Independent native verification and immutable-registration permissions/limits.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct RegisterAuthorizationOptions {
    /// Existing pure signature and complete native preparation verification.
    pub verification: PreparedAuthorizationOptions,
    /// Explicit local pending-history write permission; never consumption permission.
    pub record: ReleaseAuthorizationWriteOptions,
}

/// Failed verification or registration; any held writer remains the caller's responsibility.
#[derive(Debug)]
pub enum RegisterAuthorizationError {
    /// Signature, independent binding or native preparation join failed before registration.
    Verification(PreparedAuthorizationError),
    /// Held journal mismatch, write refusal or uncertain registration effect.
    Store(ReleaseAuthorizationWriteError),
}
impl RegisterAuthorizationError {
    /// Stable redacted category without keys, signatures, domains or destinations.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Verification(error) => error.code(),
            Self::Store(error) => error.cause.code(),
        }
    }
}
impl From<Error> for RegisterAuthorizationError {
    fn from(error: Error) -> Self {
        Self::Store(error.into())
    }
}
impl fmt::Display for RegisterAuthorizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Verification(error) => {
                write!(f, "{error}; no authorization registration attempted")
            }
            Self::Store(error) => write!(f, "{error}; no authorization consumption or dispatch"),
        }
    }
}
impl std::error::Error for RegisterAuthorizationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verification(e) => Some(e),
            Self::Store(e) => Some(e),
        }
    }
}

/// Verify the complete original body and native preparation set, then register them once.
///
/// Uses the existing held generation; it never advances or rewrites signed coordinates.
/// Complete original authorization bytes, bindings and preparation links survive reopen.
/// Registration is pending custody only: leases, current pre-states, consumption and
/// release effects remain distinct. Exact replay cannot refresh its original authority.
///
/// # Errors
/// Refuses missing permission, wrong held generation, invalid signatures/bindings,
/// changed preparations, conflicting identity/nonce, native bounds or uncertain writes.
pub fn register_prepared_authorization<F, G, S, W>(
    filesystem: &F,
    git: &G,
    store: &S,
    writer: &mut W,
    request: PreparedAuthorizationRequest<'_>,
    options: RegisterAuthorizationOptions,
) -> Result<ReleaseAuthorizationReceipt, RegisterAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
    W: ReleaseAuthorizationWriter,
{
    options.record.validate()?;
    let held = writer.inspect()?.header;
    if held != request.binding.journal || held != request.preparations.expected {
        return Err(Error::JournalMismatch.into());
    }
    let checked = super::authorization::check_prepared_authorization(
        filesystem,
        git,
        store,
        request,
        options.verification,
    )
    .map_err(RegisterAuthorizationError::Verification)?;
    let authorization = std::str::from_utf8(request.authorization)
        .map_err(|_| Error::ReleaseAuthorizationVerification)?;
    let preparation_ids: Vec<_> = request
        .preparations
        .selections
        .iter()
        .map(|selection| selection.preparation_id)
        .collect();
    if writer.inspect()?.header != held {
        return Err(Error::JournalMismatch.into());
    }
    writer
        .record_release_authorization(
            request.preparations.database,
            ReleaseAuthorizationInput {
                authorization,
                binding: &request.binding.clone().into(),
                preparations_sha256: checked.preparations_sha256,
                preparation_ids: &preparation_ids,
            },
            options.record,
        )
        .map_err(RegisterAuthorizationError::Store)
}
