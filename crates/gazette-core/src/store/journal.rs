//! Local journal coordinates and observation, never a reusable writer capability.

use super::Observation;
use crate::source::Sha256Digest;
use serde::{Deserialize, Serialize};

/// Persisted local journal coordinates. Possessing them grants no authority.
///
/// These Engine-native storage values are not a release wire ABI. A copied or
/// restored database copies these values too; they cannot by themselves prove
/// freshness, prevent rollback or fence another host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalHeader {
    /// Engine-owned storage version, independent of provider and release ABI versions.
    #[schemars(range(max = u32::MAX))]
    pub schema_version: u32,
    /// Random 256-bit journal identity created once, not a publication artifact digest.
    pub journal_id: [u8; 32],
    /// Journal epoch; advancing/adopting one requires a separate recovery protocol.
    #[schemars(range(max = u64::MAX))]
    pub journal_epoch: u64,
    /// Persisted writer generation; observing it acquires no release fence.
    #[schemars(range(max = u64::MAX))]
    pub writer_fence: u64,
}

/// One bounded read snapshot whose exact supported storage schema and integrity were checked.
///
/// This is point-in-time evidence, not Website adoption, restored-history freshness,
/// a held writer fence or release authority. An independently retained expected
/// header can detect a mismatch, but matching a stale expectation is not anti-rollback.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct JournalInspection {
    /// Native journal coordinates, independent of provider/build identity.
    pub header: JournalHeader,
    /// Current provider and native database metadata from the same read snapshot.
    pub verifier: Observation,
    /// Exact Engine-owned DDL bytes used to check the supported schema.
    /// Not a database-content hash, migration receipt or publication artifact identity.
    pub schema_sha256: Sha256Digest,
}
