//! Immutable local staging intent and retention history, not prepared-release approval.

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    release::{CommitMetadata, CommitReport, PublicationDestination},
    source::{Sha256Digest, TreeIdentity},
    stage::Address,
};
use serde::Serialize;
use std::fmt;

/// Exact local staging request derived from retained publication output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparationInput<'a> {
    /// Opaque caller-selected coordinate; changed intent requires a new coordinate.
    pub id: Sha256Digest,
    /// Native-verified canonical parent and exact child.
    pub address: &'a Address,
    /// Explicit configured delivery selection, or local-only absence; not observed remote state.
    pub destination: Option<&'a PublicationDestination>,
    /// Complete output tree identity.
    pub output: TreeIdentity,
    /// Complete output inventory entry count.
    pub entries: u64,
    /// Complete output payload byte count.
    pub bytes: u64,
    /// Bounded private typed acquisition/render evidence, retained once without payloads.
    pub evidence: &'a str,
    /// Digest of the exact evidence bytes; separate from artifact identity.
    pub evidence_sha256: Sha256Digest,
}

/// Historical exact intent, which must precede creation of the named stage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationIntent {
    /// Original coordinate.
    pub id: Sha256Digest,
    /// Original selected native address, never ownership of a later path.
    pub address: Address,
    /// Original delivery selection. Later checks and Git history refer to this intent only.
    pub destination: Option<PublicationDestination>,
    /// Complete expected tree identity.
    pub output: TreeIdentity,
    /// Complete expected inventory count.
    pub entries: u64,
    /// Complete expected payload size.
    pub bytes: u64,
    /// Exact private evidence encoding, not public page data.
    pub evidence: String,
    /// Exact evidence-byte digest.
    pub evidence_sha256: Sha256Digest,
    /// Original creation coordinates, preserved on replay.
    pub created: JournalHeader,
}

impl PreparationIntent {
    /// Borrow the immutable payload independently of its original journal coordinates.
    pub fn as_input(&self) -> PreparationInput<'_> {
        PreparationInput {
            id: self.id,
            address: &self.address,
            destination: self.destination.as_ref(),
            output: self.output,
            entries: self.entries,
            bytes: self.bytes,
            evidence: &self.evidence,
            evidence_sha256: self.evidence_sha256,
        }
    }
}

/// Native retention observation bound to an existing immutable intent.
#[derive(Clone, Copy, Debug)]
pub struct PreparationRetentionInput<'a> {
    /// Independently read original intent; the store requires exact stored equality.
    pub intent: &'a PreparationIntent,
    /// Native read-back tree identity after synchronization.
    pub output: TreeIdentity,
    /// Native inventory entry count.
    pub entries: u64,
    /// Native payload byte count.
    pub bytes: u64,
}

/// Historical synchronization/read-back record; current bytes require separate inspection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationRetention {
    /// Exact intent coordinate; address and evidence remain solely in that intent.
    pub intent_id: Sha256Digest,
    /// Independently verified retained tree.
    pub output: TreeIdentity,
    /// Retained inventory entry count.
    pub entries: u64,
    /// Retained payload byte count.
    pub bytes: u64,
    /// Original record coordinates, preserved on replay.
    pub recorded: JournalHeader,
}

/// Exact check evidence bound to the original retained stage, not certification.
#[derive(Clone, Copy, Debug)]
pub struct PreparationCheckInput<'a> {
    /// Original immutable staging intent.
    pub intent: &'a PreparationIntent,
    /// Exact historical native retention prerequisite.
    pub retention: &'a PreparationRetention,
    /// Bounded private application-owned JSON; the store does not interpret policy.
    pub evidence: &'a str,
    /// Digest of the exact evidence bytes, independent of the staged tree digest.
    pub evidence_sha256: Sha256Digest,
}

/// Immutable historical check evidence, never current policy or release authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationCheck {
    /// Exact preparation whose retained bytes were checked.
    pub intent_id: Sha256Digest,
    /// Original application evidence, preserved byte-for-byte on replay.
    pub evidence: String,
    /// Digest of the original evidence bytes.
    pub evidence_sha256: Sha256Digest,
    /// Original append coordinates, separate from the current writer generation.
    pub recorded: JournalHeader,
}

/// Exact Git recipe recorded after retention and before metadata initialization.
#[derive(Clone, Copy, Debug)]
pub struct PreparationWorktreeInput<'a> {
    /// Original stage intent, including its historical coordinates.
    pub intent: &'a PreparationIntent,
    /// Exact stored retention observation, not an assertion of current bytes.
    pub retention: &'a PreparationRetention,
    /// Exact selected check history, or explicit absence for technical preparation.
    pub check: Option<&'a PreparationCheck>,
    /// Fixed independent-worktree profile's exact commit recipe.
    pub metadata: &'a CommitMetadata,
    /// Exact local acquisition provenance; present exactly when the recipe has a parent.
    pub parent_source: Option<&'a crate::release::ParentSource>,
}

/// Historical native Git preparation request; absence of completion means unknown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationWorktree {
    /// Original exact recipe; invocation resource ceilings are not artifact identity.
    pub metadata: CommitMetadata,
    /// Retained provenance, not a dependency on this path for later native inspection.
    pub parent_source: Option<crate::release::ParentSource>,
    /// Exact selected check evidence reference; absence means technical unchecked intent.
    /// This reference prevents a lost check row from silently becoming unchecked history.
    pub check_sha256: Option<Sha256Digest>,
    /// Original intent coordinates, preserved on replay.
    pub created: JournalHeader,
    /// Immutable verified native result, never current filesystem evidence.
    pub completion: Option<PreparationCommit>,
}

/// Historical complete Git result bound to the original stage and recipe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationCommit {
    /// Complete native result, independently verified by the caller.
    pub report: CommitReport,
    /// Original completion coordinates, preserved on replay.
    pub recorded: JournalHeader,
}

/// Exact original intent and native result submitted for durable completion.
#[derive(Clone, Copy, Debug)]
pub struct PreparationCommitInput<'a> {
    /// Exact stored stage intent.
    pub intent: &'a PreparationIntent,
    /// Exact stored stage retention.
    pub retention: &'a PreparationRetention,
    /// Exact check history preserved by the Git intent, including explicit absence.
    pub check: Option<&'a PreparationCheck>,
    /// Independently read stored Git intent, including any existing completion.
    pub worktree: &'a PreparationWorktree,
    /// Complete native result; the store checks binding, not Git objects.
    pub report: &'a CommitReport,
}

/// Complete local history. Missing retention/completion never proves absent effects.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct PreparationRecord {
    /// Durable pre-write selection.
    pub intent: PreparationIntent,
    /// Optional durable native retention observation.
    pub retention: Option<PreparationRetention>,
    /// Optional check evidence recorded after retention and before the Git recipe.
    pub check: Option<PreparationCheck>,
    /// Optional Git intent, which requires a retained stage.
    pub worktree: Option<PreparationWorktree>,
}

/// Independent read-back following an append or exact replay.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreparationReceipt {
    /// True only when this call inserted its selected immutable phase row.
    /// Exact replay is false; it does not authorize recreating a filesystem effect.
    pub inserted: bool,
    /// Current writer coordinates, distinct from original record coordinates.
    pub journal: JournalHeader,
    /// Complete historical preparation record.
    pub record: PreparationRecord,
}

/// Read-only lookup under one verified journal snapshot.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct PreparationInspection {
    /// Current integrity/schema evidence, not anti-rollback qualification.
    pub journal: JournalInspection,
    /// Exact record or explicit absence; no native stage operation is implied.
    pub record: Option<PreparationRecord>,
}

/// Permission for local preparation history, never release effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct PreparationWriteOptions {
    /// Explicitly allow appending or replaying preparation evidence.
    pub allow_record: bool,
}
impl PreparationWriteOptions {
    /// Require explicit record effects before any native mutation.
    ///
    /// # Errors
    /// Refuses missing permission.
    pub fn validate(self) -> Result<(), Error> {
        if self.allow_record {
            Ok(())
        } else {
            Err(Error::PreparationPermission)
        }
    }
}

/// Preparation SQL effect boundary, independent of writer generation and filesystem effects.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum PreparationEffect {
    /// No record insertion attempted; an older record may still exist.
    NotAttempted,
    /// Insertion attempted without complete returned independent read-back.
    Unverified,
}

/// Failed record operations never imply rollback of history or filesystem state.
#[derive(Debug)]
pub struct PreparationWriteError {
    /// Redacted store failure.
    pub cause: Error,
    /// Known record-attempt boundary.
    pub effect: PreparationEffect,
}
impl From<Error> for PreparationWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: PreparationEffect::NotAttempted,
        }
    }
}
impl fmt::Display for PreparationWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; preparation record effect {:?}; preserve and inspect before retrying",
            self.cause, self.effect
        )
    }
}
impl std::error::Error for PreparationWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
