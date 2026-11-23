//! Destination-observation and durable-history commands shared by CLI and MCP.

use super::{Command, Kernel};
use gazette_sdk::{delivery, github, release, source, store};
use serde::Serialize;
use std::{fmt, path::PathBuf};

/// Application failure before transport-specific encoding.
#[derive(Debug)]
pub struct Failure {
    code: String,
    message: String,
}

impl Failure {
    fn new(code: impl ToString, message: impl ToString) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
    #[must_use]
    /// Stable native failure class.
    pub fn code(&self) -> &str {
        &self.code
    }
    #[must_use]
    /// Private explanation suitable for the selected transport.
    pub fn message_text(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for Failure {}

/// Observe one complete exact destination-target set.
pub struct ObserveTargets {
    /// Exact target-array JSON bytes.
    pub targets_json: Vec<u8>,
    /// Explicit native Git and isolation selection.
    pub git: release::GitSelection,
    /// Complete target parsing ceilings.
    pub limits: release::Limits,
    /// Explicit destination-read permissions and bounds.
    pub options: release::RemoteReadOptions,
}

impl Command for ObserveTargets {
    type Outcome = release::ReadbackReport;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.git.is_valid() {
            return Err(Failure::new(
                "release_read_request",
                "invalid exact Git selection",
            ));
        }
        let targets = release::parse_targets(&self.targets_json, self.limits)
            .map_err(|error| Failure::new(error.code(), error))?;
        release::observe_targets(&self.git, &targets, self.options)
            .map_err(|error| Failure::new(error.code(), error))
    }
}

/// Acquire, sample and record one immutable diagnostic observation.
pub struct RecordObservation {
    /// Existing private journal root.
    pub root: PathBuf,
    /// Independently retained current journal coordinates.
    pub expected_header: store::JournalHeader,
    /// Exact immutable observation coordinate.
    pub id: source::Sha256Digest,
    /// Exact target-array JSON bytes.
    pub targets_json: Vec<u8>,
    /// Explicit native Git and isolation selection.
    pub git: release::GitSelection,
    /// Complete roots protected from journal effects.
    pub protected_roots: Vec<PathBuf>,
    /// Optional independently retained coordinator selection.
    pub coordinator: Option<store::coordinator::LocalCoordinatorSelection>,
    /// Complete target parsing ceilings.
    pub limits: release::Limits,
    /// Independent read, record, writer and coordinator permissions.
    pub options: release::ObservationSessionOptions,
}

/// Owned native observation result retaining possible effect evidence.
pub struct RecordObservationOutcome {
    result: Result<release::RefObservationReceipt, release::ObservationSessionError>,
}

impl RecordObservationOutcome {
    #[must_use]
    /// Borrow the complete shared native report projection.
    pub fn report(&self) -> release::ObservationSessionReport<'_> {
        release::ObservationSessionReport::new(&self.result)
    }
    #[must_use]
    /// Whether the native observation lifecycle failed.
    pub const fn is_failure(&self) -> bool {
        self.result.is_err()
    }
}

impl Command for RecordObservation {
    type Outcome = RecordObservationOutcome;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let targets = release::parse_targets(&self.targets_json, self.limits).map_err(|error| {
            Failure::new(
                error.code(),
                "complete target input refused; no observation session started",
            )
        })?;
        let result = release::record_observation(
            &self.git,
            release::ObservationSessionRequest {
                root: &self.root,
                expected: self.expected_header,
                id: self.id,
                targets: &targets,
                protected_roots: &self.protected_roots,
                coordinator: self.coordinator.as_ref(),
            },
            self.options,
        );
        Ok(RecordObservationOutcome { result })
    }
}

/// Inspect one immutable historical observation without destination access.
pub struct InspectObservation {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing journal database file.
    pub path: PathBuf,
    /// Exact immutable observation coordinate.
    pub id: source::Sha256Digest,
    /// Optional independently retained journal expectation.
    pub expected_header: Option<store::JournalHeader>,
    /// Native read and coordination-file options.
    pub read: store::ReadOptions,
    /// Retained target-contract validation ceilings.
    pub limits: release::Limits,
}

impl Command for InspectObservation {
    type Outcome = release::RefObservationInspection;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::new(
                "storage_permission",
                "native history reads not permitted",
            ));
        }
        release::inspect_target_observation(
            &self.path,
            self.id,
            self.expected_header.as_ref(),
            self.read,
            self.limits,
        )
        .map_err(|error| Failure::new(error.code(), error))
    }
}

/// Selected immutable release-history family.
#[derive(Clone, Copy)]
pub enum HistoryKind {
    /// Original authorization custody.
    Authorization,
    /// Original destination leases.
    Leases,
    /// Exact consumed release intent.
    Consumption,
    /// Complete release attempts.
    Attempts,
    /// Reconciliation and settlement history.
    Reconciliations,
}

/// Inspect one exact durable release-history family.
pub struct InspectHistory {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing journal database file.
    pub path: PathBuf,
    /// Exact durable record identifier.
    pub record_id: String,
    /// Optional independently retained journal expectation.
    pub expected_header: Option<store::JournalHeader>,
    /// Native read and coordination-file options.
    pub read: store::ReadOptions,
    /// Selected immutable history family.
    pub kind: HistoryKind,
}

/// Exact typed result for the selected release-history family.
#[derive(Serialize)]
#[serde(untagged)]
pub enum HistoryOutcome {
    /// Original authorization history.
    Authorization(store::ReleaseAuthorizationInspection),
    /// Original lease history.
    Leases(store::ReleaseLeaseInspection),
    /// Original consumption history.
    Consumption(store::ReleaseConsumptionInspection),
    /// Complete attempt history.
    Attempts(store::ReleaseAttemptInspection),
    /// Complete reconciliation history.
    Reconciliations(store::ReleaseReconciliationInspection),
}

impl Command for InspectHistory {
    type Outcome = HistoryOutcome;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::new(
                "storage_permission",
                "explicit database-read permission required; no database opened",
            ));
        }
        let expected = self.expected_header.as_ref();
        match self.kind {
            HistoryKind::Authorization => store::inspect_release_authorization(
                &self.path,
                &self.record_id,
                expected,
                self.read,
            )
            .map(HistoryOutcome::Authorization),
            HistoryKind::Leases => {
                store::inspect_release_leases(&self.path, &self.record_id, expected, self.read)
                    .map(HistoryOutcome::Leases)
            }
            HistoryKind::Consumption => {
                store::inspect_release_consumption(&self.path, &self.record_id, expected, self.read)
                    .map(HistoryOutcome::Consumption)
            }
            HistoryKind::Attempts => {
                store::inspect_release_attempts(&self.path, &self.record_id, expected, self.read)
                    .map(HistoryOutcome::Attempts)
            }
            HistoryKind::Reconciliations => store::inspect_release_reconciliations(
                &self.path,
                &self.record_id,
                expected,
                self.read,
            )
            .map(HistoryOutcome::Reconciliations),
        }
        .map_err(|error| Failure::new(error.code(), error))
    }
}

/// Observe one exact public GitHub Pages build.
pub struct ObserveGithubPages {
    /// Explicit pinned curl selection.
    pub curl: github::Curl,
    /// Exact repository and build selection.
    pub selection: github::PagesBuild,
    /// Independent read, execute and network permissions and bounds.
    pub options: github::Options,
}

impl Command for ObserveGithubPages {
    type Outcome = github::PagesObservation;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        github::observe_pages_build(&self.curl, &self.selection, self.options)
            .map_err(|error| Failure::new(error.code(), error))
    }
}

/// Compare selected served routes with one pinned retained output.
pub struct ObserveDelivery {
    /// Complete native delivery request JSON.
    pub request_json: Vec<u8>,
    /// Complete native request ceiling.
    pub max_input_bytes: usize,
}

/// Complete served-route report and its aggregate match decision.
pub struct DeliveryOutcome {
    report: delivery::Report,
    matches: bool,
}

impl DeliveryOutcome {
    #[must_use]
    /// Borrow the complete native served-route report.
    pub fn report(&self) -> &delivery::Report {
        &self.report
    }
    #[must_use]
    /// Whether every selected route matched its retained expectation.
    pub const fn matches_selected_routes(&self) -> bool {
        self.matches
    }
}

impl Command for ObserveDelivery {
    type Outcome = DeliveryOutcome;
    type Failure = Failure;
    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = delivery::Request::parse(&self.request_json, self.max_input_bytes)
            .map_err(|error| Failure::new(error.code(), error))?;
        let report = delivery::observe_retained(&request)
            .map_err(|error| Failure::new(error.code(), error))?;
        let matches = report.matches_selected_routes();
        Ok(DeliveryOutcome { report, matches })
    }
}
