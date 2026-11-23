//! Immutable dispatch coordinates and observations; replay never grants another effect.

use super::{
    Error, JournalHeader, JournalInspection, ReleaseAuthorizationRecord, ReleaseConsumption,
};
use crate::{
    process::Cleanup,
    release::{
        DispatchEffect, DispatchError, DispatchObservation, DispatchProfile, DispatchStatus,
        RefComparison, ReleaseBinding, Target, TargetReadback,
    },
};
use serde::{Deserialize, Serialize};

/// Exact position in one consumed target's append-only attempt history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAttemptKey {
    /// Original consumed authorization, not a new authorization identity.
    pub authorization_id: String,
    /// Original target ordinal.
    pub ordinal: u32,
    /// One-based, consecutive number within that target.
    pub number: u64,
}

/// Persisted before invoking the selected native dispatcher.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAttemptStart {
    /// Exact selected effect mechanism, not a retrospective claim inferred from its result.
    pub profile: DispatchProfile,
    /// Immutable target and attempt selection.
    pub key: ReleaseAttemptKey,
    /// Independently acquired current authority evidence, not cached consumption authority.
    pub binding: ReleaseBinding,
    /// Fresh native observation of the exact reviewed pre-state.
    pub prestate: TargetReadback,
    /// Actual original held journal generation; no implicit recovery adoption.
    pub created: JournalHeader,
}

impl ReleaseAttemptStart {
    /// Validate exact intent and historical representation, not signatures or native custody.
    ///
    /// # Errors
    /// Refuses changed targets, stale coordinates, backward time or indeterminate pre-state.
    pub fn validate(
        &self,
        registration: &ReleaseAuthorizationRecord,
        consumed: &ReleaseConsumption,
    ) -> Result<(), Error> {
        consumed.validate(registration)?;
        super::ReleaseAuthorizationInput {
            authorization: &registration.authorization,
            binding: &self.binding,
            preparations_sha256: consumed.preparations_sha256,
            preparation_ids: &registration.preparation_ids,
        }
        .validate()?;
        if self.key.authorization_id != consumed.authorization_id
            || self.key.number == 0
            || self.created != consumed.created
            || *self.binding.journal() != self.created
            || self.binding.observed_at() < consumed.binding.observed_at()
        {
            return Err(Error::ReleaseAttemptVerification);
        }
        let target = self.target(consumed)?;
        validate_readback(target, &self.prestate)?;
        if !is_prestate(&self.prestate) {
            return Err(Error::ReleasePrestate);
        }
        Ok(())
    }

    /// Resolve only the original consumed target; mutable workspace configuration is irrelevant.
    ///
    /// # Errors
    /// Refuses an unknown authorization or ordinal.
    pub fn target<'a>(&self, consumed: &'a ReleaseConsumption) -> Result<&'a Target, Error> {
        if self.key.authorization_id != consumed.authorization_id {
            return Err(Error::ReleaseAttemptVerification);
        }
        consumed
            .targets
            .iter()
            .find(|row| row.target.ordinal == self.key.ordinal)
            .map(|row| &row.target)
            .ok_or(Error::ReleaseAttemptVerification)
    }

    /// Validate a new append after the latest target attempt. Exact replay is handled separately.
    /// Missing or ambiguous earlier results require explicit reconciliation, not automatic retry.
    ///
    /// # Errors
    /// Refuses gaps, backward time, observed success or unresolved prior transport effects.
    pub fn follows(&self, previous: Option<&ReleaseAttempt>) -> Result<(), Error> {
        let Some(previous) = previous else {
            return if self.key.number == 1 {
                Ok(())
            } else {
                Err(Error::ReleaseAttemptConflict)
            };
        };
        if previous.start.key.authorization_id != self.key.authorization_id
            || previous.start.profile != self.profile
            || previous.start.key.ordinal != self.key.ordinal
            || previous.start.key.number.checked_add(1) != Some(self.key.number)
            || self.binding.observed_at() < previous.start.binding.observed_at()
        {
            return Err(Error::ReleaseAttemptConflict);
        }
        if !previous
            .result
            .as_ref()
            .is_some_and(ReleaseAttemptResult::permits_retry)
        {
            return Err(Error::ReleaseReconciliationRequired);
        }
        Ok(())
    }
}

/// Native transport return and independent destination read-back have distinct meanings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAttemptResult {
    /// Exact previously persisted start.
    pub key: ReleaseAttemptKey,
    /// Native transport report or conservative failure, never effect proof.
    pub transport: Result<DispatchObservation, DispatchError>,
    /// Independent subsequent observation, including explicit unavailability.
    pub readback: TargetReadback,
    /// Same held generation; later-custody reconciliation is a separate operation.
    pub created: JournalHeader,
}

impl ReleaseAttemptResult {
    /// Validate linkage and comparison representation without promoting provider evidence.
    ///
    /// # Errors
    /// Refuses altered coordinates or invented comparison labels.
    pub fn validate(
        &self,
        start: &ReleaseAttemptStart,
        consumed: &ReleaseConsumption,
    ) -> Result<(), Error> {
        if self.key != start.key || self.created != start.created {
            return Err(Error::ReleaseAttemptVerification);
        }
        let valid_transport = match (start.profile, self.transport) {
            (DispatchProfile::ReceivePack, Ok(observed)) => observed.cleanup == Cleanup::Reaped,
            (DispatchProfile::LocalTransaction, Ok(observed)) => {
                observed.cleanup == Cleanup::NotStarted && observed.exit_code.is_none()
            }
            (DispatchProfile::LocalTransaction, Err(error)) => error.cleanup == Cleanup::NotStarted,
            (DispatchProfile::ReceivePack, Err(_)) => true,
        };
        if !valid_transport {
            return Err(Error::ReleaseAttemptVerification);
        }
        validate_readback(start.target(consumed)?, &self.readback)
    }

    /// Conservative same-holder retry gate. Unknown effects and any reported/observed success
    /// require reconciliation; a completed rejection (or pre-launch refusal) plus exact old state
    /// may retry. Native validation separately binds the cleanup class to the original profile.
    pub fn permits_retry(&self) -> bool {
        is_prestate(&self.readback)
            && match self.transport {
                Ok(observed) => {
                    observed.status == DispatchStatus::Rejected
                        && matches!(observed.cleanup, Cleanup::Reaped | Cleanup::NotStarted)
                }
                Err(error) => {
                    error.effect == DispatchEffect::NotStarted
                        && error.cleanup == Cleanup::NotStarted
                }
            }
    }
}

fn is_prestate(readback: &TargetReadback) -> bool {
    matches!(
        readback,
        TargetReadback::Observed {
            comparison: RefComparison::Prestate,
            ..
        }
    )
}

fn validate_readback(target: &Target, readback: &TargetReadback) -> Result<(), Error> {
    if let TargetReadback::Observed {
        advertisement,
        comparison,
    } = readback
        && *comparison != RefComparison::compare(target, advertisement)
    {
        return Err(Error::ReleaseAttemptVerification);
    }
    Ok(())
}

/// A missing result is valid crash history, never success or permission to redispatch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAttempt {
    /// Durable intent to invoke the exact target.
    pub start: ReleaseAttemptStart,
    /// Exact retained transport/read-back result, if delivery completed.
    pub result: Option<ReleaseAttemptResult>,
}

/// Complete ordered attempt history for one immutable consumed authorization.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseAttemptInspection {
    /// Current native snapshot, not reactivated release authority.
    pub journal: JournalInspection,
    /// Original consumed intent or explicit absence.
    pub consumption: Option<ReleaseConsumption>,
    /// Ordered by target ordinal then attempt number; incomplete starts are retained.
    pub attempts: Vec<ReleaseAttempt>,
}

/// Explicit permission to append attempt evidence, independent from dispatch permission.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseAttemptOptions {
    /// Permit bounded local journal appends; never permits provider effects by itself.
    pub allow_record: bool,
}
impl ReleaseAttemptOptions {
    /// Check local evidence permission before native writes.
    ///
    /// # Errors
    /// Refuses absent acknowledgement.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_record {
            return Err(Error::ReleaseAttemptPermission);
        }
        Ok(())
    }
}

/// Independently read-back start disposition. Only a newly created row can precede dispatch.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseAttemptReceipt {
    /// Exact original row, including any result already recorded.
    pub record: ReleaseAttempt,
    /// False for exact replay; replay is history, never a new dispatch grant.
    pub created: bool,
}
