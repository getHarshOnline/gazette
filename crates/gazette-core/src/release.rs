//! Release authorization declarations and independently supplied verification inputs.
//!
//! Deserialization establishes shape, not signature validity, current authority,
//! prepared artifacts, a held writer fence or permission to perform an effect.

use crate::{
    source::{CommitId, Sha256Digest},
    store::JournalHeader,
};
use serde::{Deserialize, Serialize};

mod acquisition;
mod checkout;
mod commit;
mod custody;
mod destination;
mod dispatch;
mod git;
mod observation;
mod readback;
mod reviewed;
mod synchronization;
mod worktree;
pub use acquisition::{
    ParentAcquisitionOptions, ParentAcquisitionReport, ParentAcquisitionRequest,
    RepositorySetupEffect, RepositorySetupError, RepositorySetupLimits,
};
pub use checkout::{
    OutputCheckoutOptions, OutputCheckoutParent, OutputCheckoutReport, OutputCheckoutRequest,
    OutputCheckoutState,
};
pub use commit::{
    CommitEffect, CommitError, CommitMetadata, CommitOptions, CommitReport, CommitSignature,
};
pub use custody::{CustodyEffect, CustodyError, CustodyOptions, CustodyReport, custody_reference};
pub use destination::{DestinationError, PublicationDestination, validate_git_destination};
pub use dispatch::{
    DispatchEffect, DispatchError, DispatchFailure, DispatchObservation, DispatchOptions,
    DispatchProfile, DispatchRequest, DispatchStatus,
};
pub use git::{GitCredentialSelection, GitSelection};
pub use observation::{RefAdvertisement, RemoteReadError, RemoteReadFailure, RemoteReadOptions};
pub use readback::{RefComparison, TargetReadback, TargetReadbackRow};
pub use reviewed::{
    ReleaseBinding, ReleaseDocument, ReviewedApproval, ReviewedApprovalSchema, ReviewedBinding,
    ReviewedBindingSchema, ReviewedPublisher,
};
pub use synchronization::{WorktreeOutputSyncOptions, WorktreeSyncError, WorktreeSyncOptions};
pub use worktree::{
    HistoryLimits, ParentSource, WorktreeEffect, WorktreeError, WorktreeLimits, WorktreeOptions,
    WorktreeReport, validate_worktree_parent,
};

/// Accepted Website authorization identity; no predecessor aliases are accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationSchema {
    /// One exact ordered release batch with publisher-scoped signatures.
    WebsiteReleaseBatchAuthorizationV1,
}

/// Gazette's sole signature transport profile for the initial authorization ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureSchema {
    /// Pure Ed25519 over the record domain and raw canonical-body SHA-256 bytes.
    #[serde(rename = "gazette.ed25519/v1")]
    Ed25519,
}

/// Versioned detached signature, omitted in full from the canonical body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignatureEnvelope {
    /// Exact algorithm/profile discriminator; no algorithm negotiation.
    pub schema_id: SignatureSchema,
    /// Canonical padded standard Base64 of exactly 64 signature bytes.
    pub signature: String,
}

/// Explicit pre-state, never an absent field or a magic zero object ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteState {
    /// The selected destination branch did not exist at observation.
    Unborn,
    /// The branch was observed at this exact commit.
    Commit {
        /// Full supported Git object coordinate, not a moving ref.
        oid: CommitId,
    },
}

/// Exact action coordinates. Matching these values does not verify native state.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// Contiguous zero-based position in the immutable ordered target set.
    #[schemars(range(max = u32::MAX))]
    pub ordinal: u32,
    /// Exact configured publication domain; no identity inference or normalization.
    pub domain: String,
    /// Exact configured remote, treated as data rather than opened by the verifier.
    pub remote: String,
    /// Literal branch name beneath `refs/heads/`.
    pub branch: String,
    /// Non-force exact `<local_commit_oid>:refs/heads/<branch>` action.
    pub refspec: String,
    /// Independently observed pre-state required for a later effect.
    pub expected_remote_oid_or_unborn: RemoteState,
    /// Byte identity of the retained local preparation receipt.
    pub prepared_release_receipt_ref: Sha256Digest,
    /// Already prepared commit, never a commit predicted after approval.
    pub local_commit_oid: CommitId,
    /// Exact reviewed output-tree digest from preparation.
    pub output_digest: Sha256Digest,
}

/// Publisher-specific signature over the complete body, restricted to its subset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublisherAuthorization {
    /// Configured native publisher identity, not inferred from a site domain.
    pub publisher_id: String,
    /// Exact key identity under the publisher's authority.
    pub publisher_key_id: String,
    /// Signed key epoch; rotation cannot substitute another epoch implicitly.
    pub publisher_key_epoch: u64,
    /// Exact publisher authority-reference hash, retained in the signed body.
    pub authority_reference: Sha256Digest,
    /// Strictly increasing nonempty subset of the ordered target ordinals.
    pub authorized_target_ordinals: Vec<u32>,
    /// Detached signature; all publishers sign the same canonical body digest.
    pub signature_envelope: SignatureEnvelope,
}

/// Untrusted authorization body. Only a verifier can produce verification evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    /// Exact accepted Website record identity.
    pub schema_id: AuthorizationSchema,
    /// Stable one-use record identity; uniqueness requires durable consumption.
    pub record_id: String,
    /// Issuer-provided nonce; this declaration alone cannot establish non-replay.
    pub nonce: String,
    /// Designated coordinator, independent from caller or host identity.
    pub coordinator_id: String,
    /// Exact native 256-bit journal identity, serialized as 32 integer bytes.
    pub journal_id: [u8; 32],
    /// Required native journal epoch.
    pub journal_epoch: u64,
    /// Required native writer generation.
    pub writer_fence: u64,
    /// Complete ordered action set; omission never means partial success.
    pub ordered_targets: Vec<Target>,
    /// Exact validation evidence, not a claim that this verifier ran its validator.
    pub validation_evidence_digest: Sha256Digest,
    /// Exact disclosure evidence.
    pub disclosure_evidence_digest: Sha256Digest,
    /// Exact policy evidence.
    pub policy_evidence_digest: Sha256Digest,
    /// Exact scanner revision coordinate.
    pub scanner_revision: String,
    /// Generic scanner rule-set identity.
    pub generic_rule_set_digest: Sha256Digest,
    /// Website-owned policy projection identity.
    pub website_policy_projection_digest: Sha256Digest,
    /// Claimed approval time as nonnegative integral Unix seconds.
    pub approved_at: u64,
    /// Inclusive activation time as nonnegative integral Unix seconds.
    pub not_before: u64,
    /// Exclusive expiry time as nonnegative integral Unix seconds.
    pub expires_at: u64,
    /// RFC 8785 body identity, excluding this field and each signature envelope.
    pub canonical_body_sha256: Sha256Digest,
    /// Disjoint publisher subsets covering the complete target set exactly once.
    pub publisher_authorizations: Vec<PublisherAuthorization>,
}

/// Caller-observed disposition at the independently retained authority frontier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KeyState {
    /// Caller reports that the exact key and authority revision remain active.
    Active,
    /// Key or authority is revoked; no new-effect verification may succeed.
    Revoked,
    /// Key or authority is superseded; an old signature cannot reactivate it.
    Superseded,
}

/// The admitted verification role, not an operator or coordinator role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PublisherRole {
    /// Native publisher authority for the explicitly bound target subset.
    Publisher,
}

/// Independently admitted key and exact scope; never learned from the signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublisherBinding {
    /// Native publisher identity selected by the caller's authority route.
    pub publisher_id: String,
    /// Exact admitted key identity.
    pub publisher_key_id: String,
    /// Exact admitted key epoch.
    pub publisher_key_epoch: u64,
    /// Exact admitted authority-reference hash.
    pub authority_reference: Sha256Digest,
    /// Required native role; no ownership/operator substitution.
    pub role: PublisherRole,
    /// Complete permitted subset for this verification, not inferred from domains.
    pub authorized_target_ordinals: Vec<u32>,
    /// Canonical padded standard Base64 of exactly 32 Ed25519 public-key bytes.
    pub public_key: String,
    /// Current caller-observed key/authority disposition.
    pub state: KeyState,
}

/// Independent expected inputs. Matching stale caller assertions is not freshness.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    /// Expected canonical body digest retained independently of the received body.
    pub canonical_body_sha256: Sha256Digest,
    /// Expected designated coordinator.
    pub coordinator_id: String,
    /// Independently observed native journal; possession does not acquire its lock.
    pub journal: JournalHeader,
    /// Complete expected target objects, including preparations and pre-states.
    pub ordered_targets: Vec<Target>,
    /// Complete admitted publisher key/scope set for this operation.
    pub publishers: Vec<PublisherBinding>,
    /// Immutable evidence coordinate of the caller's authority acquisition.
    pub authority_frontier_sha256: Sha256Digest,
    /// Immutable evidence coordinate of the caller's revocation observation.
    pub revocation_frontier_sha256: Sha256Digest,
    /// Immutable evidence coordinate of the caller's supersession observation.
    pub supersession_frontier_sha256: Sha256Digest,
    /// Caller-observed time; the pure verifier never reads an ambient clock.
    pub observed_at: u64,
    /// An active emergency stop refuses new-effect verification.
    pub emergency_stop: bool,
}
