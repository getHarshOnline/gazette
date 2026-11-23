//! Pre-effect response reservation and bounded native encoding.

use super::*;

/// Pre-effect bounded carrier storage. Use the same bounds for native generation and staging.
pub struct Reservation {
    bounds: Bounds,
    maximum: usize,
    bytes: Vec<u8>,
}

impl Reservation {
    /// Prove the worst-case complete custody response before running any native effects.
    ///
    /// # Errors
    /// Refuses invalid native bounds, insufficient response capacity or allocation failure.
    /// The compact payload excludes large version text, preserving its invocation stdout digest.
    pub fn new(bounds: Bounds, maximum: usize) -> Result<Self, &'static str> {
        if bounds.jobs > 128
            || !(1..=1 << 20).contains(&bounds.directory_bytes)
            || !(1..=1 << 20).contains(&bounds.entry_bytes)
            || !(1..=16 << 20).contains(&maximum)
        {
            return Err("documentation_response_limits");
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum + 1)
            .map_err(|_| "documentation_response_allocation")?;
        let mut path = Vec::new();
        path.try_reserve_exact(bounds.directory_bytes)
            .map_err(|_| "documentation_response_allocation")?;
        path.resize(bounds.directory_bytes, 255);
        // Controls/backslashes are not admitted EntryPaths. Quotes are valid and require
        // the maximum JSON expansion of any admitted path byte; no lossy fake path is used.
        let entry: EntryPath = "\""
            .repeat(bounds.entry_bytes)
            .parse()
            .map_err(|_| "documentation_response_limits")?;
        let selections = [
            (
                Selection::File {
                    path: entry.clone(),
                },
                Content::File {
                    bytes: u64::MAX,
                    sha256: Sha256Digest::of(&[]),
                },
            ),
            (
                Selection::Directory { path: Some(entry) },
                Content::Directory,
            ),
            (Selection::Directory { path: None }, Content::Directory),
        ];
        let code = "\0".repeat(256);
        // Valid relative context quotes expand twice; static reasons may contain
        // controls, whose JSON escaping expands each byte up to six times.
        let diagnostic_context = "\"".repeat(TARGET_CONTEXT_BYTES);
        let diagnostic_reason = "\0".repeat(TARGET_REASON_BYTES);
        let identity = gazette_core::source::InventoryBuilder::new(Default::default())
            .finish()
            .identity();
        let invocation = generator::Invocation {
            command_sha256: Sha256Digest::of(&[]),
            exit: Some(process::Exit {
                code: Some(i32::MIN),
            }),
            stdout_sha256: Some(Sha256Digest::of(&[])),
            stdout_bytes: Some(usize::MAX),
        };
        let validation = generator::Validation {
            tree: identity,
            invocation: invocation.clone(),
        };
        let header = store::JournalHeader {
            schema_version: u32::MAX,
            journal_id: [255; 32],
            journal_epoch: u64::MAX,
            writer_fence: u64::MAX,
        };
        let id = Sha256Digest::of(&[]);
        let retention = store::PreparationRetention {
            intent_id: id,
            output: identity,
            entries: u64::MAX,
            bytes: u64::MAX,
            recorded: header,
        };
        let commit: crate::source::CommitId = "ffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .map_err(|_| "documentation_response_contract")?;
        let completion = gazette_core::store::PreparationCommit {
            report: gazette_core::release::CommitReport {
                commit: commit.clone(),
                parent: Some(commit),
                output: identity,
                entries: usize::MAX,
                bytes: u64::MAX,
            },
            recorded: header,
        };
        let mut selected = Self {
            bounds,
            maximum,
            bytes,
        };
        for (selection, content) in &selections {
            let job = Job {
                module: usize::MAX,
                reference: usize::MAX,
                input_unix_bytes: &path,
                output_unix_bytes: &path,
                input_identity: identity,
                output_identity: identity,
                selection,
                content: *content,
                version: Some(&invocation),
                generation: Some(&invocation),
                validation: Some(&validation),
            };
            let witness = Compact {
                code: "documentation_response_unavailable",
                completed: Jobs::Witness(&job, bounds.jobs),
                retained_unix_bytes: Paths::Witness(&path, bounds.jobs * 2),
                failure: Failure {
                    code: Some(&code),
                    target_diagnostic: Some(TargetDiagnostic::new(
                        &diagnostic_context,
                        &diagnostic_reason,
                    )),
                    generator_phase: Some(generator::Phase::Generation),
                    process_failure: Some(process::Failure::ExecutableChanged),
                    cleanup: Some(process::Cleanup::NotStarted),
                    input_unix_bytes: Some(&path),
                    output_unix_bytes: Some(&path),
                    input_identity: Some(identity),
                    version: Some(&invocation),
                    generation: Some(&invocation),
                    validation: Some(&validation),
                    residue_unix_bytes: Some(&path),
                    residue_cause: Some(&code),
                    residue_cleanup: Some(&code),
                },
                output_identity: Some(identity),
                target_unix_bytes: Some(&path),
                preparation: Some(PreparationCustody {
                    journal_unix_bytes: &path,
                    input_expected_header: header,
                    id,
                    receipt: Some(RecoveryPin {
                        inserted: false,
                        journal: header,
                        id,
                        created: header,
                        output: identity,
                        evidence_sha256: id,
                        retention: Some(&retention),
                        check: Some(crate::release_commit::journal_output::CheckPin {
                            evidence_sha256: id,
                            recorded: header,
                        }),
                        worktree: Some(crate::release_commit::journal_output::WorktreePin {
                            created: header,
                            check_sha256: Some(id),
                            completion: Some(&completion),
                        }),
                    }),
                    journal: Some(header),
                    writer_acquired: false,
                    generation: Some(store::GenerationEffect::NotAttempted),
                    record: Some(store::PreparationEffect::NotAttempted),
                    stage_unix_bytes: Some(&path),
                }),
            };
            selected
                .encode(&witness)
                .map_err(|_| "documentation_response_limit")?;
        }
        selected.bytes.clear();
        Ok(selected)
    }

    fn encode(&mut self, value: &impl Serialize) -> Result<(), serde_json::Error> {
        self.bytes.clear();
        serde_json::to_writer(LimitedWriter::new(&mut self.bytes, self.maximum), value)
    }

    /// Encode a complete report when available; otherwise retain compact original custody.
    /// A compact delivery is always an error, even if native assembly itself completed.
    ///
    /// # Errors
    /// A violated native bound or target binding retains the exact original custody and full
    /// report in `ContractError`. Such a violation is not promised to fit the response budget.
    pub fn finish<'a, T: Serialize>(
        mut self,
        full: Option<&'a T>,
        custody: Custody<'a>,
    ) -> Result<Encoded, ContractError<'a, T>> {
        let summary = compact(custody);
        if !self.accepts(custody, &summary) {
            return Err(ContractError { full, custody });
        }
        if let Some(full) = full
            && matches!(
                custody,
                Custody::Complete { .. } | Custody::Preparation { result: Ok(_), .. }
            )
            && self.encode(full).is_ok()
        {
            return Ok(Encoded {
                bytes: self.bytes,
                is_error: false,
            });
        }
        if self.encode(&summary).is_err() {
            return Err(ContractError { full, custody });
        }
        Ok(Encoded {
            bytes: self.bytes,
            is_error: true,
        })
    }

    fn accepts(&self, custody: Custody<'_>, summary: &Compact<'_>) -> bool {
        let path_ok = |path: &[u8]| path.len() <= self.bounds.directory_bytes;
        if custody.jobs().len() > self.bounds.jobs
            || custody.retained().len() > self.bounds.jobs * 2
            || custody.retained().iter().any(|path| !path_ok(raw(path)))
        {
            return false;
        }
        for job in custody.jobs() {
            if !matches!(
                (&job.selection, job.content),
                (Selection::File { .. }, Content::File { .. })
                    | (Selection::Directory { .. }, Content::Directory)
            ) || !path_ok(raw(&job.input))
                || !path_ok(raw(&job.output))
                || job.selection.path().map_or(0, |path| path.as_str().len())
                    > self.bounds.entry_bytes
            {
                return false;
            }
        }
        let failure = &summary.failure;
        if let Some(diagnostic) = &failure.target_diagnostic
            && (diagnostic.context_omitted != diagnostic.context.is_none()
                || diagnostic.reason_omitted != diagnostic.reason.is_none()
                || diagnostic.context.is_some_and(|context| {
                    context.len() > TARGET_CONTEXT_BYTES
                        || context
                            .parse::<gazette_core::source::Subdirectory>()
                            .is_err()
                })
                || diagnostic
                    .reason
                    .is_some_and(|reason| reason.len() > TARGET_REASON_BYTES))
        {
            return false;
        }
        if let Some(preparation) = &summary.preparation {
            if !path_ok(preparation.journal_unix_bytes)
                || preparation
                    .stage_unix_bytes
                    .is_some_and(|path| !path_ok(path))
            {
                return false;
            }
            if let Custody::Preparation {
                selection,
                result,
                output_identity,
                ..
            } = custody
            {
                let receipt = match result {
                    Ok(receipt) => Some(receipt),
                    Err(error) => error.receipt.as_deref(),
                };
                if receipt.is_some_and(|receipt| {
                    receipt.record.intent.id != selection.id
                        || receipt.record.intent.output != output_identity
                }) {
                    return false;
                }
            }
        }
        if [failure.code, failure.residue_cause, failure.residue_cleanup]
            .into_iter()
            .flatten()
            .any(|code| code.len() > 256)
            || [
                failure.input_unix_bytes,
                failure.output_unix_bytes,
                failure.residue_unix_bytes,
                summary.target_unix_bytes,
            ]
            .into_iter()
            .flatten()
            .any(|path| !path_ok(path))
        {
            return false;
        }
        !matches!(custody,Custody::Complete {output_identity,target:Some(target),..}
            if output_identity!=target.inventory().identity())
    }
}

/// Complete compact JSON with capacity reserved for one extra transport newline.
pub struct Encoded {
    /// Full report or compact custody; never a successful partial result.
    pub bytes: Vec<u8>,
    /// True for native failure or compact fallback.
    pub is_error: bool,
}

/// Explicit broken caller/native contract; no original effect evidence is discarded.
#[derive(Debug)]
pub struct ContractError<'a, T> {
    /// Original full report when supplied; serialization may itself be unsupported.
    pub full: Option<&'a T>,
    /// Original typed native results, not a reduced or newly observed replacement.
    pub custody: Custody<'a>,
}
