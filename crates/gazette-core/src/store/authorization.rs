//! Durable pending authorization custody; registration never consumes or dispatches.

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    release::{ReleaseBinding, ReleaseDocument},
    source::Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fmt};

/// Exact received authorization plus the independent binding and native preparation join.
/// Storage validates representation and linkage, not signatures or current authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ReleaseAuthorizationInput<'a> {
    /// Exact received UTF-8 JSON, including signatures and original whitespace.
    pub authorization: &'a str,
    /// Independent verification inputs retained without refreshing their frontiers.
    pub binding: &'a ReleaseBinding,
    /// Exact complete native preparation-batch projection verified by the application.
    pub preparations_sha256: Sha256Digest,
    /// Existing completed preparation IDs in the signed target order.
    pub preparation_ids: &'a [Sha256Digest],
}

impl ReleaseAuthorizationInput<'_> {
    /// Validate stored representation and complete coordinate relationships.
    ///
    /// # Errors
    /// Refuses malformed JSON, mismatched journal/targets, duplicate or missing links.
    /// Cryptographic and native preparation verification remain application prerequisites.
    pub fn validate(self) -> Result<ReleaseDocument, Error> {
        let value = crate::json::parse(self.authorization.as_bytes())
            .map_err(|_| Error::ReleaseAuthorizationVerification)?;
        if !value.is_object() {
            return Err(Error::ReleaseAuthorizationVerification);
        }
        let body = ReleaseDocument::deserialize(&value)
            .map_err(|_| Error::ReleaseAuthorizationVerification)?;
        if body.record_id().is_empty()
            || body.nonce().is_empty()
            || body.coordinator_id().is_empty()
            || body.ordered_targets().is_empty()
            || body.ordered_targets().len() != self.preparation_ids.len()
            || self
                .preparation_ids
                .iter()
                .map(Sha256Digest::as_bytes)
                .collect::<BTreeSet<_>>()
                .len()
                != self.preparation_ids.len()
            || body.canonical_body_sha256() != self.binding.canonical_body_sha256()
            || body.coordinator_id() != self.binding.coordinator_id()
            || *body.journal_id() != self.binding.journal().journal_id
            || *body.journal_epoch() != self.binding.journal().journal_epoch
            || *body.writer_fence() != self.binding.journal().writer_fence
            || self.binding.journal().schema_version == 0
            || *body.journal_epoch() == 0
            || *body.writer_fence() == 0
            || body.ordered_targets() != self.binding.ordered_targets()
            || body
                .ordered_targets()
                .iter()
                .enumerate()
                .any(|(index, target)| usize::try_from(target.ordinal).ok() != Some(index))
        {
            return Err(Error::ReleaseAuthorizationVerification);
        }
        match (&body, self.binding) {
            (ReleaseDocument::Signed(_), ReleaseBinding::Signed(_)) => {}
            (ReleaseDocument::Reviewed(body), ReleaseBinding::Reviewed(binding))
                if body.ordered_sites == binding.ordered_sites
                    && body.publisher_approvals == binding.publishers => {}
            _ => return Err(Error::ReleaseAuthorizationVerification),
        }
        Ok(body)
    }
}

/// Immutable registration history, not a statement of current eligibility or consumption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAuthorizationRecord {
    /// Full received authorization bytes; never replaced by only its digest.
    pub authorization: String,
    /// Original independent key, authority, time and target bindings.
    pub binding: ReleaseBinding,
    /// Original complete preparation-join identity.
    pub preparations_sha256: Sha256Digest,
    /// Original ordered links to completed preparations.
    pub preparation_ids: Vec<Sha256Digest>,
    /// Original held writer generation, preserved on read-back and replay.
    pub created: JournalHeader,
}
impl ReleaseAuthorizationRecord {
    /// Borrow the registered payload without reauthorizing it or changing creation coordinates.
    pub fn as_input(&self) -> ReleaseAuthorizationInput<'_> {
        ReleaseAuthorizationInput {
            authorization: &self.authorization,
            binding: &self.binding,
            preparations_sha256: self.preparations_sha256,
            preparation_ids: &self.preparation_ids,
        }
    }
}

/// Read-only pending-authorization history; absence remains explicit.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseAuthorizationInspection {
    /// Current verified native journal coordinates.
    pub journal: JournalInspection,
    /// Original registration, not fresh verification or release permission.
    pub record: Option<ReleaseAuthorizationRecord>,
}

/// Independent local registration permission; no network or consumption effect is included.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseAuthorizationWriteOptions {
    /// Permit immutable registration and exact storage replay.
    pub allow_record: bool,
}
impl ReleaseAuthorizationWriteOptions {
    /// Require explicit local registration permission before native access.
    ///
    /// # Errors
    /// Refuses unacknowledged authorization-history mutation.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_record {
            return Err(Error::ReleaseAuthorizationPermission);
        }
        Ok(())
    }
}

/// Independently read-back registration; this is never a dispatch capability.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseAuthorizationReceipt {
    /// Actual held journal generation.
    pub journal: JournalHeader,
    /// Full original pending authorization and native join evidence.
    pub record: ReleaseAuthorizationRecord,
    /// False for exact storage replay; no consumption or renewed authority is implied.
    pub inserted: bool,
}

/// Conservative registration-write boundary if a complete receipt is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseAuthorizationEffect {
    /// No pending-authorization insert was attempted.
    NotAttempted,
    /// Insert was attempted; inspect original history before any retry.
    Unverified,
}

/// Registration failure, preserving uncertainty without claiming consumption or rollback.
#[derive(Debug)]
pub struct ReleaseAuthorizationWriteError {
    /// Redacted native, consistency or conflict cause.
    pub cause: Error,
    /// Known registration-write boundary.
    pub effect: ReleaseAuthorizationEffect,
}
impl From<Error> for ReleaseAuthorizationWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: ReleaseAuthorizationEffect::NotAttempted,
        }
    }
}
impl fmt::Display for ReleaseAuthorizationWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; authorization registration {:?}; inspect original history before retrying",
            self.cause, self.effect
        )
    }
}
impl std::error::Error for ReleaseAuthorizationWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
