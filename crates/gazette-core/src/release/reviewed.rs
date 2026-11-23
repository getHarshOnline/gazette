//! Reviewed-output approval shares delivery history without claiming a signature.

use super::{Authorization, Binding, Target};
use crate::{publication::Key, source::Sha256Digest, store::JournalHeader};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Distinct plain approval format; never accepted by the signed verifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReviewedApprovalSchema {
    /// Exact output and publisher scopes confirmed by the operator.
    WebsiteReviewedReleaseApprovalV1,
}

/// Explicit credential selection for a publisher's exact subset.
/// This records the caller's owner/helper binding, not provider account membership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedPublisher {
    /// Publisher from the independently retained workspace.
    pub publisher_id: Key,
    /// Strictly increasing, nonempty subset of the complete target list.
    pub authorized_target_ordinals: Vec<u32>,
    /// Explicit owner account selected for delivery; never an ambient login.
    pub credential_owner: String,
    /// Absolute helper executable, without arguments or credential secrets.
    pub credential_helper: PathBuf,
}

/// Exact reviewed delivery request. Parsing alone supplies no approval or effects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedApproval {
    /// Distinguishes plain approval from the optional signed format.
    pub schema_id: ReviewedApprovalSchema,
    /// Immutable one-use custody identity.
    pub record_id: String,
    /// Unique nonce within the coordinator's journal.
    pub nonce: String,
    /// Independently selected coordinator.
    pub coordinator_id: String,
    /// Exact journal identity.
    pub journal_id: [u8; 32],
    /// Exact journal epoch.
    pub journal_epoch: u64,
    /// Held writer generation for registration and consumption.
    pub writer_fence: u64,
    /// Complete ordered destinations, commits, outputs and expected remote states.
    pub ordered_targets: Vec<Target>,
    /// Exact configured site at each target ordinal.
    pub ordered_sites: Vec<Key>,
    /// Disjoint publisher scopes covering the whole set once.
    pub publisher_approvals: Vec<ReviewedPublisher>,
    /// Exact retained validation evidence.
    pub validation_evidence_digest: Sha256Digest,
    /// Exact retained disclosure evidence.
    pub disclosure_evidence_digest: Sha256Digest,
    /// Exact retained policy evidence.
    pub policy_evidence_digest: Sha256Digest,
    /// Scanner implementation identity.
    pub scanner_revision: String,
    /// Generic disclosure rules identity.
    pub generic_rule_set_digest: Sha256Digest,
    /// Workspace disclosure policy identity.
    pub website_policy_projection_digest: Sha256Digest,
    /// Explicit approval observation time; no inferred expiry or signing authority.
    pub approved_at: u64,
    /// RFC 8785 digest excluding this field, confirmed independently by the caller.
    pub canonical_body_sha256: Sha256Digest,
}

/// Discriminator for independently retained plain approval expectations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReviewedBindingSchema {
    /// Explicit reviewed-output binding without signing keys.
    #[serde(rename = "gazette.reviewed-release-binding/v1")]
    V1,
}

/// Independently retained expected output, configured scope and current invocation state.
/// Never construct these expectations by trusting the received approval document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewedBinding {
    /// Plain binding format, distinct from signing authority.
    pub schema_id: ReviewedBindingSchema,
    /// Digest explicitly confirmed for the reviewed complete output set.
    pub canonical_body_sha256: Sha256Digest,
    /// Independently selected coordinator.
    pub coordinator_id: String,
    /// Actual held journal generation.
    pub journal: JournalHeader,
    /// Expected exact destinations and prepared output.
    pub ordered_targets: Vec<Target>,
    /// Complete configured coupled set, in target order.
    pub ordered_sites: Vec<Key>,
    /// Publisher and owner/helper mappings retained from the operator's selection.
    pub publishers: Vec<ReviewedPublisher>,
    /// Current caller-observed time.
    pub observed_at: u64,
    /// Stop new effects without erasing existing custody or recovery evidence.
    pub emergency_stop: bool,
}

/// Both supported custody formats. Signed JSON keeps its original shape and bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ReleaseDocument {
    /// Existing publisher-signed format.
    Signed(Box<Authorization>),
    /// Separately identified explicit reviewed-output approval.
    Reviewed(Box<ReviewedApproval>),
}

macro_rules! document_field {
    ($name:ident, $ty:ty) => {
        /// Borrow the same immutable delivery field from either approval format.
        pub fn $name(&self) -> &$ty {
            match self {
                Self::Signed(value) => &value.$name,
                Self::Reviewed(value) => &value.$name,
            }
        }
    };
}
impl ReleaseDocument {
    document_field!(record_id, String);
    document_field!(nonce, String);
    document_field!(coordinator_id, String);
    document_field!(journal_id, [u8; 32]);
    document_field!(journal_epoch, u64);
    document_field!(writer_fence, u64);
    document_field!(ordered_targets, Vec<Target>);
    document_field!(canonical_body_sha256, Sha256Digest);

    /// Borrow signed input only; plain approval never becomes a signed body.
    pub fn as_signed(&self) -> Option<&Authorization> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Reviewed(_) => None,
        }
    }
}

/// Stored current evidence for either supported approval mode.
/// The signed variant has no extra wire wrapper, preserving existing journal payloads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ReleaseBinding {
    /// Existing independently acquired signing authority.
    Signed(Box<Binding>),
    /// Explicit reviewed-output expectations; not cryptographic authority.
    Reviewed(Box<ReviewedBinding>),
}

macro_rules! binding_field {
    ($name:ident, $ty:ty) => {
        /// Borrow the same immutable custody coordinate from either binding mode.
        pub fn $name(&self) -> &$ty {
            match self {
                Self::Signed(value) => &value.$name,
                Self::Reviewed(value) => &value.$name,
            }
        }
    };
}
impl ReleaseBinding {
    binding_field!(coordinator_id, String);
    binding_field!(journal, JournalHeader);
    binding_field!(ordered_targets, Vec<Target>);
    binding_field!(canonical_body_sha256, Sha256Digest);
    binding_field!(observed_at, u64);

    /// Borrow signing authority only when the stored format is signed.
    pub fn as_signed(&self) -> Option<&Binding> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Reviewed(_) => None,
        }
    }

    /// Mutate a caller-owned signed fixture or selection without coercing plain approval.
    pub fn as_signed_mut(&mut self) -> Option<&mut Binding> {
        match self {
            Self::Signed(value) => Some(value),
            Self::Reviewed(_) => None,
        }
    }
}
impl From<Binding> for ReleaseBinding {
    fn from(value: Binding) -> Self {
        Self::Signed(Box::new(value))
    }
}
impl From<ReviewedBinding> for ReleaseBinding {
    fn from(value: ReviewedBinding) -> Self {
        Self::Reviewed(Box::new(value))
    }
}
