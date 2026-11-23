//! Exact action/scope matching and fixed strict Ed25519 verification.

use super::{Authorization, Binding, Error, KeyState, Limits, RemoteState, Target};
use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::{Signature, VerifyingKey};
use gazette_core::source::Sha256Digest;
use std::collections::BTreeSet;

pub(super) fn bindings(
    body: &Authorization,
    binding: &Binding,
    limits: Limits,
) -> Result<(), Error> {
    let targets = body.ordered_targets.len();
    let publishers = body.publisher_authorizations.len();
    if targets > limits.targets
        || publishers > limits.targets
        || binding.publishers.len() > limits.targets
    {
        return Err(Error::Limit);
    }
    if targets == 0
        || publishers == 0
        || publishers > targets
        || binding.publishers.len() != publishers
    {
        return Err(Error::Scope);
    }
    if !text(&body.record_id)
        || !text(&body.nonce)
        || !text(&body.scanner_revision)
        || !text(&body.coordinator_id)
    {
        return Err(Error::Shape);
    }
    if body.journal_epoch == 0
        || binding.journal.schema_version == 0
        || body.coordinator_id != binding.coordinator_id
        || body.journal_id != binding.journal.journal_id
        || body.journal_epoch != binding.journal.journal_epoch
        || body.writer_fence != binding.journal.writer_fence
        || body.ordered_targets != binding.ordered_targets
    {
        return Err(Error::Binding);
    }
    if binding.emergency_stop {
        return Err(Error::Inactive);
    }
    if body.not_before >= body.expires_at
        || body.approved_at >= body.expires_at
        || body.approved_at > binding.observed_at
        || binding.observed_at < body.not_before
        || binding.observed_at >= body.expires_at
    {
        return Err(Error::Time);
    }
    check_targets(&body.ordered_targets, limits)?;
    let mut keys = BTreeSet::new();
    for publisher in &binding.publishers {
        if !text(&publisher.publisher_id)
            || !text(&publisher.publisher_key_id)
            || !keys.insert((&publisher.publisher_id, &publisher.publisher_key_id))
        {
            return Err(Error::Scope);
        }
    }
    let mut used_keys = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for publisher in &body.publisher_authorizations {
        let admitted = binding
            .publishers
            .iter()
            .find(|key| {
                key.publisher_id == publisher.publisher_id
                    && key.publisher_key_id == publisher.publisher_key_id
            })
            .ok_or(Error::Scope)?;
        if !used_keys.insert((&publisher.publisher_id, &publisher.publisher_key_id))
            || publisher.publisher_key_epoch != admitted.publisher_key_epoch
            || publisher.authority_reference != admitted.authority_reference
            || publisher.authorized_target_ordinals != admitted.authorized_target_ordinals
            || publisher.authorized_target_ordinals.is_empty()
        {
            return Err(Error::Scope);
        }
        if admitted.state != KeyState::Active {
            return Err(Error::Inactive);
        }
        let mut previous = None;
        for ordinal in &publisher.authorized_target_ordinals {
            if usize::try_from(*ordinal).map_or(true, |value| value >= targets)
                || previous.is_some_and(|value| value >= *ordinal)
                || !covered.insert(*ordinal)
            {
                return Err(Error::Scope);
            }
            previous = Some(*ordinal);
        }
    }
    if covered.len() != targets {
        return Err(Error::Scope);
    }
    Ok(())
}

fn text(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && !value.chars().any(char::is_control)
}

pub(super) fn check_targets(targets: &[Target], limits: Limits) -> Result<(), Error> {
    if targets.len() > limits.targets {
        return Err(Error::Limit);
    }
    if targets.is_empty() {
        return Err(Error::Target);
    }
    let mut domains = BTreeSet::new();
    let mut destinations = BTreeSet::new();
    for (ordinal, target) in targets.iter().enumerate() {
        if usize::try_from(target.ordinal).ok() != Some(ordinal)
            || !domains.insert(&target.domain)
            || !destinations.insert((&target.remote, &target.branch))
        {
            return Err(Error::Target);
        }
        check_target(target)?;
    }
    Ok(())
}

fn check_target(target: &Target) -> Result<(), Error> {
    // This exact profile does not accept mutable source refs, force, deletion,
    // patterns, tags or multiple refspecs. Native preparation remains separate.
    let branch = &target.branch;
    let zero = |oid: &gazette_core::source::CommitId| oid.as_str().bytes().all(|byte| byte == b'0');
    if !text(&target.domain)
        || gazette_core::release::validate_git_destination(&target.remote, branch).is_err()
        || zero(&target.local_commit_oid)
        || matches!(&target.expected_remote_oid_or_unborn, RemoteState::Commit { oid } if zero(oid))
        || target.refspec != format!("{}:refs/heads/{branch}", target.local_commit_oid)
    {
        return Err(Error::Target);
    }
    Ok(())
}

pub(super) fn signatures(
    body: &Authorization,
    binding: &Binding,
    digest: Sha256Digest,
) -> Result<(), Error> {
    let mut message = b"WEBSITE_RELEASE_BATCH_AUTHORIZATION_V1\0".to_vec();
    message.extend_from_slice(digest.as_bytes());
    for publisher in &body.publisher_authorizations {
        let admitted = binding
            .publishers
            .iter()
            .find(|key| {
                key.publisher_id == publisher.publisher_id
                    && key.publisher_key_id == publisher.publisher_key_id
            })
            .ok_or(Error::Scope)?;
        let key_bytes = decode::<32>(&admitted.public_key).map_err(|_| Error::Key)?;
        let key = VerifyingKey::from_bytes(&key_bytes).map_err(|_| Error::Key)?;
        // Require the canonical compressed encoding as well as Dalek's strict
        // small-order/scalar checks; never enable its legacy compatibility mode.
        if key.to_edwards().compress().to_bytes() != key_bytes || key.is_weak() {
            return Err(Error::Key);
        }
        let signature = Signature::from_bytes(
            &decode::<64>(&publisher.signature_envelope.signature).map_err(|_| Error::Signature)?,
        );
        key.verify_strict(&message, &signature)
            .map_err(|_| Error::Signature)?;
    }
    Ok(())
}

fn decode<const N: usize>(value: &str) -> Result<[u8; N], ()> {
    if value.len() != N.div_ceil(3) * 4 {
        return Err(());
    }
    let bytes: [u8; N] = STANDARD
        .decode(value)
        .map_err(|_| ())?
        .try_into()
        .map_err(|_| ())?;
    if STANDARD.encode(bytes) != value {
        return Err(());
    }
    Ok(bytes)
}
