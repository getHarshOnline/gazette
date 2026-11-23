//! Pure exact-body signature verification, never authorization consumption or release.
//!
//! Public-key admission, time and native target observations belong to the caller.
//! The result binds those assertions; it does not acquire or refresh their authority.

mod format;
mod reviewed;
mod targets;
mod verify;

pub use reviewed::{
    VerifiedReviewedApproval, parse_reviewed_binding, reviewed_body_digest, verify_reviewed,
};
pub use targets::{TargetSet, check_targets, parse_targets};

pub use gazette_core::release::*;
use gazette_core::source::Sha256Digest;
use serde::{Deserialize, Serialize};

/// Logical parsing and cryptographic-work ceilings, not an OS memory/CPU limit.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum bytes in either input and in either canonical projection.
    #[schemars(range(max = usize::MAX))]
    pub bytes: usize,
    /// Maximum retained JSON values per input, including containers.
    #[schemars(range(max = usize::MAX))]
    pub nodes: usize,
    /// Maximum retained nesting, counting the root as depth one.
    #[schemars(range(max = usize::MAX))]
    pub depth: usize,
    /// Maximum targets and publisher signatures; every publisher covers a target.
    #[schemars(range(max = usize::MAX))]
    pub targets: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 1 << 20,
            nodes: 20_000,
            depth: 32,
            targets: 256,
        }
    }
}

/// Redacted verification failure; untrusted field values never enter diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Input, canonical output or cryptographic-work ceiling was exceeded.
    Limit,
    /// Invalid, trailing, duplicate-key or excessively nested JSON.
    Json,
    /// Unsupported discriminator, field set, wire shape or scalar spelling.
    Shape,
    /// A number is not a nonnegative, exactly represented JCS-safe integer.
    Number,
    /// Claimed, recomputed and independently expected body identities differ.
    Digest,
    /// Exact coordinator, journal or target observations differ.
    Binding,
    /// A target is ambiguous, duplicate, forceful or not an exact supported action.
    Target,
    /// Publisher identity, key epoch, authority reference or exact scope disagrees.
    Scope,
    /// Activation, approval or expiry does not permit a new effect at observed time.
    Time,
    /// Emergency stop, revocation or supersession blocks new-effect verification.
    Inactive,
    /// Invalid or unsupported public-key encoding or point.
    Key,
    /// Invalid detached signature encoding, point, scalar or verification result.
    Signature,
}

impl Error {
    /// Stable failure code, without reproducing private input data.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Limit => "release_limit",
            Self::Json => "release_json",
            Self::Shape => "release_shape",
            Self::Number => "release_number",
            Self::Digest => "release_digest",
            Self::Binding => "release_binding",
            Self::Target => "release_target",
            Self::Scope => "release_scope",
            Self::Time => "release_time",
            Self::Inactive => "release_inactive",
            Self::Key => "release_key",
            Self::Signature => "release_signature",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}; no verification evidence returned; inspect the exact inputs and independent bindings",
            self.code()
        )
    }
}

impl std::error::Error for Error {}

/// Closed outcome of one pure authorization-verification call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum VerificationStatus {
    /// Signatures and caller-supplied bindings verified without granting authority.
    #[serde(rename = "signature-and-binding-verified")]
    SignatureAndBindingVerified,
}

/// Digest-only evidence from one pure verification call, not a release capability.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct VerificationReport {
    status: VerificationStatus,
    /// Identity of the received bytes, retaining noncanonical transport differences.
    pub authorization_sha256: Sha256Digest,
    /// Verified RFC 8785 authorization-body identity.
    pub canonical_body_sha256: Sha256Digest,
    /// Canonical identity of all supplied bindings, including authority frontiers.
    pub binding_sha256: Sha256Digest,
    /// Caller-supplied observation time; not a verifier clock observation.
    #[schemars(range(max = u64::MAX))]
    pub observed_at: u64,
    /// Complete selected target count.
    #[schemars(range(max = usize::MAX))]
    pub target_count: usize,
    /// Complete verified publisher signature count.
    #[schemars(range(max = usize::MAX))]
    pub publisher_count: usize,
}

/// Exact checked body and report, with no deserializer or public constructor.
///
/// This proves signatures against supplied keys and assertions only. It does not
/// prove key admission/freshness, native commits, tree equivalence, destination
/// pre-state, evidence approval, writer/lease ownership, consumption or non-replay.
/// Repeat against freshly acquired bindings before a new-effect decision; expired
/// consumed intent must remain independently available to read-only reconciliation.
pub struct VerifiedAuthorization {
    authorization: Authorization,
    canonical_body: Vec<u8>,
    report: VerificationReport,
}

impl VerifiedAuthorization {
    /// Exact decoded authorization checked by this call; not independently editable.
    pub fn authorization(&self) -> &Authorization {
        &self.authorization
    }

    /// Exact canonical bytes hashed and covered by every publisher signature.
    pub fn canonical_body(&self) -> &[u8] {
        &self.canonical_body
    }

    /// Shareable digest-only verification evidence, without keys or destinations.
    pub fn report(&self) -> &VerificationReport {
        &self.report
    }

    /// Consume this verification into its digest-only report, not an effect capability.
    pub fn into_report(self) -> VerificationReport {
        self.report
    }
}

/// Decode independent binding JSON without accepting arrays as structs or losing duplicates.
///
/// # Errors
/// Rejects malformed, unsupported, non-integral, noncanonical-shaped or over-limit inputs.
pub fn parse_binding(bytes: &[u8], limits: Limits) -> Result<Binding, Error> {
    format::decode(bytes, limits).map(|(binding, _)| binding)
}

/// Check an exact authorization against independently supplied current expectations.
///
/// No filesystem, store, clock, network, private-key or authorization-consumption
/// effects occur. The caller must not derive trusted bindings from the received body.
///
/// # Errors
/// Rejects malformed/over-limit input, body or binding mismatch, invalid targets,
/// inactive/wrongly scoped keys, time gates and any non-strict signature.
pub fn verify(
    bytes: &[u8],
    binding: &Binding,
    limits: Limits,
) -> Result<VerifiedAuthorization, Error> {
    let (authorization, value) = format::decode::<Authorization>(bytes, limits)?;
    let binding_bytes = format::encode(binding, limits)?;
    let (_, binding_value) = format::decode::<Binding>(&binding_bytes, limits)?;
    let canonical_body = format::body(value, limits)?;
    let digest = Sha256Digest::of(&canonical_body);
    if digest != authorization.canonical_body_sha256 || digest != binding.canonical_body_sha256 {
        return Err(Error::Digest);
    }
    verify::bindings(&authorization, binding, limits)?;
    verify::signatures(&authorization, binding, digest)?;
    let canonical_binding = format::canonical(&binding_value, limits)?;
    let report = VerificationReport {
        status: VerificationStatus::SignatureAndBindingVerified,
        authorization_sha256: Sha256Digest::of(bytes),
        canonical_body_sha256: digest,
        binding_sha256: Sha256Digest::of(&canonical_binding),
        observed_at: binding.observed_at,
        target_count: authorization.ordered_targets.len(),
        publisher_count: authorization.publisher_authorizations.len(),
    };
    Ok(VerifiedAuthorization {
        authorization,
        canonical_body,
        report,
    })
}
