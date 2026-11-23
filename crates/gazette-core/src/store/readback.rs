//! Immutable complete ref observations, separate from registry state and release effects.

use super::{Error, JournalHeader, JournalInspection};
use crate::{
    release::{RefComparison, Target, TargetReadback, TargetReadbackRow},
    source::Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Exact application-produced observations. The store checks representation and
/// consistency; only the selected native reader can establish destination evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct RefObservationInput<'a> {
    /// Caller-selected immutable lookup coordinate, never an authorization ID.
    pub id: Sha256Digest,
    /// Entire original selection, including destinations and reviewed pre-states.
    pub targets: &'a [Target],
    /// Entire ordered result, including failures and indeterminate reads.
    pub readbacks: &'a [TargetReadbackRow],
}

impl RefObservationInput<'_> {
    /// Check complete ordinal coverage and consistent comparison without claiming
    /// target admission or authenticating the observations.
    ///
    /// # Errors
    /// Refuses empty, shortened, reordered or internally contradictory evidence.
    pub fn validate(self) -> Result<(), Error> {
        if self.targets.is_empty() || self.targets.len() != self.readbacks.len() {
            return Err(Error::ReadbackVerification);
        }
        for (ordinal, (target, row)) in self.targets.iter().zip(self.readbacks).enumerate() {
            if usize::try_from(target.ordinal).ok() != Some(ordinal)
                || row.ordinal != target.ordinal
            {
                return Err(Error::ReadbackVerification);
            }
            if let TargetReadback::Observed {
                advertisement,
                comparison,
            } = &row.readback
                && *comparison != RefComparison::compare(target, advertisement)
            {
                return Err(Error::ReadbackVerification);
            }
        }
        Ok(())
    }
}

/// Historical evidence. Reading it does not resample destinations, settle a
/// release, establish freshness or prove that this caller caused any change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RefObservation {
    /// Original caller coordinate, immutable across exact append replay.
    pub id: Sha256Digest,
    /// Full original ordered target objects, not only a digest or current registry lookup.
    pub targets: Vec<Target>,
    /// Full original sequential results; not an atomic remote snapshot.
    pub readbacks: Vec<TargetReadbackRow>,
    /// Original writer coordinates; later reads never rewrite these.
    pub created: JournalHeader,
}

impl RefObservation {
    /// Borrow the immutable payload independently from creation coordinates.
    pub fn as_input(&self) -> RefObservationInput<'_> {
        RefObservationInput {
            id: self.id,
            targets: &self.targets,
            readbacks: &self.readbacks,
        }
    }
}

/// Verified historical lookup without creating a row or acquiring a writer.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RefObservationInspection {
    /// Current native journal evidence, not anti-rollback certification.
    pub journal: JournalInspection,
    /// Exact retained observation, or explicit absence.
    pub record: Option<RefObservation>,
}

/// Independent append permission; does not permit provider reads or release effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct RefObservationWriteOptions {
    /// Permit one immutable observation append or exact storage replay.
    pub allow_record: bool,
}

impl RefObservationWriteOptions {
    /// Require explicit local history mutation permission.
    ///
    /// # Errors
    /// Refuses unacknowledged writes before native work.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_record {
            return Err(Error::ReadbackPermission);
        }
        Ok(())
    }
}

/// Independent post-commit read-back, not publisher or destination approval.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct RefObservationReceipt {
    /// Current held writer coordinates.
    pub journal: JournalHeader,
    /// Original complete persisted payload.
    pub record: RefObservation,
    /// False only for exact storage replay; no provider is invoked by replay.
    pub inserted: bool,
}

/// Conservative local SQL boundary if complete record delivery fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefObservationEffect {
    /// No observation insert was attempted.
    NotAttempted,
    /// Insert was attempted without complete returned read-back; inspect before retrying.
    Unverified,
}

/// Record failure; any earlier provider reads or generation advance remain separate effects.
#[derive(Debug)]
pub struct RefObservationWriteError {
    /// Redacted native, conflict or verification cause.
    pub cause: Error,
    /// Known observation-write boundary.
    pub effect: RefObservationEffect,
}

impl From<Error> for RefObservationWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: RefObservationEffect::NotAttempted,
        }
    }
}
impl fmt::Display for RefObservationWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; observation write {:?}; inspect original coordinate before retrying",
            self.cause, self.effect
        )
    }
}
impl std::error::Error for RefObservationWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
