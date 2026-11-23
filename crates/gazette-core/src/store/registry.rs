//! Rebuildable registry storage, separate from durable release history.

use std::{collections::BTreeMap, fmt};

use serde::{Deserialize, Serialize};

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    publication::{Key, registry::Projection},
    source::Sha256Digest,
};

/// Complete private input to the registry adapter, never publication permission.
/// The adapter verifies coverage and identity; it does not resolve meaning again.
#[derive(Debug)]
pub struct RegistryInput<'a> {
    /// Same resolved structural model consumed by local publication outputs.
    pub model: &'a Projection<'a>,
    /// Expected normalized structural identity, independent from exact source bytes.
    pub semantic_sha256: Sha256Digest,
    /// Exact workspace bytes that selected this projection.
    pub configuration_sha256: Sha256Digest,
    /// Exact full-document byte identities, one for every content node.
    pub documents: &'a BTreeMap<&'a Key, Sha256Digest>,
}

/// Explicit permission to replace only the rebuildable registry-owned rows.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RegistryWriteOptions {
    /// Includes complete-set removals; never permits journal erasure or release.
    pub allow_replace: bool,
}

impl RegistryWriteOptions {
    /// Require the registry effect independently of writer-generation permission.
    ///
    /// # Errors
    /// Refuses an unacknowledged registry replacement before native mutation.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_replace {
            return Err(Error::RegistryPermission);
        }
        Ok(())
    }
}

/// Stored identity of one complete private registry replacement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryHead {
    /// Expected resolved structural model identity; not a full build digest.
    pub semantic_sha256: Sha256Digest,
    /// Exact workspace source identity, separate from normalized meaning.
    pub configuration_sha256: Sha256Digest,
    /// Deterministic native relational-row encoding identity, excluding this header.
    pub rows_sha256: Sha256Digest,
    /// Number of complete registry data rows covered by that encoding.
    #[schemars(range(max = u64::MAX))]
    pub rows: u64,
}

/// Independently read registry state under the same verified journal snapshot.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RegistryInspection {
    /// Provider/schema/integrity and native generation evidence.
    pub journal: JournalInspection,
    /// None only for an initialized store with no registry head or data rows.
    pub registry: Option<RegistryHead>,
}

/// Committed complete-set replacement verified through an independent connection.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegistryReceipt {
    /// Exact acquired journal coordinates; the replacement does not advance them.
    pub journal: JournalHeader,
    /// Verified native row identity. This remains private structural evidence.
    pub registry: RegistryHead,
}

/// Conservative registry-write evidence; failed receipt delivery never implies rollback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub enum RegistryEffect {
    /// No registry SQL mutation was attempted by this operation.
    NotAttempted,
    /// Mutation was attempted, but complete commit/read-back evidence was not returned.
    Unverified,
}

/// A failed registry write, independent of already acquired writer generation.
#[derive(Debug)]
pub struct RegistryWriteError {
    /// Native refusal; no SQL or stored row contents are exposed.
    pub cause: Error,
    /// Whether registry mutation was attempted; inspect before any retry.
    pub effect: RegistryEffect,
}

impl From<Error> for RegistryWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: RegistryEffect::NotAttempted,
        }
    }
}

impl fmt::Display for RegistryWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; registry effect {:?}; preserve and inspect the store before retrying",
            self.cause, self.effect
        )
    }
}

impl std::error::Error for RegistryWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
