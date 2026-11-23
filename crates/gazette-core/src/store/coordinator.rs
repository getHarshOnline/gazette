//! Independently retained trusted-local control state, never a second release journal.

use super::{
    JournalHeader, LocalCoordinatorAdoption, RestoreAdoptionSource, RestoreAdoptionTarget,
};
use crate::source::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Explicit fixed single-host binding. Labels authenticate no principal or host.
/// The anchor selection must be retained independently from journal copies/restores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalCoordinatorBinding {
    /// Exact independently selected owner coordinator ID, without hashing or normalization.
    pub coordinator_id: String,
    /// Independently selected local scope-binding digest, not principal identity or OS authentication.
    pub principal_scope: Sha256Digest,
    /// Exact canonical native journal directory, fixed at fresh enrollment.
    #[schemars(regex(pattern = "^/"))]
    pub journal_root: PathBuf,
    /// Exact enrolled journal identity.
    pub journal_id: [u8; 32],
    /// Exact enrolled journal epoch; no automatic adoption or epoch advancement.
    #[schemars(range(max = u64::MAX))]
    pub journal_epoch: u64,
}

/// Independently selected local custody location and retained binding, not acquired authority.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalCoordinatorSelection {
    /// Existing private anchor outside journal, backup, input and output roots.
    #[schemars(regex(pattern = "^/"))]
    pub anchor_root: PathBuf,
    /// Independently retained enrollment identity, never inferred from current anchor bytes.
    pub binding: LocalCoordinatorBinding,
}

/// Exact clean database bytes, not publication or external durability certification.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalCoordinatorSeal {
    /// Fixed independently selected local binding.
    pub binding: LocalCoordinatorBinding,
    /// Header observed under both held local reservations.
    pub header: JournalHeader,
    /// Complete standalone database identity, including same-generation writes.
    pub database_sha256: Sha256Digest,
    /// Domain-separated typed SQL state, independent of `SQLite` page layout.
    pub content_sha256: Sha256Digest,
}

/// Expected complete SQL state persisted before a coordinated transaction commits.
/// This is neither proof of commit nor permission to adopt interrupted custody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LocalCoordinatorTransaction {
    /// Exact post-mutation coordinates within the original SQL transaction.
    pub header: JournalHeader,
    /// Domain-separated digest of the schema and every typed persistent row.
    /// Physical page layout and database allocation are not part of this identity.
    pub content_sha256: Sha256Digest,
}

/// Bounded anchor state. In-progress never implies a write happened or did not happen.
/// A crash or failed close cannot silently clear it; there is no repair operation here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalCoordinatorState {
    /// Written clean binding. Inspection does not prove that the previous close,
    /// synchronization or delivery returned successfully.
    Clean {
        /// Exact bound clean state.
        seal: LocalCoordinatorSeal,
        /// Retained only for exact replay immediately after a live adoption.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        adoption: Option<LocalCoordinatorAdoption>,
    },
    /// Persisted before attempting the next journal generation mutation.
    InProgress {
        /// Last completed binding; never overwritten by an uncertain observation.
        previous: LocalCoordinatorSeal,
        /// Exact next generation reserved for this lifetime; no wrapping/reset.
        #[schemars(range(max = u64::MAX))]
        reserved_generation: u64,
        /// Latest expected transaction state, durably recorded before its commit.
        /// Null means generation advancement has not reached that boundary.
        /// A mismatch stays unresolved; it never permits an old-state fallback.
        transaction: Option<LocalCoordinatorTransaction>,
    },
    /// Successor bytes are complete, but the final clean anchor transition was not delivered.
    Adopting {
        /// Last clean frontier, never rewritten during adoption.
        previous: LocalCoordinatorSeal,
        /// Exact detached input retained before successor creation.
        source: RestoreAdoptionSource,
        /// Only permitted fresh successor coordinates.
        next: RestoreAdoptionTarget,
        /// Exact post-advance SQL state, persisted before the successor transaction commits.
        /// Null means no fence-advance attempt has reached that boundary.
        transaction: Option<LocalCoordinatorTransaction>,
    },
}
/// Explicit restricted recovery admission; no profile grants another dispatch.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum RecoveryProfile {
    /// Require global absence of durable release attempt starts.
    #[default]
    Unstarted,
    /// Admit only fully verified starts from synchronous child-free local transactions.
    /// Receive-pack history remains unsupported, regardless of recorded completion.
    LocalTransactions,
    /// Retain observations while prior effect lifetime remains unresolved.
    /// Admits verified receive-pack history, but cannot settle terminally or seal custody.
    ObserveInterrupted,
}
