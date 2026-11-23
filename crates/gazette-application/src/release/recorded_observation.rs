//! Native acquisition followed by immutable history, not dispatch or settlement.

use super::{ReadbackReport, observe_targets};
use gazette_compiler::release::{Limits, TargetSet, check_targets};
use gazette_core::{
    release::{RemoteReadError, RemoteReadOptions},
    source::Sha256Digest,
    store::{
        self, JournalHeader, ReadOptions, RefObservationInput, RefObservationInspection,
        RefObservationReceipt, RefObservationWriteOptions,
    },
};
use gazette_ports::{
    release::RemoteReader,
    store::{RefObservationReader, RefObservationWriter},
};
use std::{fmt, path::Path};

/// Independent provider-read and local-history permissions; no implicit effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct RecordObservationOptions {
    /// Selected provider's complete-set deadline, per-target bounds and read/network permissions.
    pub read: RemoteReadOptions,
    /// Separate immutable-record permission.
    pub record: RefObservationWriteOptions,
}

/// Preserve native read evidence when its subsequent local append cannot be verified.
#[derive(Debug)]
pub enum RecordObservationError {
    /// Refused before any new provider read; caller already owns its writer lifetime.
    BeforeRead(store::Error),
    /// Global read policy refused the observation before invoking the provider.
    Read(RemoteReadError),
    /// Reads completed, but record delivery failed; inspect the original coordinate.
    Write {
        /// Complete observed rows, not a durable receipt or fresh retry instruction.
        report: Box<ReadbackReport>,
        /// Original conservative SQL-write boundary.
        cause: store::RefObservationWriteError,
    },
}

impl fmt::Display for RecordObservationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BeforeRead(cause) => write!(f, "{cause}; no new destination read"),
            Self::Read(cause) => cause.fmt(f),
            Self::Write { cause, .. } => write!(
                f,
                "{cause}; completed destination observations retained in error evidence"
            ),
        }
    }
}
impl std::error::Error for RecordObservationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::BeforeRead(cause) => Some(cause),
            Self::Read(cause) => Some(cause),
            Self::Write { cause, .. } => Some(cause),
        }
    }
}

/// Sample the complete checked selection and append its exact result under an
/// already held writer. Existing IDs refuse before reads, even for the same
/// targets: a new sample is new evidence. Inspect retained history explicitly.
///
/// A crash before append can leave no record despite completed reads. This
/// operation never dispatches, consumes authorization or retries an external
/// effect. The supplied reader owns its provider selection and limitations.
///
/// # Errors
/// Refuses missing permissions, existing coordinates or stale writer before reads;
/// failed recording retains the complete observation and conservative SQL effects.
pub fn record_target_observation(
    reader: &impl RemoteReader,
    writer: &mut impl RefObservationWriter,
    targets: &TargetSet,
    id: Sha256Digest,
    options: RecordObservationOptions,
) -> Result<RefObservationReceipt, RecordObservationError> {
    options
        .record
        .validate()
        .map_err(RecordObservationError::BeforeRead)?;
    options
        .read
        .validate()
        .map_err(RecordObservationError::Read)?;
    if writer
        .inspect_ref_observation(id)
        .map_err(RecordObservationError::BeforeRead)?
        .record
        .is_some()
    {
        return Err(RecordObservationError::BeforeRead(
            store::Error::ReadbackConflict,
        ));
    }
    let report =
        observe_targets(reader, targets, options.read).map_err(RecordObservationError::Read)?;
    writer
        .record_ref_observation(
            RefObservationInput {
                id,
                targets: targets.targets(),
                readbacks: &report.targets,
            },
            options.record,
        )
        .map_err(|cause| RecordObservationError::Write {
            report: Box::new(report),
            cause,
        })
}

/// Inspect historical complete evidence and recheck its retained target contract.
/// Never resample destinations or silently replace a failed expected journal binding.
///
/// # Errors
/// Refuses native history, expected-header, complete-target or comparison failures.
pub fn inspect_target_observation(
    reader: &impl RefObservationReader,
    path: &Path,
    id: Sha256Digest,
    expected: Option<&JournalHeader>,
    read: ReadOptions,
    limits: Limits,
) -> Result<RefObservationInspection, store::Error> {
    read.validate()?;
    let inspection = reader.inspect_ref_observation(path, id, read)?;
    if expected.is_some_and(|header| *header != inspection.journal.header) {
        return Err(store::Error::JournalMismatch);
    }
    if let Some(record) = &inspection.record {
        check_targets(&record.targets, limits).map_err(|_| store::Error::ReadbackVerification)?;
        record.as_input().validate()?;
    }
    Ok(inspection)
}
