//! Complete-set historical reconciliation; observations never renew dispatch authority.

use super::{
    Error, ReleaseAttempt, ReleaseAttemptInspection, ReleaseAttemptResult, ReleaseConsumption,
    ReleaseCoordinator,
};
use crate::{
    release::{DispatchStatus, RefComparison, TargetReadback, TargetReadbackRow},
    render::LimitedWriter,
    source::Sha256Digest,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write};

/// One exact immutable start and the presence of its result at an observed frontier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseAttemptReference {
    /// Original target ordinal.
    pub ordinal: u32,
    /// Original one-based attempt number.
    pub number: u64,
    /// False preserves historical absence even if a result is appended later.
    pub has_result: bool,
}

/// Complete observed attempt selection and preceding reconciliation identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReconciliationFrontier {
    /// Ordered exact start/result selection; native append requires current complete coverage.
    pub attempts: Vec<ReleaseAttemptReference>,
    /// Domain-separated identity of those original start/result bytes, not a mutable latest query.
    pub attempts_sha256: Sha256Digest,
    /// Exact previous complete record, or absence only for the first reconciliation.
    pub previous: Option<Sha256Digest>,
}

#[derive(Serialize)]
struct BorrowedAttempt<'a> {
    start: &'a super::ReleaseAttemptStart,
    result: Option<&'a ReleaseAttemptResult>,
}

fn identity(domain: &[u8], value: &impl Serialize, maximum: usize) -> Result<Sha256Digest, Error> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    if output.write_all(domain).is_err() || serde_json::to_writer(&mut output, value).is_err() {
        return Err(Error::StorageLimit);
    }
    Ok(Sha256Digest::of(&output.into_inner()))
}

impl ReleaseReconciliationFrontier {
    /// Capture the current complete immutable history without reading providers or changing state.
    ///
    /// # Errors
    /// Refuses invalid order, references or a frontier exceeding the explicit byte bound.
    pub fn current(
        attempts: &[ReleaseAttempt],
        previous: Option<&ReleaseReconciliationRecord>,
        maximum: usize,
    ) -> Result<Self, Error> {
        let references: Vec<_> = attempts
            .iter()
            .map(|row| ReleaseAttemptReference {
                ordinal: row.start.key.ordinal,
                number: row.start.key.number,
                has_result: row.result.is_some(),
            })
            .collect();
        let selected = select(attempts, &references)?;
        let attempts_sha256 =
            identity(b"GAZETTE_RELEASE_ATTEMPT_FRONTIER_V1\0", &selected, maximum)?;
        Ok(Self {
            attempts: references,
            attempts_sha256,
            previous: previous
                .map(|record| record.identity(maximum))
                .transpose()?,
        })
    }

    fn selected<'a>(
        &self,
        attempts: &'a [ReleaseAttempt],
        maximum: usize,
    ) -> Result<Vec<BorrowedAttempt<'a>>, Error> {
        let selected = select(attempts, &self.attempts)?;
        if identity(b"GAZETTE_RELEASE_ATTEMPT_FRONTIER_V1\0", &selected, maximum)?
            != self.attempts_sha256
        {
            return Err(Error::ReleaseSettlementVerification);
        }
        Ok(selected)
    }
}

fn select<'a>(
    attempts: &'a [ReleaseAttempt],
    references: &[ReleaseAttemptReference],
) -> Result<Vec<BorrowedAttempt<'a>>, Error> {
    let mut indexed = BTreeMap::new();
    for row in attempts {
        if indexed
            .insert((row.start.key.ordinal, row.start.key.number), row)
            .is_some()
        {
            return Err(Error::ReleaseSettlementVerification);
        }
    }
    let mut previous: Option<(u32, u64)> = None;
    let mut prior_result: Option<&ReleaseAttemptResult> = None;
    references
        .iter()
        .map(|reference| {
            let key = (reference.ordinal, reference.number);
            if previous.is_some_and(|old| key <= old)
                || (previous.is_some_and(|old| old.0 == key.0)
                    && !prior_result.is_some_and(ReleaseAttemptResult::permits_retry))
                || reference.number
                    != previous
                        .filter(|old| old.0 == key.0)
                        .map_or(Some(1), |old| old.1.checked_add(1))
                        .ok_or(Error::ReleaseSettlementVerification)?
            {
                return Err(Error::ReleaseSettlementVerification);
            }
            previous = Some(key);
            let row = indexed
                .get(&key)
                .ok_or(Error::ReleaseSettlementVerification)?;
            let result = if reference.has_result {
                Some(
                    row.result
                        .as_ref()
                        .ok_or(Error::ReleaseSettlementVerification)?,
                )
            } else {
                None
            };
            prior_result = result;
            Ok(BorrowedAttempt {
                start: &row.start,
                result,
            })
        })
        .collect()
}

/// Native custody's interpretation boundary, retained with the acquired observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseReconciliationMode {
    /// Ordinary settlement derivation under qualified native custody.
    Settlement,
    /// Prior effect lifetime is unresolved; observations cannot establish terminal settlement.
    ObservationOnly,
}

/// Acquired observations for the complete original consumed set, never a request to retry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReconciliation {
    /// Original immutable consumed authorization.
    pub authorization_id: String,
    /// Consecutive one-based reconciliation coordinate.
    pub number: u64,
    /// Exact history observed before provider acquisition; native append compares it atomically.
    pub frontier: ReleaseReconciliationFrontier,
    /// Actual held original or cleanly acquired later coordinator, not substituted original custody.
    pub coordinator: ReleaseCoordinator,
    /// Native-held interpretation restriction, not a caller's request to grant authority.
    pub mode: ReleaseReconciliationMode,
    /// All original targets in order, including unavailable members.
    pub readbacks: Vec<TargetReadbackRow>,
}

/// Technical outcome of one exact original target, independent from transport causation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseTargetOutcome {
    /// Current advertisement equals the intended commit; who caused it is not established.
    Intended,
    /// Exact pre-state and complete history establish no applied target effect.
    NoEffect,
    /// Another commit or regression after historical success conflicts with the retained intent.
    Conflict,
    /// Missing/ambiguous observation or incomplete prior effects prevent a conclusion.
    Unresolved,
}

/// Derived complete-set state. Reconciliation-required is explicitly nonterminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseSettlementState {
    /// Every original target is observed at its intended commit.
    Succeeded,
    /// Every target is conclusively unchanged and none has historical success.
    FailedNoEffect,
    /// At least one target succeeded and a conclusive conflict prevents completion of another.
    PartialTerminal,
    /// Ambiguity, unreachable state or a still-incomplete original set remains.
    ReconciliationRequired,
}
impl ReleaseSettlementState {
    /// Terminal history cannot be reopened by a later observation or dispatch request.
    pub fn is_terminal(self) -> bool {
        self != Self::ReconciliationRequired
    }
}

/// Per-target conclusion linked to exact acquired evidence and retained history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseTargetSettlement {
    /// Original target position; no site may disappear from a settlement.
    pub ordinal: u32,
    /// Derived observation/history result, never caller-assigned success.
    pub outcome: ReleaseTargetOutcome,
    /// Monotone historical success evidence, including native transport reports and earlier reads.
    pub ever_intended: bool,
}

/// Derived settlement for one exact reconciliation; no signature or retry capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSettlement {
    /// Complete-set outcome, not a claim of atomic multi-site deployment.
    pub state: ReleaseSettlementState,
    /// Original target order and full coverage.
    pub targets: Vec<ReleaseTargetSettlement>,
}

/// Immutable acquired evidence and its recomputable technical interpretation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReleaseReconciliationRecord {
    /// Actual observations and exact historical frontier.
    pub reconciliation: ReleaseReconciliation,
    /// Derived conclusions stored atomically with the complete evidence.
    pub settlement: ReleaseSettlement,
}
impl ReleaseReconciliationRecord {
    /// Domain-separated bounded identity for exact replay and the next history link.
    ///
    /// # Errors
    /// Refuses evidence exceeding its explicit byte bound.
    pub fn identity(&self, maximum: usize) -> Result<Sha256Digest, Error> {
        identity(b"GAZETTE_RELEASE_RECONCILIATION_V1\0", self, maximum)
    }
}

impl ReleaseReconciliation {
    /// Interpret complete observations against exact immutable attempt and preceding history.
    /// Native storage separately verifies actual held custody and current frontier equality.
    ///
    /// # Errors
    /// Refuses altered/partial targets, malformed comparisons, historical regression of custody,
    /// broken frontier links or any attempt to reopen terminal history.
    pub fn settle(
        &self,
        consumed: &ReleaseConsumption,
        attempts: &[ReleaseAttempt],
        previous: Option<&ReleaseReconciliationRecord>,
        maximum: usize,
    ) -> Result<ReleaseSettlement, Error> {
        if self.authorization_id != consumed.authorization_id
            || self.number
                != previous
                    .map_or(Some(1), |row| row.reconciliation.number.checked_add(1))
                    .ok_or(Error::ReleaseSettlementVerification)?
            || self.coordinator.binding != consumed.leases.coordinator.binding
            || self.coordinator.journal.schema_version != consumed.created.schema_version
            || self.coordinator.journal.journal_id != consumed.created.journal_id
            || self.coordinator.journal.journal_epoch != consumed.created.journal_epoch
            || self.coordinator.journal.writer_fence < consumed.created.writer_fence
            || self.readbacks.len() != consumed.targets.len()
            || self.readbacks.is_empty()
            || self.frontier.previous != previous.map(|row| row.identity(maximum)).transpose()?
        {
            return Err(Error::ReleaseSettlementVerification);
        }
        if let Some(previous) = previous {
            if previous.settlement.state.is_terminal() {
                return Err(Error::ReleaseTerminal);
            }
            if previous.reconciliation.authorization_id != self.authorization_id
                || (previous.reconciliation.mode == ReleaseReconciliationMode::ObservationOnly
                    && self.mode != ReleaseReconciliationMode::ObservationOnly)
                || previous.reconciliation.coordinator.journal.writer_fence
                    > self.coordinator.journal.writer_fence
                || previous.settlement.targets.len() != consumed.targets.len()
            {
                return Err(Error::ReleaseSettlementVerification);
            }
            // Retained uncertainty cannot disappear by omitting an earlier start or result.
            for old in &previous.reconciliation.frontier.attempts {
                let current = self
                    .frontier
                    .attempts
                    .iter()
                    .find(|row| row.ordinal == old.ordinal && row.number == old.number);
                if current.is_none_or(|row| old.has_result && !row.has_result) {
                    return Err(Error::ReleaseSettlementVerification);
                }
            }
        }
        let selected = self.frontier.selected(attempts, maximum)?;
        if selected.iter().any(|row| {
            row.start.key.authorization_id != self.authorization_id
                || usize::try_from(row.start.key.ordinal)
                    .map_or(true, |ordinal| ordinal >= consumed.targets.len())
        }) {
            return Err(Error::ReleaseSettlementVerification);
        }
        let mut targets = Vec::with_capacity(consumed.targets.len());
        for (index, (intent, row)) in consumed.targets.iter().zip(&self.readbacks).enumerate() {
            let target = &intent.target;
            if usize::try_from(target.ordinal).ok() != Some(index) || row.ordinal != target.ordinal
            {
                return Err(Error::ReleaseSettlementVerification);
            }
            let past = previous.map(|record| &record.settlement.targets[index]);
            if past.is_some_and(|row| row.ordinal != target.ordinal) {
                return Err(Error::ReleaseSettlementVerification);
            }
            let related: Vec<_> = selected
                .iter()
                .filter(|row| row.start.key.ordinal == target.ordinal)
                .collect();
            let mut ever_intended = past.is_some_and(|row| row.ever_intended)
                || related
                    .iter()
                    .any(|row| row.result.is_some_and(reported_success));
            let outcome = match &row.readback {
                TargetReadback::Observed {
                    advertisement,
                    comparison,
                } => {
                    if *comparison != RefComparison::compare(target, advertisement) {
                        return Err(Error::ReleaseSettlementVerification);
                    }
                    match comparison {
                        RefComparison::Intended => {
                            ever_intended = true;
                            ReleaseTargetOutcome::Intended
                        }
                        RefComparison::Prestate if ever_intended => ReleaseTargetOutcome::Conflict,
                        RefComparison::Prestate
                            if self.mode == ReleaseReconciliationMode::Settlement
                                && related.iter().all(|row| {
                                    row.result.is_some_and(ReleaseAttemptResult::permits_retry)
                                }) =>
                        {
                            ReleaseTargetOutcome::NoEffect
                        }
                        RefComparison::Conflict => ReleaseTargetOutcome::Conflict,
                        RefComparison::Prestate | RefComparison::Indeterminate => {
                            ReleaseTargetOutcome::Unresolved
                        }
                    }
                }
                TargetReadback::Unavailable { .. } => ReleaseTargetOutcome::Unresolved,
            };
            targets.push(ReleaseTargetSettlement {
                ordinal: target.ordinal,
                outcome,
                ever_intended,
            });
        }
        let all = |state| targets.iter().all(|row| row.outcome == state);
        let any = |state| targets.iter().any(|row| row.outcome == state);
        let state = if self.mode == ReleaseReconciliationMode::ObservationOnly {
            ReleaseSettlementState::ReconciliationRequired
        } else if all(ReleaseTargetOutcome::Intended) {
            ReleaseSettlementState::Succeeded
        } else if all(ReleaseTargetOutcome::NoEffect) {
            ReleaseSettlementState::FailedNoEffect
        } else if any(ReleaseTargetOutcome::Unresolved) {
            ReleaseSettlementState::ReconciliationRequired
        } else if any(ReleaseTargetOutcome::Intended) && any(ReleaseTargetOutcome::Conflict) {
            ReleaseSettlementState::PartialTerminal
        } else {
            ReleaseSettlementState::ReconciliationRequired
        };
        Ok(ReleaseSettlement { state, targets })
    }
}

fn reported_success(result: &ReleaseAttemptResult) -> bool {
    result.transport.is_ok_and(|row| {
        matches!(
            row.status,
            DispatchStatus::Updated | DispatchStatus::AlreadyPresent
        )
    }) || matches!(
        result.readback,
        TargetReadback::Observed {
            comparison: RefComparison::Intended,
            ..
        }
    )
}

/// Historical native inspection; current signature validity and provider access are irrelevant.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseReconciliationInspection {
    /// Complete current native consumed/attempt history and journal observation.
    pub history: ReleaseAttemptInspection,
    /// Consecutive immutable reconciliations, retaining incomplete and terminal conclusions.
    pub records: Vec<ReleaseReconciliationRecord>,
}

/// Explicit local evidence effects, separate from all remote mutation permissions.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReleaseReconciliationOptions {
    /// Permit reconciliation and derived settlement recording, never dispatch.
    pub allow_record: bool,
    /// Maximum domain-separated record/frontier encoding in bytes.
    #[schemars(range(max = usize::MAX))]
    pub evidence_bytes: usize,
}
impl Default for ReleaseReconciliationOptions {
    fn default() -> Self {
        Self {
            allow_record: false,
            evidence_bytes: 1 << 20,
        }
    }
}
impl ReleaseReconciliationOptions {
    /// Check explicit local evidence effects and a positive byte bound.
    ///
    /// # Errors
    /// Refuses absent permission or a zero bound before writing.
    pub fn validate(self) -> Result<(), Error> {
        if !self.allow_record {
            return Err(Error::ReleaseSettlementPermission);
        }
        if self.evidence_bytes == 0 {
            return Err(Error::InvalidLimits);
        }
        Ok(())
    }
}

/// Independently read-back complete record; exact replay preserves original custody and bytes.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct ReleaseReconciliationReceipt {
    /// Original complete acquired evidence and its derived settlement.
    pub record: ReleaseReconciliationRecord,
    /// Whether this call inserted the record; false never grants any new effect.
    pub inserted: bool,
}
