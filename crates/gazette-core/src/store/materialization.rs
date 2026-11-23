//! Exact configured-output history, separate from staging and publication authority.

use std::{fmt, path::PathBuf};

use serde::Serialize;

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    destination::{OutputBinding, OutputObservation},
    publication::Key,
    source::{Inventory, Sha256Digest, TreeIdentity},
};

/// Immutable pre-write intent derived from a retained render and native observation.
/// The store checks linkage and representation; the application owns output policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterializationInput<'a> {
    /// Explicit operation coordinate. Equal replay never grants another filesystem write.
    pub id: Sha256Digest,
    /// Publication whose generated entries this operation owns.
    pub site: &'a Key,
    /// Exact native destination observation, not a pathname-only ownership assertion.
    pub binding: &'a OutputBinding,
    /// Independently inspected preceding completed operation, or an unowned destination.
    pub previous: Option<Sha256Digest>,
    /// Complete non-administrative tree before effects, including user files.
    pub observed: &'a Inventory,
    /// Complete intended tree after effects, including preserved user files.
    pub expected: &'a Inventory,
    /// Exact generated subset. A digest alone cannot authorize later removals.
    pub generated: &'a Inventory,
    /// Selected source and other destination roots that effects must not enter.
    pub protected: &'a [PathBuf],
    /// Bounded private build/composition evidence without source payloads.
    pub evidence: &'a str,
    /// Digest of that exact evidence, separate from artifact identity.
    pub evidence_sha256: Sha256Digest,
}

/// Original intent retained before any configured-output effect starts.
/// Public deserialization cannot manufacture this historical record.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MaterializationIntent {
    /// Original explicit coordinate.
    pub id: Sha256Digest,
    /// Exact publication selection.
    pub site: Key,
    /// Original private native facts; current ownership requires reobservation.
    pub binding: OutputBinding,
    /// Previous completed head selected by this operation.
    pub previous: Option<Sha256Digest>,
    /// Original complete before-state.
    pub observed: Inventory,
    /// Original complete expected state.
    pub expected: Inventory,
    /// Generated ownership subset, distinct from unrelated entries.
    pub generated: Inventory,
    /// Original protected-root selection.
    pub protected: Vec<PathBuf>,
    /// Exact private evidence encoding, not public content.
    pub evidence: String,
    /// Original evidence-byte digest.
    pub evidence_sha256: Sha256Digest,
    /// Original append coordinates, preserved across exact replay.
    pub created: JournalHeader,
}

impl MaterializationIntent {
    /// Borrow the exact payload independently of its historical journal coordinates.
    pub fn as_input(&self) -> MaterializationInput<'_> {
        MaterializationInput {
            id: self.id,
            site: &self.site,
            binding: &self.binding,
            previous: self.previous,
            observed: &self.observed,
            expected: &self.expected,
            generated: &self.generated,
            protected: &self.protected,
            evidence: &self.evidence,
            evidence_sha256: self.evidence_sha256,
        }
    }
}

/// Exact independently read intent plus the complete native result.
/// A supplied value alone grants no ownership; the writer verifies its stored intent.
#[derive(Clone, Copy, Debug)]
pub struct MaterializationCompletionInput<'a> {
    /// Complete original intent read under the reserved writer lifetime.
    pub intent: &'a MaterializationIntent,
    /// Successful full inventory and native binding after effects.
    pub read_back: &'a OutputObservation,
}

impl MaterializationCompletionInput<'_> {
    /// Require full intended bytes and original native destination, not just a tree digest.
    ///
    /// # Errors
    /// Refuses changed native bindings or any missing, extra or changed entry.
    pub fn validate(self) -> Result<(), Error> {
        if self.read_back.binding() != &self.intent.binding
            || self.read_back.inventory() != &self.intent.expected
        {
            return Err(Error::MaterializationVerification);
        }
        Ok(())
    }
}

/// Durable historical read-back, not current filesystem custody or certification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MaterializationCompletion {
    /// Exact original intent whose destination was independently verified.
    pub intent_id: Sha256Digest,
    /// Complete expected/read-back identity, including preserved user files.
    pub output: TreeIdentity,
    /// Complete entry count.
    pub entries: u64,
    /// Complete payload byte count.
    pub bytes: u64,
    /// Original completion coordinates.
    pub recorded: JournalHeader,
}

impl MaterializationCompletion {
    /// Check exact original linkage, complete expected-tree facts and header lineage.
    /// This verifies a historical representation, never current native bytes or custody.
    ///
    /// # Errors
    /// Refuses mismatched output facts, another journal/epoch or reversed append ordering.
    pub fn validate(&self, intent: &MaterializationIntent) -> Result<(), Error> {
        if self.intent_id != intent.id
            || self.output != intent.expected.identity()
            || self.entries != intent.expected.entries().len() as u64
            || self.bytes != intent.expected.total_bytes()
            || self.recorded.schema_version != intent.created.schema_version
            || self.recorded.journal_id != intent.created.journal_id
            || self.recorded.journal_epoch != intent.created.journal_epoch
            || self.recorded.writer_fence < intent.created.writer_fence
        {
            return Err(Error::MaterializationVerification);
        }
        Ok(())
    }
}

/// Missing completion means uncertain effects, never permission to repeat them.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MaterializationRecord {
    /// Exact original pre-write request.
    pub intent: MaterializationIntent,
    /// Full native read-back retained only after successful completion.
    pub completion: Option<MaterializationCompletion>,
}

/// One destination's confirmed ownership and separately unresolved operation.
/// Native equality and journal history do not fence another host or journal.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct MaterializationFrontier {
    /// Canonical private destination selected for this observation.
    pub root: PathBuf,
    /// Last completed generation, or explicit absence; not current byte evidence.
    pub head: Option<MaterializationRecord>,
    /// Original uncompleted intent, which blocks another start at this destination.
    pub active: Option<MaterializationRecord>,
}

/// Write-free inspection under one verified store snapshot.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MaterializationInspection {
    /// Native schema/integrity observation, not anti-rollback proof.
    pub journal: JournalInspection,
    /// Exact destination history; reading does not reconcile effects.
    pub frontier: MaterializationFrontier,
}

/// Independent read-back after appending or exactly replaying a history phase.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MaterializationReceipt {
    /// True only when this call inserted its selected phase, never permission to retry.
    pub inserted: bool,
    /// Current writer coordinates, separate from original append coordinates.
    pub journal: JournalHeader,
    /// Complete history returned by independent storage read-back.
    pub record: MaterializationRecord,
}

/// Permission for output-history writes, independent of filesystem or release effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct MaterializationWriteOptions {
    /// Explicit permission to append/replay and advance the completed ownership head.
    pub allow_record: bool,
}

impl MaterializationWriteOptions {
    /// Require explicit journal effects before any append or head transition.
    ///
    /// # Errors
    /// Refuses absent record permission without attempting SQL mutation.
    pub fn validate(self) -> Result<(), Error> {
        if self.allow_record {
            Ok(())
        } else {
            Err(Error::MaterializationPermission)
        }
    }
}

/// Storage effect boundary, independent of any filesystem effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum MaterializationEffect {
    /// No history insertion attempted; earlier operations may already exist.
    NotAttempted,
    /// SQL effects attempted without complete independently returned read-back.
    Unverified,
}

/// A failed journal operation never implies output rollback or absent history.
#[derive(Debug)]
pub struct MaterializationWriteError {
    /// Exact redacted native/store cause.
    pub cause: Error,
    /// Known journal-effect boundary.
    pub effect: MaterializationEffect,
}

impl From<Error> for MaterializationWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: MaterializationEffect::NotAttempted,
        }
    }
}

impl fmt::Display for MaterializationWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; output-history effect {:?}; preserve destination and journal before reconciliation",
            self.cause, self.effect
        )
    }
}

impl std::error::Error for MaterializationWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
