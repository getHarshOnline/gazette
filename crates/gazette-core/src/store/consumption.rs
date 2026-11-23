//! One-use technical intent and local destination leases, never dispatch or settlement.

use super::{
    Error, JournalHeader, JournalInspection, LocalCoordinatorBinding, ReleaseAuthorizationRecord,
};
use crate::{
    release::{
        RefAdvertisement, RefComparison, ReleaseBinding, RemoteState, Target, TargetReadback,
        TargetReadbackRow,
    },
    source::Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Actual independently anchored writer selection, obtained from the native held guard.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseCoordinator {
    /// Fixed selected local custody binding; labels do not authenticate a principal.
    pub binding: LocalCoordinatorBinding,
    /// Current held generation, not a predicted successor.
    pub journal: JournalHeader,
}

impl ReleaseCoordinator {
    /// Whether this held coordinator is a strictly newer generation in the same anchored lineage.
    /// The journal root may change only through separately verified adoption; identity, scope,
    /// journal and epoch never do. This is history comparison, not authority acquisition.
    pub fn continues(&self, previous: &Self) -> bool {
        self.binding.coordinator_id == previous.binding.coordinator_id
            && self.binding.principal_scope == previous.binding.principal_scope
            && self.binding.journal_id == previous.binding.journal_id
            && self.binding.journal_epoch == previous.binding.journal_epoch
            && self.journal.schema_version == previous.journal.schema_version
            && self.journal.journal_id == previous.journal.journal_id
            && self.journal.journal_epoch == previous.journal.journal_epoch
            && self.journal.writer_fence > previous.journal.writer_fence
    }
}

/// Generation-bearing reservation of one exact configured destination within the selected journal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseLease {
    /// Original target position.
    pub ordinal: u32,
    /// Exact configured destination; no alias normalization is inferred.
    pub remote: String,
    /// Exact branch name.
    pub branch: String,
    /// Monotonically increasing native generation for this destination.
    pub generation: u64,
}

/// Complete local lease set. This is not a cross-host or remote-provider locking claim.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseLeases {
    /// Existing pending authorization identity.
    pub authorization_id: String,
    /// Actual anchored holder and original acquisition generation.
    pub coordinator: ReleaseCoordinator,
    /// Entire ordered target set; no partial lease set is usable.
    pub targets: Vec<ReleaseLease>,
}

impl ReleaseLeases {
    /// Validate representation against the original pending authorization.
    /// Native storage separately proves that these remain the current held generations.
    ///
    /// # Errors
    /// Refuses a different holder, target set, epoch, fence or missing generation.
    pub fn validate(&self, registration: &ReleaseAuthorizationRecord) -> Result<(), Error> {
        let body = registration.as_input().validate()?;
        let held = &self.coordinator;
        if self.authorization_id != *body.record_id()
            || held.binding.coordinator_id != *body.coordinator_id()
            || held.binding.journal_id != *body.journal_id()
            || held.binding.journal_epoch != *body.journal_epoch()
            || held.journal != *registration.binding.journal()
            || self.targets.len() != body.ordered_targets().len()
        {
            return Err(Error::ReleaseLeaseVerification);
        }
        for (lease, target) in self.targets.iter().zip(body.ordered_targets()) {
            if lease.ordinal != target.ordinal
                || lease.remote != target.remote
                || lease.branch != target.branch
                || lease.generation == 0
            {
                return Err(Error::ReleaseLeaseVerification);
            }
        }
        Ok(())
    }
}

/// Independent local lease permission, not permission to consume or dispatch.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseLeaseOptions {
    /// Permit the complete local lease-set append.
    pub allow_acquire: bool,
}
impl ReleaseLeaseOptions {
    /// Require explicit local mutation permission.
    ///
    /// # Errors
    /// Refuses before native lease work.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_acquire {
            return Err(Error::ReleaseLeasePermission);
        }
        Ok(())
    }
}

/// Application-verified current inputs to one atomic pending-to-consumed transition.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct ReleaseConsumptionInput<'a> {
    /// Exact original retained authorization and preparation selection.
    pub registration: &'a ReleaseAuthorizationRecord,
    /// Fresh independently supplied verification bindings; original registration is not rewritten.
    pub binding: &'a ReleaseBinding,
    /// Complete current native lease set.
    pub leases: &'a ReleaseLeases,
    /// Complete sequential native pre-state observations, never an atomic remote snapshot.
    pub readbacks: &'a [TargetReadbackRow],
    /// Reverified complete native preparation identity.
    pub preparations_sha256: Sha256Digest,
}

impl ReleaseConsumptionInput<'_> {
    /// Validate representation and exact pre-state equality, not signatures or provider truth.
    ///
    /// # Errors
    /// Refuses changed intent, backward observation time, partial/indeterminate/conflicting reads,
    /// or mismatched leases. Not-advertised never means unborn.
    pub fn validate(self) -> Result<(), Error> {
        self.leases.validate(self.registration)?;
        super::ReleaseAuthorizationInput {
            authorization: &self.registration.authorization,
            binding: self.binding,
            preparations_sha256: self.preparations_sha256,
            preparation_ids: &self.registration.preparation_ids,
        }
        .validate()?;
        if self.preparations_sha256 != self.registration.preparations_sha256
            || self.binding.observed_at() < self.registration.binding.observed_at()
            || self.readbacks.len() != self.binding.ordered_targets().len()
        {
            return Err(Error::ReleaseConsumptionVerification);
        }
        for (row, target) in self.readbacks.iter().zip(self.binding.ordered_targets()) {
            if row.ordinal != target.ordinal {
                return Err(Error::ReleaseConsumptionVerification);
            }
            match (&row.readback, &target.expected_remote_oid_or_unborn) {
                (
                    TargetReadback::Observed {
                        advertisement: advertisement @ RefAdvertisement::Commit { oid },
                        comparison,
                    },
                    RemoteState::Commit { oid: expected },
                ) if oid == expected
                    && *comparison == RefComparison::compare(target, advertisement) => {}
                (
                    TargetReadback::Observed {
                        advertisement: RefAdvertisement::Absent,
                        comparison: RefComparison::Prestate,
                    },
                    RemoteState::Unborn,
                ) => {}
                _ => return Err(Error::ReleasePrestate),
            }
        }
        Ok(())
    }
}

/// Accepted batch-intent identity; no predecessor compatibility variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum ReleaseBatchSchema {
    /// Exact single batch and complete immutable per-target intents.
    WebsiteReleaseBatchIntentV1,
}

/// Exact effect recipe reserved durably before any dispatch may begin.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTargetIntent {
    /// Exact original target, not a lookup in mutable workspace configuration.
    pub target: Target,
    /// Native destination generation held at consumption.
    pub lease_generation: u64,
}

/// Immutable one-use technical intent. Authorization custody remains its separate FK owner.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseConsumption {
    /// Closed batch-intent schema identity.
    pub schema_id: ReleaseBatchSchema,
    /// Original pending record identity, unique for consumption within the journal.
    pub authorization_id: String,
    /// Exact original received body including signatures and transport whitespace.
    pub authorization_sha256: Sha256Digest,
    /// Exact native preparation batch consumed, not current reconstructed output.
    pub preparations_sha256: Sha256Digest,
    /// Independent authority/key/time binding used at this transition.
    pub binding: ReleaseBinding,
    /// Original complete native holder/generations, not reactivated by historical read-back.
    pub leases: ReleaseLeases,
    /// Original complete native pre-state evidence.
    pub readbacks: Vec<TargetReadbackRow>,
    /// Complete target intents persisted in the same transaction as this batch.
    pub targets: Vec<ReleaseTargetIntent>,
    /// Original consumed writer generation.
    pub created: JournalHeader,
}
impl ReleaseConsumption {
    /// Construct the exact storage representation after bounded input validation.
    /// Storage still verifies original rows, current holder and active lease generations.
    ///
    /// # Errors
    /// Refuses mismatched intent or preconditions; does not perform cryptographic verification.
    pub fn from_input(input: ReleaseConsumptionInput<'_>) -> Result<Self, Error> {
        input.validate()?;
        Ok(Self {
            schema_id: ReleaseBatchSchema::WebsiteReleaseBatchIntentV1,
            authorization_id: input.leases.authorization_id.clone(),
            authorization_sha256: Sha256Digest::of(input.registration.authorization.as_bytes()),
            preparations_sha256: input.preparations_sha256,
            binding: input.binding.clone(),
            leases: input.leases.clone(),
            readbacks: input.readbacks.to_vec(),
            targets: input
                .binding
                .ordered_targets()
                .iter()
                .zip(&input.leases.targets)
                .map(|(target, lease)| ReleaseTargetIntent {
                    target: target.clone(),
                    lease_generation: lease.generation,
                })
                .collect(),
            created: input.leases.coordinator.journal,
        })
    }

    /// Validate historical representation against its retained original authorization.
    /// This neither samples current time nor turns original leases into current authority.
    ///
    /// # Errors
    /// Refuses altered keys, targets, comparisons, generations or original input identity.
    pub fn validate(&self, registration: &ReleaseAuthorizationRecord) -> Result<(), Error> {
        let expected = Self::from_input(ReleaseConsumptionInput {
            registration,
            binding: &self.binding,
            leases: &self.leases,
            readbacks: &self.readbacks,
            preparations_sha256: self.preparations_sha256,
        })?;
        if *self != expected {
            return Err(Error::ReleaseConsumptionVerification);
        }
        Ok(())
    }
}

/// Original lease evidence, including superseded or uncertain acquisition history.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseLeaseInspection {
    /// Current journal observation, not a renewed lease generation.
    pub journal: JournalInspection,
    /// Original complete set or explicit absence; presence does not establish current custody.
    pub record: Option<ReleaseLeases>,
}

/// Read-only exact consumed history, available without active signatures or provider access.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseConsumptionInspection {
    /// Current native journal inspection, not a new consumption generation.
    pub journal: JournalInspection,
    /// Exact original intent or explicit absence.
    pub record: Option<ReleaseConsumption>,
}

/// Independent local consumption permission; no push or signing capability.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseConsumptionOptions {
    /// Permit one irreversible local authorization-consumption transaction.
    pub allow_consume: bool,
}
impl ReleaseConsumptionOptions {
    /// Require explicit consumption permission.
    ///
    /// # Errors
    /// Refuses before consumption, never implying earlier lease acquisition was rolled back.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_consume {
            return Err(Error::ReleaseConsumptionPermission);
        }
        Ok(())
    }
}

/// Independently read-back complete consumed intent, not a dispatch grant or success claim.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseConsumptionReceipt {
    /// Actual held native generation.
    pub journal: JournalHeader,
    /// Original immutable batch and all targets.
    pub record: ReleaseConsumption,
}

/// Conservative write boundary shared by the two required release-transaction appends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseWriteEffect {
    /// No insert attempted by this operation; earlier phases remain separate.
    NotAttempted,
    /// Native write attempted without a complete returned receipt; inspect original history.
    Unverified,
}

/// Failed lease or consumption write. Never infer rollback from missing receipt delivery.
#[derive(Debug)]
pub struct ReleaseWriteError {
    /// Redacted native or consistency cause.
    pub cause: Error,
    /// Known boundary for this write, not any prior lease/provider effect.
    pub effect: ReleaseWriteEffect,
}
impl From<Error> for ReleaseWriteError {
    fn from(cause: Error) -> Self {
        Self {
            cause,
            effect: ReleaseWriteEffect::NotAttempted,
        }
    }
}
impl fmt::Display for ReleaseWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; release journal write {:?}; inspect original intent before retrying",
            self.cause, self.effect
        )
    }
}
impl std::error::Error for ReleaseWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
