//! Exact restore adoption evidence; no backup, migration or release authority.

use super::{
    Error, JournalHeader, JournalInspection, LocalCoordinatorBinding, LocalCoordinatorSeal,
    ReadOptions,
};
use crate::source::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Bounded verification and one explicit fresh-successor effect.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RestoreAdoptionOptions {
    /// Permit creation of one fresh successor root and anchor transition.
    pub allow_adopt: bool,
    /// Permit independently anchored state persistence.
    pub allow_anchor_write: bool,
    /// Read and native coordination limits used for all three stores.
    pub read: ReadOptions,
    /// Maximum canonical restore receipt bytes before decoding.
    #[schemars(range(min = 1, max = u64::MAX - 1))]
    pub receipt_bytes: u64,
}

impl Default for RestoreAdoptionOptions {
    fn default() -> Self {
        Self {
            allow_adopt: false,
            allow_anchor_write: false,
            read: ReadOptions::default(),
            receipt_bytes: 16 * 1024,
        }
    }
}

impl RestoreAdoptionOptions {
    /// Validate permission and native bounds before any path is opened.
    ///
    /// # Errors
    ///
    /// Refuses missing adoption or anchor-write permission and invalid read or receipt bounds.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_adopt {
            return Err(Error::RestoreAdoptionPermission);
        }
        if !self.allow_anchor_write {
            return Err(Error::CoordinatorPermission);
        }
        self.read.validate()?;
        if self.receipt_bytes == 0 || self.receipt_bytes == u64::MAX {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Fully verified detached restore, still without live-store authority.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreInspection {
    /// Exact retained restore directory.
    pub directory: PathBuf,
    /// Independently retained exact receipt-byte identity.
    pub receipt_sha256: Sha256Digest,
    /// Original backup manifest pin carried by the restore receipt.
    pub source_manifest_sha256: Sha256Digest,
    /// Exact prepared database bytes.
    pub database_sha256: Sha256Digest,
    /// Native schema, integrity and header observation.
    pub journal: JournalInspection,
    /// Typed complete SQL state independent of page layout.
    pub content_sha256: Sha256Digest,
}

/// Exact immutable source retained through an adoption transition and replay.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreAdoptionSource {
    /// Exact detached restore directory, never the successor root.
    pub restore_root: PathBuf,
    /// Independently retained receipt-byte identity.
    pub receipt_sha256: Sha256Digest,
    /// Original backup manifest identity.
    pub source_manifest_sha256: Sha256Digest,
    /// Exact detached database bytes.
    pub database_sha256: Sha256Digest,
    /// Complete typed SQL state before the successor fence advance.
    pub content_sha256: Sha256Digest,
}

/// Expected fresh successor coordinates; not a capability until native adoption completes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreAdoptionTarget {
    /// Same coordinator/journal lineage at one fresh root.
    pub binding: LocalCoordinatorBinding,
    /// Exact one-fence successor expected after adoption.
    pub header: JournalHeader,
}

/// Retained exact prior/source lineage for one completed adoption.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalCoordinatorAdoption {
    /// Independently retained last clean frontier.
    pub previous: LocalCoordinatorSeal,
    /// Exact detached input verified against that frontier.
    pub source: RestoreAdoptionSource,
}

/// Successful fresh-root adoption. It does not authorize a release or delete either input.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreAdoptionReceipt {
    /// Original clean frontier, left intact.
    pub previous: LocalCoordinatorSeal,
    /// Exact detached source, left intact.
    pub source: RestoreAdoptionSource,
    /// Newly selected and sealed successor frontier.
    pub successor: LocalCoordinatorSeal,
}
