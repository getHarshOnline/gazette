//! Pure comparison with independently confirmed output and publisher scopes.

use super::{Error, Limits, ReviewedApproval, ReviewedBinding, format};
use gazette_core::{release::GitCredentialSelection, source::Sha256Digest};
use std::collections::BTreeSet;

/// Exact plain approval checked against caller-retained expectations, not a signature.
pub struct VerifiedReviewedApproval {
    approval: ReviewedApproval,
}
impl VerifiedReviewedApproval {
    /// Borrow the entire checked request without changing its scope.
    pub fn approval(&self) -> &ReviewedApproval {
        &self.approval
    }
}

/// Compute the review digest over every field except the digest itself.
/// This prepares reviewable input; it does not record or grant approval.
///
/// # Errors
/// Refuses unsupported JSON shapes, unsafe integers and exhausted bounds.
pub fn reviewed_body_digest(
    approval: &ReviewedApproval,
    limits: Limits,
) -> Result<Sha256Digest, Error> {
    let (_, mut value) =
        format::decode::<ReviewedApproval>(&format::encode(approval, limits)?, limits)?;
    value
        .as_object_mut()
        .ok_or(Error::Shape)?
        .remove("canonical_body_sha256");
    Ok(Sha256Digest::of(&format::canonical(&value, limits)?))
}

/// Decode exact named current expectations without accepting duplicates or positional fields.
///
/// # Errors
/// Refuses malformed, unsupported or over-limit input.
pub fn parse_reviewed_binding(bytes: &[u8], limits: Limits) -> Result<ReviewedBinding, Error> {
    format::decode(bytes, limits).map(|(binding, _)| binding)
}

/// Check plain approval against the digest the operator confirmed and independent site scopes.
/// No key, account login, clock, filesystem, journal or network is consulted.
///
/// # Errors
/// Refuses changed output, incomplete or overlapping publisher scopes, invalid helpers,
/// changed destinations, an active stop, future approval or malformed bounded input.
pub fn verify_reviewed(
    bytes: &[u8],
    binding: &ReviewedBinding,
    limits: Limits,
) -> Result<VerifiedReviewedApproval, Error> {
    let (approval, _) = format::decode::<ReviewedApproval>(bytes, limits)?;
    let _: (ReviewedBinding, _) = format::decode(&format::encode(binding, limits)?, limits)?;
    let digest = reviewed_body_digest(&approval, limits)?;
    if digest != approval.canonical_body_sha256 || digest != binding.canonical_body_sha256 {
        return Err(Error::Digest);
    }
    let text = |value: &str| {
        !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
    };
    if !text(&approval.record_id)
        || !text(&approval.nonce)
        || !text(&approval.coordinator_id)
        || !text(&approval.scanner_revision)
    {
        return Err(Error::Shape);
    }
    if binding.journal.schema_version == 0
        || binding.journal.journal_epoch == 0
        || binding.journal.writer_fence == 0
        || approval.coordinator_id != binding.coordinator_id
        || approval.journal_id != binding.journal.journal_id
        || approval.journal_epoch != binding.journal.journal_epoch
        || approval.writer_fence != binding.journal.writer_fence
        || approval.ordered_targets != binding.ordered_targets
        || approval.ordered_sites != binding.ordered_sites
        || approval.publisher_approvals != binding.publishers
    {
        return Err(Error::Binding);
    }
    if binding.emergency_stop {
        return Err(Error::Inactive);
    }
    if approval.approved_at > binding.observed_at {
        return Err(Error::Time);
    }
    super::check_targets(&approval.ordered_targets, limits)?;
    let count = approval.ordered_targets.len();
    if approval.publisher_approvals.len() > limits.targets
        || approval.ordered_sites.len() > limits.targets
    {
        return Err(Error::Limit);
    }
    if approval.ordered_sites.len() != count
        || approval.ordered_sites.iter().collect::<BTreeSet<_>>().len() != count
        || approval.publisher_approvals.is_empty()
    {
        return Err(Error::Scope);
    }
    let mut publishers = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for publisher in &approval.publisher_approvals {
        if !publishers.insert(&publisher.publisher_id)
            || !text(&publisher.credential_owner)
            || publisher.authorized_target_ordinals.is_empty()
        {
            return Err(Error::Scope);
        }
        // Reuse the native helper-path grammar; these inert paths grant no execution.
        let selection = GitCredentialSelection {
            helper: publisher.credential_helper.clone(),
            allow_credentials: false,
        };
        if !selection.is_valid() {
            return Err(Error::Scope);
        }
        let mut previous = None;
        for ordinal in &publisher.authorized_target_ordinals {
            if usize::try_from(*ordinal).map_or(true, |ordinal| ordinal >= count)
                || previous.is_some_and(|previous| previous >= *ordinal)
                || !covered.insert(*ordinal)
            {
                return Err(Error::Scope);
            }
            previous = Some(*ordinal);
        }
    }
    if covered.len() != count {
        return Err(Error::Scope);
    }
    Ok(VerifiedReviewedApproval { approval })
}
