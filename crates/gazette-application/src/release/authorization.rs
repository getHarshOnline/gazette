//! Native preparation binding supplements, never replaces, pure signature verification.

use gazette_compiler::release::{self, Binding, VerifiedAuthorization};
use gazette_core::source::Sha256Digest;
use gazette_ports::{
    destination::RootSeparation,
    release::{ReleaseWorktrees, WorktreeReader},
    store::PreparationReader,
};
use serde::Serialize;
use std::fmt;

use super::preparations::inspect_admitted_preparations;
use super::{PreparationBatchError, PreparationBatchOptions, PreparationBatchRequest};

/// Independently selected expected authority and local preparations for one exact body.
#[derive(Clone, Copy)]
pub struct PreparedAuthorizationRequest<'a> {
    /// Received private authorization bytes, not trusted binding inputs.
    pub authorization: &'a [u8],
    /// Independently acquired body, target, key, time and authority expectations.
    pub binding: &'a Binding,
    /// Complete ordered native selection; its header must equal the binding header.
    pub preparations: PreparationBatchRequest<'a>,
}

/// Independent pure-verification and local-inspection bounds.
#[derive(Clone, Copy, Debug, Default, Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PreparedAuthorizationOptions {
    /// Byte, shape and signature-work limits, applied before native access.
    pub authorization: release::Limits,
    /// Per-target native inspection and batch evidence bounds/permissions.
    pub preparations: PreparationBatchOptions,
}

/// Closed outcome of joining verified signatures to exact local preparations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum PreparedAuthorizationStatus {
    /// Signatures and preparations matched; certification remains separate.
    #[serde(rename = "signature-and-local-preparations-verified-not-certified")]
    VerifiedNotCertified,
}

/// Digest-only native-binding report; not a release-ready token or consumption record.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreparedAuthorizationReport {
    status: PreparedAuthorizationStatus,
    /// Pure signature verification against the caller's independent bindings.
    pub verification: release::VerificationReport,
    /// Exact private complete local preparation-batch projection identity.
    pub preparations_sha256: Sha256Digest,
}

/// Failed pure verification or native join; no partially verified capability is returned.
#[derive(Debug)]
pub enum PreparedAuthorizationError {
    /// Signature, scope, canonical body or independently supplied binding failed.
    Authorization(release::Error),
    /// Local preparation/history/check/output inspection failed.
    Preparation(PreparationBatchError),
    /// Native complete-set targets or evidence differ from the signed intent.
    Mismatch,
}
impl PreparedAuthorizationError {
    /// Stable redacted failure category.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Authorization(error) => error.code(),
            Self::Preparation(error) => error.code(),
            Self::Mismatch => "release_native_binding",
        }
    }
}
impl fmt::Display for PreparedAuthorizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; no native-bound authorization report; no release effects",
            self.code()
        )
    }
}
impl std::error::Error for PreparedAuthorizationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Authorization(e) => Some(e),
            Self::Preparation(e) => Some(e),
            Self::Mismatch => None,
        }
    }
}

/// Check signatures and the complete exact local preparation set without release effects.
///
/// Pure verification precedes native access. Native destinations, commits, output
/// identities and original check evidence must then match the entire signed body.
/// Domain/site associations, authority freshness and remote pre-states remain
/// independently supplied. No owner approval, certification, lease, writer lock,
/// consumption, network read or push is acquired by this operation.
///
/// # Errors
/// Refuses invalid signatures, missing/changed preparations or any complete-set,
/// journal, target, evidence or scanner-profile mismatch; never shrinks the batch.
pub fn verify_prepared_authorization<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: PreparedAuthorizationRequest<'_>,
    options: PreparedAuthorizationOptions,
) -> Result<PreparedAuthorizationReport, PreparedAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    let checked = check_prepared_authorization(filesystem, git, store, request, options)?;
    Ok(PreparedAuthorizationReport {
        status: PreparedAuthorizationStatus::VerifiedNotCertified,
        verification: checked.verified.into_report(),
        preparations_sha256: checked.preparations_sha256,
    })
}

pub(super) struct CheckedAuthorization {
    pub(super) verified: VerifiedAuthorization,
    pub(super) preparations_sha256: Sha256Digest,
}

pub(super) fn check_prepared_authorization<F, G, S>(
    filesystem: &F,
    git: &G,
    store: &S,
    request: PreparedAuthorizationRequest<'_>,
    options: PreparedAuthorizationOptions,
) -> Result<CheckedAuthorization, PreparedAuthorizationError>
where
    F: RootSeparation + WorktreeReader,
    G: ReleaseWorktrees,
    S: PreparationReader,
{
    use PreparedAuthorizationError as Error;
    let verified = release::verify(
        request.authorization,
        request.binding,
        options.authorization,
    )
    .map_err(Error::Authorization)?;
    if request.preparations.expected != request.binding.journal
        || request.preparations.selections.len() != request.binding.ordered_targets.len()
    {
        return Err(Error::Mismatch);
    }
    let admitted = inspect_admitted_preparations(
        filesystem,
        git,
        store,
        request.preparations,
        options.preparations,
    )
    .map_err(Error::Preparation)?;
    let batch = admitted.batch();
    if !matches(&verified, batch) {
        return Err(Error::Mismatch);
    }
    let preparations_sha256 =
        super::preparations::digest(batch, options.preparations.evidence_bytes)
            .map_err(Error::Preparation)?;
    Ok(CheckedAuthorization {
        verified,
        preparations_sha256,
    })
}

fn matches(verified: &VerifiedAuthorization, batch: &super::PreparationBatch) -> bool {
    let body = verified.authorization();
    let evidence = batch.evidence();
    body.ordered_targets == batch.targets()
        && body.validation_evidence_digest == evidence.validation_evidence_digest
        && body.disclosure_evidence_digest == evidence.disclosure_evidence_digest
        && body.policy_evidence_digest == evidence.policy_evidence_digest
        && body.scanner_revision == evidence.scanner_revision
        && body.generic_rule_set_digest == evidence.generic_rule_set_digest
        && body.website_policy_projection_digest == evidence.website_policy_projection_digest
}
