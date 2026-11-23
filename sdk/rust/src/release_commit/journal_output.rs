//! Pre-effect delivery reservation for native checkpointing and recorded-worktree recovery.

use super::{
    CheckpointError, SynchronizationState, WorktreeRecoveryError, WorktreeRecoveryFailure,
    WorktreeRecoveryProgress, output::Encoded,
};
use crate::{
    render::LimitedWriter,
    source::{CommitId, Sha256Digest},
    store::{JournalHeader, PreparationReceipt},
};
use gazette_core::{
    release::CommitReport,
    source::InventoryBuilder,
    store::{
        CheckpointEffect, CheckpointReceipt, GenerationEffect, PreparationCommit,
        PreparationEffect, PreparationRetention,
    },
};
use schemars::JsonSchema;
use serde::Serialize;
use std::{os::unix::ffi::OsStrExt, path::Path};

/// Exact checkpoint failure boundary; absent coordinates are never inferred from the request.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckpointFailure<'a> {
    /// Stable native diagnostic, without private provider text.
    pub code: &'a str,
    /// Whether the native operation actually returned a held writer.
    pub writer_acquired: bool,
    /// Conservative attempted generation when acquisition failed.
    pub generation: Option<GenerationEffect>,
    /// Conservative checkpoint write boundary when recording failed.
    pub effect: Option<CheckpointEffect>,
}

/// Recovery failure retains independent synchronization and journal observations.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveryFailure<'a> {
    /// Stable original native failure class.
    pub code: &'a str,
    /// Original progress, not a replacement observation or reusable lifetime.
    pub progress: &'a WorktreeRecoveryProgress,
    /// Acquisition uncertainty, separate from successful writer acquisition.
    pub generation: Option<GenerationEffect>,
    /// Completion-record uncertainty, separate from synchronization.
    pub effect: Option<PreparationEffect>,
}

/// Native checkpoint lookup identity when the complete private recipe does not fit.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckpointPin<'a> {
    /// Actual current writer observation returned with the receipt.
    pub journal: JournalHeader,
    /// Exact immutable checkpoint coordinate.
    pub id: Sha256Digest,
    /// Original creation coordinates, including on replay.
    pub created: JournalHeader,
    /// Complete native commit/object identity and inventory totals.
    pub commit: &'a CommitReport,
}

/// Existing check identity without its potentially large private evidence text.
#[derive(Debug, Serialize, JsonSchema)]
pub struct CheckPin {
    /// Exact original private evidence digest.
    pub evidence_sha256: Sha256Digest,
    /// Original check append coordinates.
    pub recorded: JournalHeader,
}

/// Original worktree phase identity, not current filesystem verification.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WorktreePin<'a> {
    /// Original recipe append coordinates.
    pub created: JournalHeader,
    /// Exact selected check, including explicit absence.
    pub check_sha256: Option<Sha256Digest>,
    /// Original complete completion evidence if it was observed.
    pub completion: Option<&'a PreparationCommit>,
}

/// Exact immutable recovery lookup and all observed phase coordinates.
#[derive(Debug, Serialize, JsonSchema)]
pub struct RecoveryPin<'a> {
    /// Original native insertion/replay outcome.
    pub inserted: bool,
    /// Current writer coordinates returned by the native operation.
    pub journal: JournalHeader,
    /// Exact immutable preparation coordinate.
    pub id: Sha256Digest,
    /// Original intent coordinates.
    pub created: JournalHeader,
    /// Original complete publication output identity.
    pub output: crate::source::TreeIdentity,
    /// Original private input-evidence identity.
    pub evidence_sha256: Sha256Digest,
    /// Complete bounded native retention observation.
    pub retention: Option<&'a PreparationRetention>,
    /// Original check identity without private text.
    pub check: Option<CheckPin>,
    /// Original Git recipe/completion identity without duplicating metadata.
    pub worktree: Option<WorktreePin<'a>>,
}

/// Native success is unchanged. Reduced delivery is explicit and is never successful delivery.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Report<'a, T, F, P> {
    /// Complete original native success receipt.
    Complete(&'a T),
    /// Native failure, retaining a verified receipt even when final close failed.
    Failed {
        /// Original failure and conservative effects.
        failure: F,
        /// Original receipt, if the native operation returned one before failing.
        receipt: Option<&'a T>,
    },
    /// Full private receipt exceeded the reserved carrier; exact lookup evidence remains.
    Limited {
        /// Explicit delivery failure, not native rollback or a fresh success receipt.
        code: &'static str,
        /// Exact caller-selected Unix locator bytes; not an inferred canonical path.
        #[schemars(with = "Vec<u8>")]
        locator: &'a [u8],
        /// Exact caller-selected record coordinate for independent inspection.
        id: Sha256Digest,
        /// Original native failure, absent only when the native operation succeeded.
        failure: Option<F>,
        /// Original observed receipt identity, never synthesized from request expectations.
        receipt: Option<P>,
    },
}

/// Shared native/full/compact checkpoint serialization and discovery shape.
pub type CheckpointReport<'a> =
    Report<'a, CheckpointReceipt, CheckpointFailure<'a>, CheckpointPin<'a>>;
/// Shared native/full/compact recovery serialization and discovery shape.
pub type RecoveryReport<'a> = Report<'a, PreparationReceipt, RecoveryFailure<'a>, RecoveryPin<'a>>;

/// Project only the existing native result; no read-back or operation is repeated.
pub fn checkpoint_report(
    result: &Result<CheckpointReceipt, CheckpointError>,
) -> CheckpointReport<'_> {
    match result {
        Ok(receipt) => Report::Complete(receipt),
        Err(error) => {
            let (writer_acquired, generation, effect, receipt) = match error {
                CheckpointError::Policy(_) => (false, None, None, None),
                CheckpointError::Separation {
                    writer_acquired, ..
                }
                | CheckpointError::Git {
                    writer_acquired, ..
                } => (*writer_acquired, None, None, None),
                CheckpointError::Acquire(error) => (false, Some(error.generation), None, None),
                CheckpointError::Write(error) => (true, None, Some(error.effect), None),
                CheckpointError::Close { receipt, .. } => {
                    (true, None, None, Some(receipt.as_ref()))
                }
            };
            Report::Failed {
                failure: CheckpointFailure {
                    code: error.code(),
                    writer_acquired,
                    generation,
                    effect,
                },
                receipt,
            }
        }
    }
}

/// Preserve synchronization, attempted writes and an original receipt independently.
pub fn recovery_report(
    result: &Result<PreparationReceipt, WorktreeRecoveryError>,
) -> RecoveryReport<'_> {
    match result {
        Ok(receipt) => Report::Complete(receipt),
        Err(error) => Report::Failed {
            failure: RecoveryFailure {
                code: error.code(),
                progress: &error.progress,
                generation: match error.cause.as_ref() {
                    WorktreeRecoveryFailure::Writer(error) => Some(error.generation),
                    _ => None,
                },
                effect: match error.cause.as_ref() {
                    WorktreeRecoveryFailure::Record(error) => Some(error.effect),
                    _ => None,
                },
            },
            receipt: error.receipt.as_deref(),
        },
    }
}

impl<'a> From<&'a CheckpointReceipt> for CheckpointPin<'a> {
    fn from(receipt: &'a CheckpointReceipt) -> Self {
        Self {
            journal: receipt.journal,
            id: receipt.checkpoint.id,
            created: receipt.checkpoint.created,
            commit: &receipt.checkpoint.commit,
        }
    }
}
impl<'a> From<&'a PreparationReceipt> for RecoveryPin<'a> {
    fn from(receipt: &'a PreparationReceipt) -> Self {
        Self {
            inserted: receipt.inserted,
            journal: receipt.journal,
            id: receipt.record.intent.id,
            created: receipt.record.intent.created,
            output: receipt.record.intent.output,
            evidence_sha256: receipt.record.intent.evidence_sha256,
            retention: receipt.record.retention.as_ref(),
            check: receipt.record.check.as_ref().map(|check| CheckPin {
                evidence_sha256: check.evidence_sha256,
                recorded: check.recorded,
            }),
            worktree: receipt
                .record
                .worktree
                .as_ref()
                .map(|worktree| WorktreePin {
                    created: worktree.created,
                    check_sha256: worktree.check_sha256,
                    completion: worktree.completion.as_ref(),
                }),
        }
    }
}

/// Allocation and exact-locator reservation made before any native operation.
pub struct Reservation<'a> {
    locator: &'a [u8],
    id: Sha256Digest,
    maximum: usize,
    bytes: Vec<u8>,
}

impl<'a> Reservation<'a> {
    /// Prove worst-case compact native effect and identity delivery for this exact selection.
    /// The caller reserves framing separately and invokes its native operation only afterward.
    ///
    /// # Errors
    /// Refuses invalid/insufficient capacity or allocation failure before native effects.
    /// A future native code beyond 256 UTF-8 bytes is a contract violation, not silent truncation.
    pub fn new(locator: &'a Path, id: Sha256Digest, maximum: usize) -> Result<Self, &'static str> {
        if maximum == 0 || maximum > 16 << 20 {
            return Err("journal_response_limit");
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum + 1)
            .map_err(|_| "journal_response_allocation")?;
        let mut reservation = Self {
            locator: locator.as_os_str().as_bytes(),
            id,
            maximum,
            bytes,
        };
        reservation.prove()?;
        reservation.bytes.clear();
        Ok(reservation)
    }

    /// Encode the original checkpoint result, or explicitly reduced original custody evidence.
    ///
    /// # Errors
    /// A violated native/serialization contract preserves the complete original result.
    pub fn checkpoint<'r>(
        mut self,
        result: &'r Result<CheckpointReceipt, CheckpointError>,
    ) -> Result<Encoded, ContractError<'r, CheckpointReceipt, CheckpointError>>
    where
        'a: 'r,
    {
        let full = checkpoint_report(result);
        let observed = match &full {
            Report::Complete(receipt) => Some(*receipt),
            Report::Failed { receipt, .. } => *receipt,
            Report::Limited { .. } => None,
        };
        if observed.is_some_and(|receipt| receipt.checkpoint.id != self.id) {
            return Err(ContractError { result });
        }
        let failed = result.is_err();
        if self.encode(&full).is_ok() {
            return Ok(Encoded {
                bytes: self.bytes,
                is_error: failed,
            });
        }
        let (failure, receipt) = match full {
            Report::Complete(receipt) => (None, Some(receipt)),
            Report::Failed { failure, receipt } => (Some(failure), receipt),
            Report::Limited { .. } => return Err(ContractError { result }),
        };
        let compact: CheckpointReport<'_> = Report::Limited {
            code: "checkpoint_report_limit",
            locator: self.locator,
            id: self.id,
            failure,
            receipt: receipt.map(CheckpointPin::from),
        };
        self.encode(&compact)
            .map_err(|_| ContractError { result })?;
        Ok(Encoded {
            bytes: self.bytes,
            is_error: true,
        })
    }

    /// Encode the original recovery result without re-running synchronization or journal work.
    ///
    /// # Errors
    /// A violated native/serialization contract preserves the complete original result.
    pub fn recover<'r>(
        mut self,
        result: &'r Result<PreparationReceipt, WorktreeRecoveryError>,
    ) -> Result<Encoded, ContractError<'r, PreparationReceipt, WorktreeRecoveryError>>
    where
        'a: 'r,
    {
        let full = recovery_report(result);
        let observed = match &full {
            Report::Complete(receipt) => Some(*receipt),
            Report::Failed { receipt, .. } => *receipt,
            Report::Limited { .. } => None,
        };
        if observed.is_some_and(|receipt| receipt.record.intent.id != self.id) {
            return Err(ContractError { result });
        }
        let failed = result.is_err();
        if self.encode(&full).is_ok() {
            return Ok(Encoded {
                bytes: self.bytes,
                is_error: failed,
            });
        }
        let (failure, receipt) = match full {
            Report::Complete(receipt) => (None, Some(receipt)),
            Report::Failed { failure, receipt } => (Some(failure), receipt),
            Report::Limited { .. } => return Err(ContractError { result }),
        };
        let compact: RecoveryReport<'_> = Report::Limited {
            code: "recovery_report_limit",
            locator: self.locator,
            id: self.id,
            failure,
            receipt: receipt.map(RecoveryPin::from),
        };
        self.encode(&compact)
            .map_err(|_| ContractError { result })?;
        Ok(Encoded {
            bytes: self.bytes,
            is_error: true,
        })
    }

    fn encode(&mut self, value: &impl Serialize) -> Result<(), &'static str> {
        self.bytes.clear();
        serde_json::to_writer(LimitedWriter::new(&mut self.bytes, self.maximum), value)
            .map_err(|_| "journal_response_contract")
    }

    fn prove(&mut self) -> Result<(), &'static str> {
        let header = JournalHeader {
            schema_version: u32::MAX,
            journal_id: [255; 32],
            journal_epoch: u64::MAX,
            writer_fence: u64::MAX,
        };
        let id = Sha256Digest::of(&[]);
        let commit: CommitId = "ffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .map_err(|_| "journal_response_contract")?;
        let report = CommitReport {
            commit: commit.clone(),
            output: InventoryBuilder::new(Default::default())
                .finish()
                .identity(),
            parent: Some(commit.clone()),
            entries: usize::MAX,
            bytes: u64::MAX,
        };
        let retention = PreparationRetention {
            intent_id: id,
            output: report.output,
            entries: u64::MAX,
            bytes: u64::MAX,
            recorded: header,
        };
        let completion = PreparationCommit {
            report: report.clone(),
            recorded: header,
        };
        let code = "\0".repeat(256);
        // Include every finite effect value rather than relying on enum spelling lengths.
        for generation in [GenerationEffect::NotAttempted, GenerationEffect::Unverified] {
            for effect in [CheckpointEffect::NotAttempted, CheckpointEffect::Unverified] {
                let value: CheckpointReport<'_> = Report::Limited {
                    code: "checkpoint_report_limit",
                    locator: self.locator,
                    id: self.id,
                    failure: Some(CheckpointFailure {
                        code: &code,
                        writer_acquired: false,
                        generation: Some(generation),
                        effect: Some(effect),
                    }),
                    receipt: Some(CheckpointPin {
                        journal: header,
                        id,
                        created: header,
                        commit: &report,
                    }),
                };
                self.encode(&value)?;
            }
            for effect in [
                PreparationEffect::NotAttempted,
                PreparationEffect::Unverified,
            ] {
                for sync in [
                    SynchronizationState::NotAttempted,
                    SynchronizationState::Unverified,
                    SynchronizationState::Verified,
                ] {
                    let progress = WorktreeRecoveryProgress {
                        writer_acquired: false,
                        journal: Some(header),
                        verified_commit: Some(commit.clone()),
                        output_sync: sync,
                        git_sync: sync,
                    };
                    let value: RecoveryReport<'_> = Report::Limited {
                        code: "recovery_report_limit",
                        locator: self.locator,
                        id: self.id,
                        failure: Some(RecoveryFailure {
                            code: &code,
                            progress: &progress,
                            generation: Some(generation),
                            effect: Some(effect),
                        }),
                        receipt: Some(RecoveryPin {
                            inserted: false,
                            journal: header,
                            id,
                            created: header,
                            output: report.output,
                            evidence_sha256: id,
                            retention: Some(&retention),
                            check: Some(CheckPin {
                                evidence_sha256: id,
                                recorded: header,
                            }),
                            worktree: Some(WorktreePin {
                                created: header,
                                check_sha256: Some(id),
                                completion: Some(&completion),
                            }),
                        }),
                    };
                    self.encode(&value)?;
                }
            }
        }
        Ok(())
    }
}

/// Original evidence retained when a future native result violates the reserved contract.
#[derive(Debug)]
pub struct ContractError<'a, T, E> {
    /// Complete native result; no bounded-delivery guarantee is made for a contract violation.
    pub result: &'a Result<T, E>,
}
