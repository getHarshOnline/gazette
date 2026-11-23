//! Explicit local object retention, distinct from journal or release authority.

use super::CommitReport;
use crate::source::{RevisionLimits, Sha256Digest};
use serde::Serialize;
use std::fmt;

/// One immutable local custody coordinate. It cannot select arbitrary Git refs.
pub fn custody_reference(id: Sha256Digest) -> String {
    format!("refs/gazette/custody/{id}")
}

/// Native retention permission and bounded complete repository synchronization.
#[derive(Clone, Copy, Debug)]
pub struct CustodyOptions {
    /// Allow creation of the exact custody ref; never replacement or deletion.
    pub allow_retain: bool,
    /// Existing complete object verification and metadata ceilings.
    pub limits: RevisionLimits,
    /// Maximum aggregate regular-file bytes in the selected repository to synchronize.
    pub storage_bytes: u64,
}

impl Default for CustodyOptions {
    fn default() -> Self {
        Self {
            allow_retain: false,
            limits: RevisionLimits::default(),
            storage_bytes: 256 * 1024 * 1024,
        }
    }
}

/// Exact local ref/object read-back. A successful retention operation additionally
/// synchronizes storage; inspection alone does not establish prior synchronization.
/// Neither is hardware power-loss qualification, a lease or a prepared-release receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct CustodyReport {
    /// Immutable custody coordinate, mapped by `custody_reference`.
    pub id: Sha256Digest,
    /// Independently reverified existing commit and complete output.
    pub commit: CommitReport,
}

/// Conservative ref effect after a refused or interrupted retention operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CustodyEffect {
    /// No reference creation was attempted; pre-existing state is not ruled out.
    NotAttempted,
    /// A ref creation was attempted; preserve and inspect it before retrying.
    ReferenceMayRemain,
}

/// Redacted failure; an absent report never establishes rollback.
#[derive(Debug)]
pub struct CustodyError {
    /// Static reason, without source bytes, attribution or credentials.
    pub reason: &'static str,
    /// Known local ref mutation boundary; no remote effect is possible.
    pub effect: CustodyEffect,
}

impl CustodyError {
    /// Refuse before attempting reference creation.
    pub fn refused(reason: &'static str) -> Self {
        Self {
            reason,
            effect: CustodyEffect::NotAttempted,
        }
    }
}

impl fmt::Display for CustodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; local custody effect {:?}; preserve and inspect before retrying",
            self.reason, self.effect
        )
    }
}
impl std::error::Error for CustodyError {}
