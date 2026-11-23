//! Pre-effect response reservation for existing local Git object and custody operations.

use super::{CommitEffect, CommitError, CustodyEffect, CustodyError};
use crate::{
    render::LimitedWriter,
    source::{CommitId, Sha256Digest},
};
use gazette_core::source::InventoryBuilder;
use schemars::JsonSchema;
use serde::Serialize;

const REASON_BYTES: usize = 256;

/// Complete success retains the native shape; failure never hides the native effect.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Report<'a, T, E> {
    /// Exact original native read-back report.
    Complete(&'a T),
    /// Native failure with its bounded static diagnostic.
    Failed {
        /// Local operation class, not publisher or release status.
        code: &'static str,
        /// Exact native reason when it fits the explicitly reserved diagnostic bound.
        reason: &'a str,
        /// Original conservative effect, including a created commit lacking read-back.
        effect: &'a E,
    },
    /// An oversized diagnostic is identified without discarding its native effect.
    DiagnosticLimit {
        /// Explicit diagnostic-overflow class, distinct from successful native work.
        code: &'static str,
        /// Identity of the complete original UTF-8 reason, not a truncated quotation.
        reason_sha256: Sha256Digest,
        /// Exact original effect; diagnostic loss never becomes rollback or retry permission.
        effect: &'a E,
    },
}

impl<T, E> Report<'_, T, E> {
    /// True for every native failure, including an oversized diagnostic.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::Complete(_))
    }
}

/// Discovery and delivery shape for local commit preparation.
pub type CommitReport<'a> = Report<'a, super::CommitReport, CommitEffect>;
/// Discovery and delivery shape for immutable local custody retention.
pub type CustodyReport<'a> = Report<'a, super::CustodyReport, CustodyEffect>;

/// Project a returned result without native reads, reconstruction or another write attempt.
pub fn commit_report(result: &Result<super::CommitReport, CommitError>) -> CommitReport<'_> {
    match result {
        Ok(report) => Report::Complete(report),
        Err(error) => failure(
            "release_commit",
            "release_commit_diagnostic_limit",
            error.reason,
            &error.effect,
        ),
    }
}

/// Project exact custody success or conservative native failure without reinspection.
pub fn custody_report(result: &Result<super::CustodyReport, CustodyError>) -> CustodyReport<'_> {
    match result {
        Ok(report) => Report::Complete(report),
        Err(error) => failure(
            "release_custody",
            "release_custody_diagnostic_limit",
            error.reason,
            &error.effect,
        ),
    }
}

fn failure<'a, T, E>(
    code: &'static str,
    overflow: &'static str,
    reason: &'a str,
    effect: &'a E,
) -> Report<'a, T, E> {
    if reason.len() <= REASON_BYTES {
        Report::Failed {
            code,
            reason,
            effect,
        }
    } else {
        Report::DiagnosticLimit {
            code: overflow,
            reason_sha256: Sha256Digest::of(reason.as_bytes()),
            effect,
        }
    }
}

/// Allocated response capacity, proven before the caller invokes native work exactly once.
/// Does not perform preparation, retention, inspection or delivery to a carrier.
pub struct Reservation {
    maximum: usize,
    bytes: Vec<u8>,
}

impl Reservation {
    /// Reserve every complete native success/effect shape, including worst-case diagnostic escaping.
    /// Framing is separate: a CLI must reserve its newline outside maximum.
    ///
    /// # Errors
    /// Refuses unsupported/insufficient response capacity or allocation failure before effects.
    pub fn new(maximum: usize) -> Result<Self, &'static str> {
        if maximum == 0 || maximum > 16 << 20 {
            return Err("release_commit_response_limit");
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum.saturating_add(1))
            .map_err(|_| "release_commit_response_allocation")?;
        let mut reserved = Self { maximum, bytes };
        let commit: CommitId = "ffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .map_err(|_| "release_commit_response_contract")?;
        let report = super::CommitReport {
            commit: commit.clone(),
            output: InventoryBuilder::new(Default::default())
                .finish()
                .identity(),
            parent: Some(commit.clone()),
            entries: usize::MAX,
            bytes: u64::MAX,
        };
        let custody = super::CustodyReport {
            id: Sha256Digest::of(&[]),
            commit: report.clone(),
        };
        reserved.encode(&CommitReport::Complete(&report))?;
        reserved.encode(&CustodyReport::Complete(&custody))?;
        let reason = "\0".repeat(REASON_BYTES);
        for effect in [
            CommitEffect::NotAttempted,
            CommitEffect::ObjectsMayRemain,
            CommitEffect::CommitCreated(commit),
        ] {
            reserved.encode(&CommitReport::Failed {
                code: "release_commit",
                reason: &reason,
                effect: &effect,
            })?;
            reserved.encode(&CommitReport::DiagnosticLimit {
                code: "release_commit_diagnostic_limit",
                reason_sha256: Sha256Digest::of(&[]),
                effect: &effect,
            })?;
        }
        for effect in [
            CustodyEffect::NotAttempted,
            CustodyEffect::ReferenceMayRemain,
        ] {
            reserved.encode(&CustodyReport::Failed {
                code: "release_custody",
                reason: &reason,
                effect: &effect,
            })?;
            reserved.encode(&CustodyReport::DiagnosticLimit {
                code: "release_custody_diagnostic_limit",
                reason_sha256: Sha256Digest::of(&[]),
                effect: &effect,
            })?;
        }
        reserved.bytes.clear();
        Ok(reserved)
    }

    fn encode(&mut self, value: &impl Serialize) -> Result<(), &'static str> {
        self.bytes.clear();
        serde_json::to_writer(LimitedWriter::new(&mut self.bytes, self.maximum), value)
            .map_err(|_| "release_commit_response_limit")
    }

    /// Encode the original prepare result; neither delivery failure nor diagnostic overflow retries it.
    ///
    /// # Errors
    /// A changed native serialization contract retains the original result, without a bounded claim.
    pub fn prepare<'a>(
        self,
        result: &'a Result<super::CommitReport, CommitError>,
    ) -> Result<Encoded, ContractError<'a, super::CommitReport, CommitError>> {
        let report = commit_report(result);
        self.finish(result, &report, report.is_error())
    }

    /// Encode the original retain result, preserving its exact immutable custody and object evidence.
    ///
    /// # Errors
    /// A changed native serialization contract retains the original result, without a bounded claim.
    pub fn retain<'a>(
        self,
        result: &'a Result<super::CustodyReport, CustodyError>,
    ) -> Result<Encoded, ContractError<'a, super::CustodyReport, CustodyError>> {
        let report = custody_report(result);
        self.finish(result, &report, report.is_error())
    }

    fn finish<'a, T, E>(
        mut self,
        result: &'a Result<T, E>,
        report: &impl Serialize,
        is_error: bool,
    ) -> Result<Encoded, ContractError<'a, T, E>> {
        if self.encode(report).is_err() {
            return Err(ContractError { result });
        }
        Ok(Encoded {
            bytes: self.bytes,
            is_error,
        })
    }
}

/// Exact compact result bytes, shared by CLI and MCP without another serialization contract.
pub struct Encoded {
    /// Original success/effect projection; transport framing has not been appended.
    pub bytes: Vec<u8>,
    /// Native work or complete outcome delivery failed, even if these bytes were delivered.
    pub is_error: bool,
}

/// Actual serialization-contract violation, not a generic result or an automatic retry instruction.
#[derive(Debug)]
pub struct ContractError<'a, T, E> {
    /// Complete original result remains borrowed, including every exact effect and native report.
    pub result: &'a Result<T, E>,
}
