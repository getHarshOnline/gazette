//! Fresh journal origin, not an adopted release authority or restored generation.

use super::{Error, JournalHeader, Observation, OutputLimits, SessionOptions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Permission to initialize one empty, private local coordination directory.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct InitializeOptions {
    /// Permit fresh database/schema creation; never overwrite or migrate existing state.
    pub allow_create: bool,
    /// Local reservation permission and bounded connection/read-back policy.
    pub session: SessionOptions,
    /// Maximum main-database bytes during creation, rounded down to whole native pages.
    /// Rollback journal bytes and filesystem overhead are additional.
    #[schemars(range(min = 1, max = u64::MAX))]
    pub database_bytes: u64,
    /// Bound the exact resolved directory retained in output evidence.
    pub output: OutputLimits,
}

impl Default for InitializeOptions {
    fn default() -> Self {
        Self {
            allow_create: false,
            session: SessionOptions::default(),
            database_bytes: 1024 * 1024,
            output: OutputLimits::default(),
        }
    }
}

impl InitializeOptions {
    /// Check explicit creation/reservation effects and representable limits.
    ///
    /// # Errors
    /// Refuses missing permissions, invalid read bounds or a zero database ceiling.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_create {
            return Err(Error::InitializePermission);
        }
        self.session.validate()?;
        self.output.validate()?;
        if self.database_bytes == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Successful initial transaction and post-close native read-back/flush evidence.
#[derive(Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitializationReceipt {
    /// Selected private store directory; never publish this operational location.
    pub directory: PathBuf,
    /// Exact new database path, not an ambient or automatically selected store.
    pub database: PathBuf,
    /// Coordinates read from the committed database by an independent connection.
    pub header: JournalHeader,
    /// Native metadata from that SQL read-only verification connection.
    pub verifier: Observation,
}
