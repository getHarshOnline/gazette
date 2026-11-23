//! Exact bounded delivery of one existing registry reconciliation, without retry or storage work.

use super::RegistryReconcileError;
use crate::{
    render::LimitedWriter,
    source::Sha256Digest,
    store::{GenerationEffect, JournalHeader, RegistryEffect, RegistryHead, RegistryReceipt},
};
use schemars::JsonSchema;
use serde::Serialize;

/// Original failure stage and conservative effects; no inferred current journal coordinates.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum Failure<'a> {
    /// Complete projection failed before acquisition.
    Projection {
        /// Original stable native cause.
        code: &'a str,
    },
    /// Effect policy refused before acquisition.
    Policy {
        /// Original stable native cause.
        code: &'a str,
    },
    /// A portable source root could not be resolved before writer acquisition.
    SourceRoot {
        /// Original stable native cause.
        code: &'a str,
    },
    /// Source/store separation may fail before or after acquisition.
    Separation {
        /// Original stable native cause.
        code: &'a str,
        /// True only when the native operation acquired and advanced its writer.
        writer_acquired: bool,
    },
    /// Acquisition failed; an attempted generation is not rolled back by this response.
    Acquire {
        /// Original stable native cause.
        code: &'a str,
        /// Exact conservative generation effect from failed native acquisition.
        generation: GenerationEffect,
    },
    /// A writer was acquired, but replacement did not return verified evidence.
    Write {
        /// Original stable native cause.
        code: &'a str,
        /// Exact conservative replacement effect; acquisition already succeeded.
        effect: RegistryEffect,
    },
    /// Replacement returned this exact verified receipt, but closing its lifetime failed.
    Close {
        /// Original stable native cause.
        code: &'a str,
        /// Complete original replacement receipt; final close is not established.
        receipt: &'a RegistryReceipt,
    },
}

/// Complete native receipt or truthful failure; there is no reduced successful receipt.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Report<'a> {
    /// Exact native receipt, retaining its established field shape.
    Complete(&'a RegistryReceipt),
    /// Native failure with effect evidence but no private provider error text.
    Failed(Failure<'a>),
}

impl Report<'_> {
    /// Failure includes a verified replacement whose final close failed.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// Project only evidence already returned by the native operation.
pub fn report(result: &Result<RegistryReceipt, RegistryReconcileError>) -> Report<'_> {
    match result {
        Ok(receipt) => Report::Complete(receipt),
        Err(error) => Report::Failed(match error {
            RegistryReconcileError::Projection(_) => Failure::Projection { code: error.code() },
            RegistryReconcileError::Policy(_) => Failure::Policy { code: error.code() },
            RegistryReconcileError::SourceRoot(_) => Failure::SourceRoot { code: error.code() },
            RegistryReconcileError::Separation {
                writer_acquired, ..
            } => Failure::Separation {
                code: error.code(),
                writer_acquired: *writer_acquired,
            },
            RegistryReconcileError::Acquire(value) => Failure::Acquire {
                code: error.code(),
                generation: value.generation,
            },
            RegistryReconcileError::Write(value) => Failure::Write {
                code: error.code(),
                effect: value.effect,
            },
            RegistryReconcileError::Close { receipt, .. } => Failure::Close {
                code: error.code(),
                receipt,
            },
        }),
    }
}

/// Pre-effect storage for a complete compact response. It does not perform reconciliation.
pub struct Reservation {
    maximum: usize,
    bytes: Vec<u8>,
}

impl Reservation {
    /// Reserve and prove every native result shape before any writer acquisition.
    ///
    /// # Errors
    /// Refuses invalid budgets, allocation failure, or insufficient capacity. Native codes are
    /// bounded to 256 UTF-8 bytes here; a future violating result remains available through
    /// `ContractError`, rather than losing effects or claiming bounded delivery.
    pub fn new(maximum: usize) -> Result<Self, &'static str> {
        if maximum == 0 || maximum > 16 << 20 {
            return Err("registry_response_limit");
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum + 1)
            .map_err(|_| "registry_response_allocation")?;
        let mut reservation = Self { maximum, bytes };
        let receipt = RegistryReceipt {
            journal: JournalHeader {
                schema_version: u32::MAX,
                journal_id: [255; 32],
                journal_epoch: u64::MAX,
                writer_fence: u64::MAX,
            },
            registry: RegistryHead {
                semantic_sha256: Sha256Digest::of(&[]),
                configuration_sha256: Sha256Digest::of(&[]),
                rows_sha256: Sha256Digest::of(&[]),
                rows: u64::MAX,
            },
        };
        let code = "\0".repeat(256);
        reservation.encode(&Report::Complete(&receipt))?;
        for failure in [
            Failure::Projection { code: &code },
            Failure::Policy { code: &code },
            Failure::SourceRoot { code: &code },
            Failure::Separation {
                code: &code,
                writer_acquired: false,
            },
            Failure::Acquire {
                code: &code,
                generation: GenerationEffect::NotAttempted,
            },
            Failure::Acquire {
                code: &code,
                generation: GenerationEffect::Unverified,
            },
            Failure::Write {
                code: &code,
                effect: RegistryEffect::NotAttempted,
            },
            Failure::Write {
                code: &code,
                effect: RegistryEffect::Unverified,
            },
            Failure::Close {
                code: &code,
                receipt: &receipt,
            },
        ] {
            reservation.encode(&Report::Failed(failure))?;
        }
        reservation.bytes.clear();
        Ok(reservation)
    }

    fn encode(&mut self, report: &Report<'_>) -> Result<(), &'static str> {
        self.bytes.clear();
        serde_json::to_writer(LimitedWriter::new(&mut self.bytes, self.maximum), report)
            .map_err(|_| "registry_response_limit")
    }

    /// Encode the original result once. Both interfaces deliver these exact compact bytes.
    ///
    /// # Errors
    /// A changed native/schema contract preserves the original result and report, without a
    /// bounded-delivery claim. No result is retried, replaced with a generic success or discarded.
    pub fn finish<'a>(
        mut self,
        result: &'a Result<RegistryReceipt, RegistryReconcileError>,
    ) -> Result<Encoded, ContractError<'a>> {
        let report = report(result);
        if result
            .as_ref()
            .err()
            .is_some_and(|error| error.code().len() > 256)
            || self.encode(&report).is_err()
        {
            return Err(ContractError { result, report });
        }
        Ok(Encoded {
            bytes: self.bytes,
            is_error: report.is_error(),
        })
    }
}

/// Complete compact payload; framing is the caller's separately reserved responsibility.
pub struct Encoded {
    /// Original result projection, not a fallback reconstructed from a database.
    pub bytes: Vec<u8>,
    /// Whether the exact native outcome failed, including final close failure.
    pub is_error: bool,
}

/// Explicit violated native/delivery contract, not a bounded fallback or successful receipt.
#[derive(Debug)]
pub struct ContractError<'a> {
    /// Original result remains available to the caller without loss or native rereads.
    pub result: &'a Result<RegistryReceipt, RegistryReconcileError>,
    /// Exact effect projection for best-effort diagnostics; not promised to fit the reservation.
    pub report: Report<'a>,
}
