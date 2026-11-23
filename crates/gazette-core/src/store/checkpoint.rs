//! Immutable local commit evidence, separate from replaceable registry projections.

use std::fmt;

use serde::Serialize;

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    release::{CommitMetadata, CommitReport},
    source::Sha256Digest,
};

/// Exact verified local commit inputs. The application obtains the commit report
/// from native verification; the store does not authenticate Git objects itself.
#[derive(Clone, Copy, Debug)]
pub struct CheckpointInput<'a> {
    /// Caller-selected opaque checkpoint coordinate; not publisher authorization.
    pub id: Sha256Digest,
    /// Exact absolute UTF-8 repository locator, never a discovered fallback.
    pub repository: &'a str,
    /// Complete recipe, including explicit parent and Git attribution metadata.
    pub metadata: &'a CommitMetadata,
    /// Full native object/tree read-back, not a caller-invented inventory count.
    pub commit: &'a CommitReport,
}

/// Historical local evidence. Reinspection of this record does not prove that
/// its Git objects still exist, that a restored journal is fresh, or that release is approved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CommitCheckpoint {
    /// Original caller coordinate; exact replay may not replace its payload.
    pub id: Sha256Digest,
    /// Retained local repository spelling.
    pub repository: String,
    /// Original complete commit recipe.
    pub metadata: CommitMetadata,
    /// Independently verified local commit and tree evidence at creation.
    pub commit: CommitReport,
    /// Original creation coordinates, never rewritten during a later replay.
    pub created: JournalHeader,
}

/// Append or exact replay permission, distinct from writer-generation acquisition.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct CheckpointWriteOptions {
    /// Permit recording this checkpoint only; no authorization consumption or Git effects.
    pub allow_record: bool,
}

impl CheckpointWriteOptions {
    /// Require checkpoint effects explicitly before any native mutation.
    ///
    /// # Errors
    /// Refuses an unacknowledged checkpoint write.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_record {
            return Err(Error::CheckpointPermission);
        }
        Ok(())
    }
}

/// Verified append/replay result. Current generation and original creation are distinct.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckpointReceipt {
    /// Current acquired writer coordinates.
    pub journal: JournalHeader,
    /// Independently read immutable stored evidence.
    pub checkpoint: CommitCheckpoint,
}

/// Exact lookup under one verified native snapshot; absence is explicit.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct CheckpointInspection {
    /// Current schema, integrity and journal evidence, not anti-rollback certification.
    pub journal: JournalInspection,
    /// Retained record or absence; no record is created by lookup.
    pub checkpoint: Option<CommitCheckpoint>,
}

/// Failed record delivery does not establish that the transaction rolled back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum CheckpointEffect {
    /// No checkpoint SQL write was attempted.
    NotAttempted,
    /// A checkpoint SQL write was attempted without complete returned read-back.
    Unverified,
}

/// Checkpoint effects stay separate from any previously advanced writer generation.
#[derive(Debug)]
pub struct CheckpointWriteError {
    /// Redacted native or record-verification refusal.
    pub cause: Error,
    /// Known attempted-record boundary, never an automatic retry instruction.
    pub effect: CheckpointEffect,
}

impl From<Error> for CheckpointWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: CheckpointEffect::NotAttempted,
        }
    }
}
impl fmt::Display for CheckpointWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; checkpoint effect {:?}; preserve and inspect before retrying",
            self.cause, self.effect
        )
    }
}
impl std::error::Error for CheckpointWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
