//! Release-artifact and custody commands shared by CLI and MCP.

use std::{fmt, path::PathBuf};

use gazette_sdk::{release, release_commit, source, store};

use super::{Command, Kernel};

/// Application failure before transport-specific encoding.
#[derive(Debug)]
pub struct Failure {
    code: String,
    message: String,
    reconciliation_required: bool,
}

impl Failure {
    fn before(code: impl ToString, message: impl ToString) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            reconciliation_required: false,
        }
    }

    fn after(code: impl ToString, message: impl ToString) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            reconciliation_required: true,
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

    #[must_use]
    /// Whether admitted native effects require exact-state inspection.
    pub const fn reconciliation_required(&self) -> bool {
        self.reconciliation_required
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Bounded native result bytes with their effect-aware failure bit.
pub struct EncodedOutcome {
    bytes: Vec<u8>,
    is_error: bool,
    native_failure: Option<(String, String)>,
}

impl EncodedOutcome {
    #[must_use]
    /// Borrow the complete native result bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    #[must_use]
    /// Whether the complete native result represents failure.
    pub const fn is_failure(&self) -> bool {
        self.is_error
    }

    #[must_use]
    /// Consume the outcome and retain its complete bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Borrow the native failure class and explanation when execution failed.
    #[must_use]
    pub fn native_failure(&self) -> Option<(&str, &str)> {
        self.native_failure
            .as_ref()
            .map(|(code, message)| (code.as_str(), message.as_str()))
    }
}

fn metadata(bytes: &[u8], maximum: usize) -> Result<release_commit::CommitMetadata, Failure> {
    release_commit::CommitMetadata::parse_json(bytes, maximum)
        .map_err(|error| Failure::before("commit_metadata_input", error.reason))
}

/// Verify one exact existing local commit without writes.
pub struct VerifyCommit {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing independent bare repository.
    pub repository: PathBuf,
    /// Exact commit object identifier.
    pub commit: source::CommitId,
    /// Independently expected complete output tree.
    pub expected_tree: source::TreeIdentity,
    /// Exact metadata recipe JSON bytes.
    pub metadata_json: Vec<u8>,
    /// Complete metadata input ceiling.
    pub max_metadata_bytes: usize,
    /// Native object and content ceilings.
    pub limits: source::RevisionLimits,
}

impl Command for VerifyCommit {
    type Outcome = release_commit::CommitReport;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "release_commit_permission",
                "explicit native-read permission required; no repository opened",
            ));
        }
        let metadata = metadata(&self.metadata_json, self.max_metadata_bytes)?;
        release_commit::verify(
            &self.repository,
            &self.commit,
            self.expected_tree,
            &metadata,
            self.limits,
        )
        .map_err(|error| Failure::before("release_commit", error))
    }
}

/// Inspect one exact immutable custody reference and its commit objects.
pub struct InspectCustody {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing independent bare repository.
    pub repository: PathBuf,
    /// Exact immutable custody coordinate.
    pub custody_id: source::Sha256Digest,
    /// Expected commit object identifier.
    pub commit: source::CommitId,
    /// Independently expected complete output tree.
    pub expected_tree: source::TreeIdentity,
    /// Exact metadata recipe JSON bytes.
    pub metadata_json: Vec<u8>,
    /// Complete metadata input ceiling.
    pub max_metadata_bytes: usize,
    /// Native object and content ceilings.
    pub limits: source::RevisionLimits,
}

impl Command for InspectCustody {
    type Outcome = release_commit::CustodyReport;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "release_custody_permission",
                "explicit native-read permission required; no repository opened",
            ));
        }
        let metadata = metadata(&self.metadata_json, self.max_metadata_bytes)?;
        release_commit::inspect_custody(
            &self.repository,
            self.custody_id,
            &self.commit,
            self.expected_tree,
            &metadata,
            self.limits,
        )
        .map_err(|error| Failure::before("release_custody", error))
    }
}

/// Prepare detached local commit objects with response custody reserved first.
pub struct PrepareCommit {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing independent bare repository.
    pub repository: PathBuf,
    /// Existing retained output root.
    pub output_root: PathBuf,
    /// Independently expected complete output tree.
    pub expected_tree: source::TreeIdentity,
    /// Exact metadata recipe JSON bytes.
    pub metadata_json: Vec<u8>,
    /// Complete metadata input ceiling.
    pub max_metadata_bytes: usize,
    /// Complete roots protected from repository effects.
    pub protected_roots: Vec<PathBuf>,
    /// Native object-write permission and ceilings.
    pub options: release_commit::CommitOptions,
    /// Complete native response reservation bytes.
    pub response_bytes: usize,
}

impl Command for PrepareCommit {
    type Outcome = EncodedOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads || !self.options.allow_write_objects {
            return Err(Failure::before(
                "release_commit_permission",
                "explicit --allow-write-objects and native-read permissions are required; no native operation attempted",
            ));
        }
        let reservation = release_commit::output::Reservation::new(self.response_bytes)
            .map_err(|code| Failure::before(code, code))?;
        let metadata = metadata(&self.metadata_json, self.max_metadata_bytes)?;
        let result = release_commit::prepare(
            &self.repository,
            &self.output_root,
            self.expected_tree,
            &metadata,
            &self.protected_roots,
            self.options,
        );
        let native_failure = result
            .as_ref()
            .err()
            .map(|error| ("release_commit".to_owned(), error.to_string()));
        let encoded = reservation.prepare(&result).map_err(|_| {
            Failure::after(
                "release_commit_response_contract",
                "bounded delivery is not established; inspect the original repository before retry",
            )
        })?;
        Ok(EncodedOutcome {
            bytes: encoded.bytes,
            is_error: encoded.is_error,
            native_failure,
        })
    }
}

/// Retain exact commit objects under immutable local custody.
pub struct RetainCommit {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing independent bare repository.
    pub repository: PathBuf,
    /// Exact immutable custody coordinate.
    pub custody_id: source::Sha256Digest,
    /// Exact existing commit object identifier.
    pub commit: source::CommitId,
    /// Independently expected complete output tree.
    pub expected_tree: source::TreeIdentity,
    /// Exact metadata recipe JSON bytes.
    pub metadata_json: Vec<u8>,
    /// Complete metadata input ceiling.
    pub max_metadata_bytes: usize,
    /// Complete roots protected from custody effects.
    pub protected_roots: Vec<PathBuf>,
    /// Native retention permission and ceilings.
    pub options: release_commit::CustodyOptions,
    /// Complete native response reservation bytes.
    pub response_bytes: usize,
}

impl Command for RetainCommit {
    type Outcome = EncodedOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads || !self.options.allow_retain {
            return Err(Failure::before(
                "release_custody_permission",
                "explicit --allow-retain and native-read permissions are required; no native operation attempted",
            ));
        }
        let reservation = release_commit::output::Reservation::new(self.response_bytes)
            .map_err(|code| Failure::before(code, code))?;
        let metadata = metadata(&self.metadata_json, self.max_metadata_bytes)?;
        let result = release_commit::retain(
            &self.repository,
            self.custody_id,
            &self.commit,
            self.expected_tree,
            &metadata,
            &self.protected_roots,
            self.options,
        );
        let native_failure = result
            .as_ref()
            .err()
            .map(|error| ("release_custody".to_owned(), error.to_string()));
        let encoded = reservation.retain(&result).map_err(|_| {
            Failure::after(
                "release_commit_response_contract",
                "bounded delivery is not established; inspect the original repository before retry",
            )
        })?;
        Ok(EncodedOutcome {
            bytes: encoded.bytes,
            is_error: encoded.is_error,
            native_failure,
        })
    }
}

/// Inspect one exact immutable checkpoint without current Git verification.
pub struct InspectCheckpoint {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing journal database file.
    pub path: PathBuf,
    /// Exact immutable checkpoint coordinate.
    pub checkpoint_id: source::Sha256Digest,
    /// Optional independently retained journal expectation.
    pub expected_header: Option<store::JournalHeader>,
    /// Native read and coordination-file options.
    pub read: store::ReadOptions,
}

impl Command for InspectCheckpoint {
    type Outcome = release_commit::CheckpointInspection;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "storage_permission",
                "explicit native-read permission required; no inspection started",
            ));
        }
        release_commit::inspect_checkpoint(
            &self.path,
            self.checkpoint_id,
            self.expected_header.as_ref(),
            self.read,
        )
        .map_err(|error| Failure::before(error.code(), error))
    }
}

/// Join one recorded preparation to exact current worktree evidence.
pub struct InspectJournalWorktree {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing journal database file.
    pub database: PathBuf,
    /// Exact immutable preparation coordinate.
    pub preparation_id: source::Sha256Digest,
    /// Optional independently retained journal expectation.
    pub expected_header: Option<store::JournalHeader>,
    /// Independently selected current commit.
    pub commit: source::CommitId,
    /// Complete roots protected from native storage.
    pub protected_roots: Vec<PathBuf>,
    /// Native journal, object, output and history ceilings.
    pub options: release_commit::JournalWorktreeInspectionOptions,
}

impl Command for InspectJournalWorktree {
    type Outcome = release_commit::JournalWorktreeInspection;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "storage_permission",
                "explicit native-read permission required; no inspection started",
            ));
        }
        release_commit::inspect_journal_worktree(
            release_commit::JournalWorktreeInspectionRequest {
                database: &self.database,
                preparation_id: self.preparation_id,
                expected_header: self.expected_header.as_ref(),
                commit: &self.commit,
                protected_roots: &self.protected_roots,
            },
            self.options,
        )
        .map_err(|error| Failure::before(error.code(), error))
    }
}

/// Verify and record one exact local commit checkpoint.
pub struct CheckpointCommit {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing private journal root.
    pub root: PathBuf,
    /// Independently retained current journal coordinates.
    pub expected_header: store::JournalHeader,
    /// Exact immutable checkpoint coordinate.
    pub checkpoint_id: source::Sha256Digest,
    /// Existing independent bare repository.
    pub repository: PathBuf,
    /// Exact existing commit object identifier.
    pub commit: source::CommitId,
    /// Independently expected complete output tree.
    pub expected_tree: source::TreeIdentity,
    /// Exact metadata recipe JSON bytes.
    pub metadata_json: Vec<u8>,
    /// Complete metadata input ceiling.
    pub max_metadata_bytes: usize,
    /// Complete roots protected from journal/repository effects.
    pub protected_roots: Vec<PathBuf>,
    /// Native checkpoint, writer and object options.
    pub options: release_commit::CheckpointOptions,
    /// Complete native response reservation bytes.
    pub response_bytes: usize,
}

impl Command for CheckpointCommit {
    type Outcome = EncodedOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "storage_permission",
                "explicit native-read permission required; no journal operation attempted",
            ));
        }
        let reservation = release_commit::journal_output::Reservation::new(
            &self.root,
            self.checkpoint_id,
            self.response_bytes,
        )
        .map_err(|code| Failure::before(code, code))?;
        let metadata = metadata(&self.metadata_json, self.max_metadata_bytes)?;
        let result = release_commit::checkpoint(
            &self.root,
            self.expected_header,
            release_commit::CheckpointRequest {
                id: self.checkpoint_id,
                repository: &self.repository,
                commit: &self.commit,
                output: self.expected_tree,
                metadata: &metadata,
                protected_roots: &self.protected_roots,
            },
            self.options,
        );
        let native_failure = result
            .as_ref()
            .err()
            .map(|error| (error.code().to_owned(), error.to_string()));
        let encoded = reservation.checkpoint(&result).map_err(|_| {
            Failure::after(
                "journal_response_contract",
                "bounded delivery is not established; inspect original journal coordinates",
            )
        })?;
        Ok(EncodedOutcome {
            bytes: encoded.bytes,
            is_error: encoded.is_error,
            native_failure,
        })
    }
}

/// Synchronize and record one exact existing journal-bound worktree.
pub struct RecoverJournalWorktree {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing private journal database.
    pub database: PathBuf,
    /// Exact immutable preparation coordinate.
    pub preparation_id: source::Sha256Digest,
    /// Independently retained current journal coordinates.
    pub expected_header: store::JournalHeader,
    /// Independently selected current commit.
    pub commit: source::CommitId,
    /// Complete roots protected from recovery effects.
    pub protected_roots: Vec<PathBuf>,
    /// Native synchronization, record and writer options.
    pub options: release_commit::RecoverWorktreeOptions,
    /// Complete native response reservation bytes.
    pub response_bytes: usize,
}

impl Command for RecoverJournalWorktree {
    type Outcome = EncodedOutcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.allow_native_reads {
            return Err(Failure::before(
                "storage_permission",
                "explicit native-read permission required; no recovery attempted",
            ));
        }
        let reservation = release_commit::journal_output::Reservation::new(
            &self.database,
            self.preparation_id,
            self.response_bytes,
        )
        .map_err(|code| Failure::before(code, code))?;
        let result = release_commit::recover_recorded_worktree(
            release_commit::RecoverRecordedWorktreeRequest {
                database: &self.database,
                preparation_id: self.preparation_id,
                expected: self.expected_header,
                commit: &self.commit,
                protected_roots: &self.protected_roots,
            },
            self.options,
        );
        let native_failure = result
            .as_ref()
            .err()
            .map(|error| (error.code().to_owned(), error.to_string()));
        let encoded = reservation.recover(&result).map_err(|_| {
            Failure::after(
                "journal_response_contract",
                "bounded delivery is not established; inspect original journal coordinates",
            )
        })?;
        Ok(EncodedOutcome {
            bytes: encoded.bytes,
            is_error: encoded.is_error,
            native_failure,
        })
    }
}

/// Inspect one complete ordered set of retained local preparations.
pub struct InspectPreparations {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing private journal database.
    pub database: PathBuf,
    /// Independently retained current journal coordinates.
    pub expected_header: store::JournalHeader,
    /// Exact ordered selection-array JSON.
    pub selections_json: String,
    /// Complete roots protected from selected native storage.
    pub protected_roots: Vec<PathBuf>,
    /// Native complete-set inspection ceilings.
    pub options: release::PreparationBatchOptions,
}

impl Command for InspectPreparations {
    type Outcome = release::PreparationBatch;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        validate_preparation_paths(&self.database, &self.protected_roots)?;
        let selections = preparation_selections(
            self.allow_native_reads,
            &self.selections_json,
            self.options.targets,
        )?;
        release::inspect_preparations(
            release::PreparationBatchRequest {
                database: &self.database,
                expected: self.expected_header,
                selections: &selections,
                protected_roots: &self.protected_roots,
            },
            self.options,
        )
        .map_err(|error| Failure::before(error.code(), error))
    }
}

/// Verify exact authorization against a complete retained preparation set.
pub struct VerifyPreparedAuthorization {
    /// Explicit native-read permission.
    pub allow_native_reads: bool,
    /// Existing private journal database.
    pub database: PathBuf,
    /// Independently retained current journal coordinates.
    pub expected_header: store::JournalHeader,
    /// Exact ordered selection-array JSON.
    pub selections_json: String,
    /// Complete roots protected from selected native storage.
    pub protected_roots: Vec<PathBuf>,
    /// Native complete-set inspection ceilings.
    pub preparation_options: release::PreparationBatchOptions,
    /// Exact received authorization JSON.
    pub authorization_json: String,
    /// Independently supplied authorization binding JSON.
    pub binding_json: String,
    /// Native signature and input ceilings.
    pub authorization_limits: release::Limits,
}

impl Command for VerifyPreparedAuthorization {
    type Outcome = release::PreparedAuthorizationReport;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        validate_preparation_paths(&self.database, &self.protected_roots)?;
        let selections = preparation_selections(
            self.allow_native_reads,
            &self.selections_json,
            self.preparation_options.targets,
        )?;
        let binding =
            release::parse_binding(self.binding_json.as_bytes(), self.authorization_limits)
                .map_err(|error| Failure::before(error.code(), error))?;
        release::verify_prepared_authorization(
            release::PreparedAuthorizationRequest {
                authorization: self.authorization_json.as_bytes(),
                binding: &binding,
                preparations: release::PreparationBatchRequest {
                    database: &self.database,
                    expected: self.expected_header,
                    selections: &selections,
                    protected_roots: &self.protected_roots,
                },
            },
            release::PreparedAuthorizationOptions {
                authorization: self.authorization_limits,
                preparations: self.preparation_options,
            },
        )
        .map_err(|error| Failure::before(error.code(), error))
    }
}

fn preparation_selections(
    allowed: bool,
    json: &str,
    maximum: usize,
) -> Result<Vec<release::PreparationSelection>, Failure> {
    if !allowed {
        return Err(Failure::before(
            "native_read_permission",
            "explicit native-read permission required; no preparation inspection performed",
        ));
    }
    let raw: Vec<&serde_json::value::RawValue> = serde_json::from_str(json)
        .map_err(|_| Failure::before("release_preparation_request", "invalid selection JSON"))?;
    if raw.is_empty() || raw.len() > maximum {
        return Err(Failure::before(
            "release_preparation_selection",
            "complete preparation selection is empty or exceeds its bound",
        ));
    }
    raw.iter()
        .map(|value| release::parse_preparation_selection(value.get().as_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| Failure::before(error.code(), error))
}

fn validate_preparation_paths(
    database: &std::path::Path,
    protected_roots: &[PathBuf],
) -> Result<(), Failure> {
    if !database.is_absolute() || protected_roots.iter().any(|path| !path.is_absolute()) {
        return Err(Failure::before(
            "release_preparation_request",
            "database and every protected root must be explicit absolute paths",
        ));
    }
    Ok(())
}
