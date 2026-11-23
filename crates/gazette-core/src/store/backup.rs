//! Fresh backup effects and evidence, independent of Website adoption or restore authority.

use super::{Error, Observation, OutputLimits, ReadOptions};
use crate::source::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Explicit fresh-output permission and bounded native backup work.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BackupOptions {
    /// Authorize a new private backup directory, never replacement of existing data.
    pub allow_create: bool,
    /// Existing-source connection effects and per-connection work limits.
    pub read: ReadOptions,
    /// Maximum logical database bytes and final backup file bytes.
    #[schemars(range(min = 1, max = u64::MAX))]
    pub database_bytes: u64,
    /// Positive number of pages copied per native step.
    #[schemars(range(min = 1, max = i32::MAX))]
    pub pages_per_step: i32,
    /// Maximum native copy calls; busy/locked results fail without blind retry.
    #[schemars(range(min = 1, max = u32::MAX))]
    pub steps: u32,
    /// Bound the resolved fresh directory, including its native generated name.
    pub output: OutputLimits,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            allow_create: false,
            read: ReadOptions::default(),
            database_bytes: 256 * 1024 * 1024,
            pages_per_step: 32,
            steps: 8192,
            output: OutputLimits::default(),
        }
    }
}

impl BackupOptions {
    /// Validate effect permission and representable positive work ceilings.
    ///
    /// # Errors
    /// Refuses missing creation/coordination permission or invalid bounds.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_create {
            return Err(Error::BackupPermission);
        }
        self.read.validate()?;
        self.output.validate()?;
        if self.database_bytes == 0 || self.pages_per_step <= 0 || self.steps == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Portable evidence for one copied database, not authority to restore it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupSnapshot {
    /// Engine-native backup evidence format, not a Website/release wire ABI.
    pub format: String,
    /// Exact child filename within the separately located backup directory.
    pub database: String,
    /// Observed source metadata from the same held read transaction as the copy.
    pub source: Observation,
    /// Final copied database byte count.
    #[schemars(range(min = 1, max = u64::MAX))]
    pub bytes: u64,
    /// Exact copied bytes, not deterministic publication artifact identity.
    pub sha256: Sha256Digest,
}

impl BackupSnapshot {
    /// Current Engine-native evidence format; not a Website authorization ABI.
    pub const FORMAT: &str = "gazette.sqlite-backup.v1";
    /// Fixed database filename; manifest input cannot select a different path.
    pub const DATABASE_FILE: &str = "store.sqlite";

    /// Check supported manifest meaning, separately from native byte verification.
    ///
    /// # Errors
    /// Rejects an unknown format, path selection or inconsistent source observation.
    pub fn validate(&self) -> Result<(), Error> {
        if self.format != Self::FORMAT
            || self.database != Self::DATABASE_FILE
            || self.bytes == 0
            || self.source.provider != "sqlite"
            || self.source.provider_version.is_empty()
            || self.source.provider_source_id.is_empty()
            || !self.source.sql_read_only
        {
            return Err(Error::BackupManifest);
        }
        Ok(())
    }
}

/// Successfully flushed backup location plus its portable evidence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupReceipt {
    /// Private native output location; never publish this operational receipt.
    pub directory: PathBuf,
    /// The same semantic payload written to `backup.json` in that directory.
    pub snapshot: BackupSnapshot,
    /// Exact manifest bytes to retain independently for later backup verification.
    pub manifest_sha256: Sha256Digest,
}

/// Bounded retained-backup inspection, with no creation or restore permission.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct BackupVerifyOptions {
    /// Existing-store connection policy and database/SQL/VM work ceilings.
    pub read: ReadOptions,
    /// Maximum manifest bytes before decoding; allocation overhead is additional.
    #[schemars(range(min = 1, max = u64::MAX - 1))]
    pub manifest_bytes: u64,
}

impl Default for BackupVerifyOptions {
    fn default() -> Self {
        Self {
            read: ReadOptions::default(),
            manifest_bytes: 16 * 1024,
        }
    }
}

impl BackupVerifyOptions {
    /// Validate explicit connection policy and a bounded manifest read.
    ///
    /// # Errors
    /// Refuses missing coordination permission or an invalid byte ceiling.
    pub fn validate(self) -> Result<(), Error> {
        self.read.validate()?;
        if self.manifest_bytes == 0 || self.manifest_bytes == u64::MAX {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Native read-back evidence at inspection time, not durability or restore authority.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackupInspection {
    /// Declared original observation, checked against copied bytes/header markers.
    /// Historical provider provenance is not authenticated by a matching hash.
    pub snapshot: BackupSnapshot,
    /// Observed manifest identity, equal to the caller's independently retained pin.
    pub manifest_sha256: Sha256Digest,
    /// Current verifier/provider observation, distinct from historical source metadata.
    pub verifier: Observation,
}
