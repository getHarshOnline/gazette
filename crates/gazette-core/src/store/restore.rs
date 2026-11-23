//! Detached recovery preparation; live-store adoption is a different effect.

use super::{BackupVerifyOptions, Error, Observation, OutputLimits};
use crate::source::Sha256Digest;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Explicit permission and bounds for a new detached recovery copy.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RestoreOptions {
    /// Authorize fresh private output, never replacement or activation of a store.
    pub allow_prepare: bool,
    /// Backup evidence/connection/byte ceilings, also applied to copied read-back.
    pub verification: BackupVerifyOptions,
    /// Bound the resolved fresh directory, including its native generated name.
    pub output: OutputLimits,
}

impl RestoreOptions {
    /// Validate the exact preparation effect before reading or creating output.
    ///
    /// # Errors
    /// Refuses missing preparation/coordination permission or invalid limits.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_prepare {
            return Err(Error::RestorePermission);
        }
        self.verification.validate()?;
        self.output.validate()
    }
}

/// Evidence for a byte-exact recovery copy, not restoration of operational authority.
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestorePreparation {
    /// Engine-native preparation format, not Website migration or release acceptance.
    pub format: String,
    /// Exact independently selected backup manifest identity.
    pub source_manifest_sha256: Sha256Digest,
    /// Fixed copied database filename in the private output directory.
    pub database: String,
    /// Exact copied length, equal to the selected backup's length.
    #[schemars(range(min = 1, max = u64::MAX))]
    pub bytes: u64,
    /// Exact copied bytes, equal to the selected backup's digest.
    pub sha256: Sha256Digest,
    /// Read-only native inspection of the prepared copy, not Website schema adoption.
    pub verifier: Observation,
}

impl RestorePreparation {
    /// Current detached restore receipt format.
    pub const FORMAT: &str = "gazette.sqlite-restore-preparation.v1";

    /// Verify closed restore-receipt meaning before native byte inspection.
    ///
    /// # Errors
    ///
    /// Refuses an unsupported receipt format or incomplete native verification evidence.
    pub fn validate(&self) -> Result<(), Error> {
        if self.format != Self::FORMAT
            || self.database != super::BackupSnapshot::DATABASE_FILE
            || self.bytes == 0
            || self.verifier.provider != "sqlite"
            || self.verifier.provider_version.is_empty()
            || self.verifier.provider_source_id.is_empty()
            || !self.verifier.sql_read_only
        {
            return Err(Error::RestoreAdoptionVerification);
        }
        Ok(())
    }
}

/// Flushed detached output and its exact preparation evidence; no activation grant.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RestoreReceipt {
    /// Private native locator, never publication content or a selected live-store binding.
    pub directory: PathBuf,
    /// Same payload persisted in `restore.json`.
    pub preparation: RestorePreparation,
    /// Exact preparation manifest bytes to retain for subsequent reconciliation.
    pub receipt_sha256: Sha256Digest,
}
