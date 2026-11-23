//! Native complete-set reconciliation without renewing publication or dispatch authority.

use gazette_compiler::release::Limits;
use gazette_core::{
    release::{RemoteReadError, RemoteReadOptions},
    store::{
        self, JournalHeader, ReleaseReconciliation, ReleaseReconciliationFrontier,
        ReleaseReconciliationOptions, ReleaseReconciliationReceipt, ReleaseWriteError,
    },
};
use gazette_ports::{release::RemoteReader, store::ReleaseReconciliationWriter};
use std::{fmt, path::Path};

/// Select original consumed history under an already-held native coordinator.
#[derive(Clone, Copy)]
pub struct ReconcileReleaseRequest<'a> {
    /// Exact native journal selection; no hidden acquisition or database migration.
    pub database: &'a Path,
    /// Independently retained current held generation, not a replacement for original custody.
    pub expected: JournalHeader,
    /// Original consumed authorization identifier; current approval validity is irrelevant.
    pub authorization_id: &'a str,
    /// Consecutive observation coordinate. Exact replay returns retained evidence without reads.
    pub number: u64,
}

/// Separate bounded destination reads and local evidence effects; no dispatch permission.
#[derive(
    Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct ReconcileReleaseOptions {
    /// Structural validation limits for the complete original target selection.
    pub targets: Limits,
    /// Independently selected native observation policy and limits.
    pub read: RemoteReadOptions,
    /// Permit bounded immutable reconciliation/settlement recording only.
    pub record: ReleaseReconciliationOptions,
}

/// Failed phase; an observation never becomes a retry authorization.
#[derive(Debug)]
pub enum ReconcileReleaseFailure {
    /// Native custody, complete history or original selection refused.
    Store(store::Error),
    /// Original targets failed the shared compiler contract.
    Targets(gazette_compiler::release::Error),
    /// Global observation policy or work limits refused.
    Read(RemoteReadError),
    /// Preserve uncertain durable-delivery evidence and inspect the original coordinate.
    Record(ReleaseWriteError),
}
impl From<store::Error> for ReconcileReleaseFailure {
    fn from(error: store::Error) -> Self {
        Self::Store(error)
    }
}
impl fmt::Display for ReconcileReleaseFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => error.fmt(f),
            Self::Targets(error) => error.fmt(f),
            Self::Read(error) => error.fmt(f),
            Self::Record(error) => error.fmt(f),
        }
    }
}

/// Keep acquired complete evidence available even when recording or custody verification fails.
#[derive(Debug)]
pub struct ReconcileReleaseError {
    /// Exact failed phase, without hidden repair or retry.
    pub cause: ReconcileReleaseFailure,
    /// Acquired evidence, if reads completed; not necessarily durably recorded.
    pub observed: Option<Box<ReleaseReconciliation>>,
}
impl fmt::Display for ReconcileReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}; preserve original reconciliation evidence",
            self.cause
        )
    }
}
impl std::error::Error for ReconcileReleaseError {}

/// Read every original target and atomically bind the derived settlement to retained history.
///
/// Reuses native immutable consumption after expiry, revocation or a clean custody succession.
/// The original authorization, attempt bytes and fences never change. Sequential observations
/// are not an atomic remote snapshot and matching intended refs do not establish causation.
/// Missing/ambiguous evidence remains explicit; this operation never dispatches or adopts a retry.
///
/// # Errors
/// Refuses wrong custody, partial/changed history, terminal reopening and stale-frontier appends.
/// A write failure retains acquired observations and native uncertainty for inspection.
pub fn reconcile_release(
    writer: &mut impl ReleaseReconciliationWriter,
    reader: &impl RemoteReader,
    request: ReconcileReleaseRequest<'_>,
    options: ReconcileReleaseOptions,
) -> Result<ReleaseReconciliationReceipt, ReconcileReleaseError> {
    let mut observed = None;
    let result = (|| -> Result<ReleaseReconciliationReceipt, ReconcileReleaseFailure> {
        options.record.validate()?;
        options
            .read
            .validate()
            .map_err(ReconcileReleaseFailure::Read)?;
        let held = writer.inspect_release_coordinator()?;
        let mode = writer.reconciliation_mode();
        if held.journal != request.expected {
            return Err(store::Error::JournalMismatch.into());
        }
        let inspected =
            writer.inspect_release_reconciliations(request.database, request.authorization_id)?;
        if inspected.history.journal.header != held.journal {
            return Err(store::Error::JournalMismatch.into());
        }
        let consumed = inspected
            .history
            .consumption
            .as_ref()
            .ok_or(store::Error::ReleaseSettlementVerification)?;
        if consumed.leases.coordinator.binding != held.binding {
            return Err(store::Error::ReleaseSettlementVerification.into());
        }
        if let Some(existing) = inspected
            .records
            .iter()
            .find(|row| row.reconciliation.number == request.number)
        {
            return Ok(ReleaseReconciliationReceipt {
                record: existing.clone(),
                inserted: false,
            });
        }
        let previous = inspected.records.last();
        if previous.is_some_and(|row| row.settlement.state.is_terminal()) {
            return Err(store::Error::ReleaseTerminal.into());
        }
        if Some(request.number)
            != previous.map_or(Some(1), |row| row.reconciliation.number.checked_add(1))
        {
            return Err(store::Error::ReleaseSettlementConflict.into());
        }
        let frontier = ReleaseReconciliationFrontier::current(
            &inspected.history.attempts,
            previous,
            options.record.evidence_bytes,
        )?;
        let targets = gazette_compiler::release::check_targets(
            &consumed
                .targets
                .iter()
                .map(|row| row.target.clone())
                .collect::<Vec<_>>(),
            options.targets,
        )
        .map_err(ReconcileReleaseFailure::Targets)?;
        let readbacks = super::observe_targets(reader, &targets, options.read)
            .map_err(ReconcileReleaseFailure::Read)?
            .targets;
        let input = ReleaseReconciliation {
            authorization_id: request.authorization_id.to_owned(),
            number: request.number,
            frontier,
            coordinator: held.clone(),
            mode,
            readbacks,
        };
        observed = Some(Box::new(input.clone()));
        let settlement = input.settle(
            consumed,
            &inspected.history.attempts,
            previous,
            options.record.evidence_bytes,
        )?;
        if writer.inspect_release_coordinator()? != held || writer.reconciliation_mode() != mode {
            return Err(store::Error::ReleaseSettlementConflict.into());
        }
        let receipt = writer
            .record_release_reconciliation(request.database, &input, options.record)
            .map_err(ReconcileReleaseFailure::Record)?;
        if receipt.record.reconciliation != input || receipt.record.settlement != settlement {
            return Err(store::Error::ReleaseSettlementVerification.into());
        }
        Ok(receipt)
    })();
    result.map_err(|cause| ReconcileReleaseError { cause, observed })
}
