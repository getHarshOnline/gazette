//! Bounded delivery of existing native creation results, without retry or output reconstruction.

use super::{
    BackupReceipt, InitializationReceipt, JournalHeader, OutputError, OutputLimits, RestoreReceipt,
};
use crate::{render::LimitedWriter, source::Sha256Digest};
use schemars::JsonSchema;
use serde::Serialize;
use std::{os::unix::ffi::OsStrExt, path::Path};

/// Independently retainable evidence from the actual native result, not inspected or inferred later.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evidence {
    /// Fresh initialization returned this independently read-back header.
    Initialized {
        /// Original receipt's coordinates; not adoption or release authority.
        header: JournalHeader,
    },
    /// Backup completed; this original pin is required for independent verification.
    Backup {
        /// Exact original manifest identity, never recomputed from suspect output.
        manifest_sha256: Sha256Digest,
    },
    /// Detached restoration completed, without live-store activation.
    Restore {
        /// Exact original preparation receipt identity.
        receipt_sha256: Sha256Digest,
    },
    /// Native work failed; retained residue is unverified, even when files look complete.
    Failed,
}

/// Native failure with a lossless private locator; no source paths or raw provider error text.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Failure<'a> {
    /// Stable native cause classification.
    pub code: &'static str,
    /// Exact Unix path bytes. None means the native result reported no new output.
    pub directory_unix_bytes: Option<&'a [u8]>,
}

/// Compact fallback when the complete native result cannot be encoded within the selected budget.
#[derive(Debug, Serialize, JsonSchema)]
pub struct Delivery<'a> {
    /// Fixed delivery failure code, separate from the native operation's result.
    pub code: &'static str,
    /// Actual native-created directory, including non-UTF-8 paths, never a predicted parent/child.
    pub directory_unix_bytes: Option<&'a [u8]>,
    /// Original success evidence or explicit failed output; no automatic adoption or retry follows.
    pub evidence: Evidence,
}

/// Shared CLI/MCP shape: complete receipts retain their native fields without another wrapper.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Report<'a, T> {
    /// Complete unchanged native success receipt.
    Complete(&'a T),
    /// Native refusal or partial failure, preserving its locator and code.
    Failed(Failure<'a>),
    /// Full-result delivery failed; retain these original coordinates and evidence.
    Delivery(Delivery<'a>),
}

impl<T> Report<'_, T> {
    /// Whether native work or complete-result delivery failed; a fallback is never ordinary success.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::Complete(_))
    }
}

/// One pre-effect response reservation for native initialization, backup or detached restoration.
///
/// Pass the same `OutputLimits` to the native operation, invoke it exactly once, then select its
/// report here. This helper neither performs storage work nor guarantees transport delivery.
pub struct Reservation {
    limits: OutputLimits,
    maximum: usize,
    buffer: Vec<u8>,
}

impl Reservation {
    /// Prove the compact response fits before any native creation or acquisition.
    /// The native creator, not the caller's input spelling, enforces the directory byte bound.
    ///
    /// # Errors
    /// Refuses invalid limits, more than 16 MiB of response capacity, allocation failure or a
    /// budget unable to retain the worst-case lossless directory plus original success evidence.
    pub fn new(limits: OutputLimits, maximum: usize) -> Result<Self, &'static str> {
        limits.validate().map_err(|_| "store_output_limits")?;
        if maximum == 0 || maximum > 16 << 20 {
            return Err("store_output_response_limit");
        }
        let mut directory = Vec::new();
        directory
            .try_reserve_exact(limits.directory_bytes)
            .map_err(|_| "store_output_response_allocation")?;
        directory.resize(limits.directory_bytes, 255);
        let digest = Sha256Digest::of(&[]);
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(maximum)
            .map_err(|_| "store_output_response_allocation")?;
        for evidence in [
            Evidence::Initialized {
                header: JournalHeader {
                    schema_version: u32::MAX,
                    journal_id: [255; 32],
                    journal_epoch: u64::MAX,
                    writer_fence: u64::MAX,
                },
            },
            Evidence::Backup {
                manifest_sha256: digest,
            },
            Evidence::Restore {
                receipt_sha256: digest,
            },
            Evidence::Failed,
        ] {
            buffer.clear();
            let mut writer = LimitedWriter::new(&mut buffer, maximum);
            serde_json::to_writer(&mut writer, &delivery(Some(&directory), evidence))
                .map_err(|_| "store_output_response_limit")?;
        }
        buffer.clear();
        Ok(Self {
            limits,
            maximum,
            buffer,
        })
    }

    /// Select the fresh initialization result without replacing its header or directory.
    ///
    /// # Errors
    /// Preserves the original result if it violates the reserved native bounds.
    pub fn initialize<'a>(
        self,
        result: &'a Result<InitializationReceipt, OutputError>,
    ) -> Result<Report<'a, InitializationReceipt>, ContractError<'a, InitializationReceipt>> {
        self.finish(result, |receipt| {
            (
                &receipt.directory,
                Evidence::Initialized {
                    header: receipt.header,
                },
            )
        })
    }

    /// Select the backup result, retaining its original independent verification pin on overflow.
    ///
    /// # Errors
    /// Preserves the original result if it violates the reserved native bounds.
    pub fn backup<'a>(
        self,
        result: &'a Result<BackupReceipt, OutputError>,
    ) -> Result<Report<'a, BackupReceipt>, ContractError<'a, BackupReceipt>> {
        self.finish(result, |receipt| {
            (
                &receipt.directory,
                Evidence::Backup {
                    manifest_sha256: receipt.manifest_sha256,
                },
            )
        })
    }

    /// Select detached preparation evidence, never a live-store activation result.
    ///
    /// # Errors
    /// Preserves the original result if it violates the reserved native bounds.
    pub fn restore<'a>(
        self,
        result: &'a Result<RestoreReceipt, OutputError>,
    ) -> Result<Report<'a, RestoreReceipt>, ContractError<'a, RestoreReceipt>> {
        self.finish(result, |receipt| {
            (
                &receipt.directory,
                Evidence::Restore {
                    receipt_sha256: receipt.receipt_sha256,
                },
            )
        })
    }

    fn finish<'a, T: Serialize>(
        mut self,
        result: &'a Result<T, OutputError>,
        retained: impl FnOnce(&'a T) -> (&'a Path, Evidence),
    ) -> Result<Report<'a, T>, ContractError<'a, T>> {
        let (report, directory, evidence) = match result {
            Ok(receipt) => {
                let (directory, evidence) = retained(receipt);
                (Report::Complete(receipt), Some(directory), evidence)
            }
            Err(error) => (
                Report::Failed(Failure {
                    code: error.cause.code(),
                    directory_unix_bytes: error.directory.as_deref().map(path_bytes),
                }),
                error.directory.as_deref(),
                Evidence::Failed,
            ),
        };
        let fallback = delivery(directory.map(path_bytes), evidence);
        let violation = |mut delivery: Delivery<'a>| {
            delivery.code = "store_output_delivery_contract";
            ContractError { result, delivery }
        };
        if directory.is_some_and(|path| {
            self.limits
                .check_directory_bytes(path_bytes(path).len())
                .is_err()
        }) {
            return Err(violation(fallback));
        }
        let mut writer = LimitedWriter::new(&mut self.buffer, self.maximum);
        if serde_json::to_writer(&mut writer, &report).is_ok() {
            Ok(report)
        } else {
            self.buffer.clear();
            let mut writer = LimitedWriter::new(&mut self.buffer, self.maximum);
            if serde_json::to_writer(&mut writer, &fallback).is_err() {
                return Err(violation(fallback));
            }
            Ok(Report::Delivery(fallback))
        }
    }
}

/// A caller/native result violated the selected reservation; not an ordinary delivery fallback.
/// The result remains borrowed intact. Never truncate its locator or retry the operation.
#[derive(Debug)]
pub struct ContractError<'a, T> {
    /// Original complete native result, including any partial failure.
    pub result: &'a Result<T, OutputError>,
    /// Exact locator and original evidence, explicitly not promised to fit the violated budget.
    pub delivery: Delivery<'a>,
}

impl<T> std::fmt::Display for ContractError<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("native output exceeds the reserved delivery contract; retain original evidence without retry or adoption")
    }
}

impl<T: std::fmt::Debug> std::error::Error for ContractError<'_, T> {}

fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}

fn delivery(directory_unix_bytes: Option<&[u8]>, evidence: Evidence) -> Delivery<'_> {
    Delivery {
        code: "store_output_response_unavailable",
        directory_unix_bytes,
        evidence,
    }
}
