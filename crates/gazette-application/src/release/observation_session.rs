//! One managed local writer lifetime around the existing native observation/append path.

use super::{RecordObservationError, RecordObservationOptions, record_target_observation};
use gazette_compiler::release::TargetSet;
use gazette_core::{
    source::Sha256Digest,
    store::{self, JournalHeader, RefObservationReceipt, WriterOptions},
};
use gazette_ports::{
    destination::RootSeparation,
    release::RemoteReader,
    store::{JournalWriter, RefObservationWriter, WriterConnector},
};
use std::{
    fmt,
    path::{Path, PathBuf},
};

/// Exact diagnostic-history request; target selection is already structurally checked.
pub struct ObservationSessionRequest<'a> {
    /// Existing initialized private journal directory, never created or adopted here.
    pub root: &'a Path,
    /// Independently retained complete coordinates before generation acquisition.
    pub expected: JournalHeader,
    /// New immutable observation coordinate. Existing history is never resampled.
    pub id: Sha256Digest,
    /// Complete original target set, never inferred from native read results.
    pub targets: &'a TargetSet,
    /// All selected sources/isolation roots that must remain separate from the journal.
    pub protected_roots: &'a [PathBuf],
    /// Optional independently retained local custody selection; the connector must implement it.
    pub coordinator: Option<&'a store::LocalCoordinatorSelection>,
}

/// Separate local writer, destination-read and history-write permissions.
#[derive(Clone, Copy, Debug, Default)]
pub struct ObservationSessionOptions {
    /// Existing local generation acquisition and per-connection bounds.
    pub writer: WriterOptions,
    /// Independent provider read and immutable append permissions.
    pub observation: RecordObservationOptions,
    /// Independently permit control-anchor writes when a local coordinator is selected.
    pub allow_anchor_write: bool,
}

/// A later failure never erases an earlier native effect or successful receipt.
#[derive(Debug)]
pub enum ObservationSessionError {
    /// Refused before generation acquisition or provider invocation.
    Before(RecordObservationError),
    /// Local generation acquisition failed; retain its exact conservative effect boundary.
    Acquire(store::WriterAcquireError),
    /// Observation/append failed after acquisition; close failure is separate evidence.
    Record {
        /// Complete native failure, including observed rows after a failed append.
        cause: Box<RecordObservationError>,
        /// Independent close failure; absence means the local lifetime closed successfully.
        close_error: Option<store::Error>,
    },
    /// Append and independent read-back succeeded, but lifetime close did not.
    Close {
        /// Exact durable receipt remains available for independent inspection.
        receipt: Box<RefObservationReceipt>,
        /// Native close error never means the append was undone.
        cause: store::Error,
    },
}

impl fmt::Display for ObservationSessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Before(cause) => write!(f, "{cause}; no writer acquisition"),
            Self::Acquire(cause) => cause.fmt(f),
            Self::Record { cause, close_error } => write!(
                f,
                "{cause}; acquired generation remains; close failure: {}; inspect original observation before retrying",
                close_error.as_ref().map_or("none", store::Error::code)
            ),
            Self::Close { receipt, cause } => write!(
                f,
                "{cause}; observation {} was recorded; lifetime close failed; preserve and inspect history",
                receipt.record.id
            ),
        }
    }
}
impl std::error::Error for ObservationSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(match self {
            Self::Before(cause) => cause,
            Self::Acquire(cause) => cause,
            Self::Record { cause, .. } => cause.as_ref(),
            Self::Close { cause, .. } => cause,
        })
    }
}

/// Acquire one exact local writer, sample/record through the existing owner, and close.
///
/// Every permission is checked before acquisition. Source separation precedes any store
/// access. Existing IDs refuse before provider invocation, though the explicitly permitted
/// generation acquisition has already occurred. An explicitly selected coordinator checks
/// journal-only rollback; it does not grant cross-host fencing, destination leases,
/// consumed authorization, attempt records or settlement. No automatic retry occurs.
///
/// # Errors
/// Preserves generation uncertainty, completed native reads after failed append, and
/// successful receipts after failed close. Dropping a failed lifetime grants no rollback.
pub fn record_observation_session<C>(
    filesystem: &impl RootSeparation,
    connector: &C,
    reader: &impl RemoteReader,
    request: ObservationSessionRequest<'_>,
    options: ObservationSessionOptions,
) -> Result<RefObservationReceipt, ObservationSessionError>
where
    C: WriterConnector,
    C::Writer: RefObservationWriter,
{
    use ObservationSessionError as Error;
    options
        .observation
        .record
        .validate()
        .and_then(|()| options.writer.validate())
        .map_err(|cause| Error::Before(RecordObservationError::BeforeRead(cause)))?;
    options
        .observation
        .read
        .validate()
        .map_err(|cause| Error::Before(RecordObservationError::Read(cause)))?;
    if let Some(selected) = request.coordinator {
        crate::store::coordinator::validate_selection(
            selected,
            request.root,
            request.expected,
            options.allow_anchor_write,
        )
        .map_err(|cause| Error::Before(RecordObservationError::BeforeRead(cause)))?;
    }
    if !request.root.is_absolute()
        || request
            .protected_roots
            .iter()
            .any(|path| !path.is_absolute())
        || filesystem
            .check_root_separation(request.root, request.protected_roots)
            .is_err()
    {
        return Err(Error::Before(RecordObservationError::BeforeRead(
            store::Error::UnsafePath,
        )));
    }
    if let Some(selected) = request.coordinator {
        crate::store::coordinator::check_selection_roots(
            filesystem,
            selected,
            request.protected_roots,
        )
        .map_err(|cause| Error::Before(RecordObservationError::BeforeRead(cause)))?;
    }
    let mut writer = connector
        .acquire_writer(request.root, request.expected, options.writer)
        .map_err(Error::Acquire)?;
    let result = record_target_observation(
        reader,
        &mut writer,
        request.targets,
        request.id,
        options.observation,
    );
    let closed = writer.close();
    match (result, closed) {
        (Ok(receipt), Ok(())) => Ok(receipt),
        (Ok(receipt), Err(cause)) => Err(Error::Close {
            receipt: Box::new(receipt),
            cause,
        }),
        (Err(cause), closed) => Err(Error::Record {
            cause: Box::new(cause),
            close_error: closed.err(),
        }),
    }
}
