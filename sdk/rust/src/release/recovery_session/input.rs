//! Narrow recovery wire shapes; ordinary release permissions are not representable.

use crate::{
    release::{ReconcileReleaseOptions, session},
    store,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Exact interrupted custody selection; no authority or generation-write options.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenRequest {
    /// Independently retained journal/anchor identity.
    pub coordinator: store::coordinator::LocalCoordinatorSelection,
    /// Exact retained current generation, not its predicted successor.
    pub expected: store::JournalHeader,
    /// Explicit native lifetime profile; omission requires no durable attempt starts.
    #[serde(default)]
    pub profile: store::coordinator::RecoveryProfile,
    /// Sources, staging, output and backups excluded from native custody roots.
    pub protected_roots: Vec<PathBuf>,
    /// Independent native read/coordination and held-reservation permissions.
    #[serde(default)]
    pub session: store::SessionOptions,
    /// Allows later exact evidence retention or explicitly permitted sealing; not admission.
    #[serde(default)]
    pub allow_anchor_write: bool,
    /// Selected native observer; absent selection still permits inspection and close.
    #[serde(default)]
    pub git: Option<session::GitSelection>,
    /// Independently selected scopes and credentials for observing a reviewed release.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "crate::release::ReviewedSelection")]
    pub reviewed: Option<crate::release::ReviewedSelection>,
}

/// Restricted operations never acquire authority, consume intent or dispatch effects.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    /// Refresh exact native held state.
    Inspect,
    /// Observe and settle the complete original consumed set, or replay its exact receipt.
    Reconcile {
        /// Exact held generation; prior approval bytes are never rewritten.
        expected: store::JournalHeader,
        /// Original consumption identity.
        authorization_id: String,
        /// Consecutive reconciliation number or exact historical replay.
        #[schemars(range(max = u64::MAX))]
        number: u64,
        /// Native reads and evidence writes only.
        #[serde(default)]
        options: ReconcileReleaseOptions,
    },
    /// Verify and release reservations without sealing the anchor clean.
    Close,
    /// Seal exact child-free custody only after every consumed batch is terminal.
    /// This consumes the session but grants no writer generation or release effect.
    Seal {
        /// Independently permit sealing; anchor-write permission remains required.
        #[serde(default)]
        allow_seal: bool,
    },
}

/// Decode with the same bounded duplicate/shape checks as ordinary release transport.
///
/// # Errors
/// Rejects unknown/duplicate fields, positional objects and excessive input.
pub fn parse_open(bytes: &[u8]) -> Result<OpenRequest, &'static str> {
    session::parse(bytes)
}

/// Decode only the narrow recovery operation set, never filtering an ordinary operation.
///
/// # Errors
/// Rejects unsupported effects and malformed, duplicated or excessive input.
pub fn parse_operation(bytes: &[u8]) -> Result<Operation, &'static str> {
    session::parse(bytes)
}
