//! Configured documentation output through one already-held ordinary writer.

use std::path::PathBuf;

use gazette_sdk::{build, source, store};

use super::{Command, Failure, Kernel, Native, Selection, lock};
use crate::command::documentation;

/// Consume the single native documentation operation and materialize its retained assembly.
/// This neither acquires another writer nor closes, retries, repairs or certifies it.
pub struct MaterializeDocumentation<R> {
    /// Existing opaque ordinary writer handle.
    pub handle: String,
    /// Shared documentation sequence and supplied/local workspace selection.
    /// Fresh target Stage/Preparation retention is incompatible with configured output.
    pub documentation: documentation::Assemble,
    /// Exact caller-selected durable operation coordinate.
    pub id: source::Sha256Digest,
    /// Explicit peer roots protected in addition to native sources and held journal roots.
    pub protected: Vec<PathBuf>,
    /// Complete observed/proposed native output ceilings.
    pub limits: source::Limits,
    /// Independent output-write/history permissions and evidence ceiling.
    pub options: build::MaterializationOptions,
    /// Reserve full combined response custody under the held registry before any
    /// configuration, source, generator, journal append or output effect.
    pub reserve: R,
}

/// Exact reason configured output was never dispatched. Earlier generator effects,
/// when present, remain in the original documentation response rather than being erased.
#[derive(Debug)]
pub enum DocumentationMaterializationFailure {
    /// Shared permission or conflicting-retention selection refused before native work.
    Request(documentation::Failure),
    /// Original native pre-generator configuration/acquisition/assembly failure.
    Before(build::documentation_output::BeforeError),
    /// Original encoded documentation failure or compact fallback; not assembly success.
    Delivery,
    /// Successful native delivery omitted its retained assembly; no bytes are reconstructed.
    MissingAssembly,
    /// Independent filesystem/history permission refused before the documentation sequence.
    Permission(build::MaterializationError),
}

/// Exceptional serialization failure while retaining an already violated native contract.
/// Evidence is explicitly incomplete and unbounded, not a pre-effect or rollback claim.
#[derive(Debug)]
pub struct DocumentationContractError {
    /// Exact native custody-serialization error.
    pub cause: serde_json::Error,
    /// Original borrowed custody diagnostic, retained before its owning callback ends.
    /// This is not a substitute for the complete native serialized evidence.
    pub custody: String,
}

/// Native configured-output outcome, separate from the documentation operation's custody.
#[derive(Debug)]
pub enum DocumentationMaterializationOutcome {
    /// No configured-output plan or writer dispatch occurred.
    NotAttempted(DocumentationMaterializationFailure),
    /// Exact shared destination/history planning refusal after successful documentation delivery.
    PlanError(build::MaterializationError),
    /// Original intent/write/completion failure, including attempts, residue and read-back.
    WriteError(build::MaterializationError),
    /// Original durable receipt plus optional actual read-back; replay output remains absent.
    Materialized(Box<build::MaterializationResult>),
    /// Original native violated-contract evidence; no configured-output dispatch occurred.
    /// Prior generator effects are not denied, and no bounded carrier promise or retry follows.
    Contract {
        /// SDK-owned custody projection, not an interface reconstruction or success receipt.
        evidence: Result<serde_json::Value, DocumentationContractError>,
    },
}

/// Owned command custody while the same actual ordinary writer remains held.
pub struct DocumentationMaterializationReport {
    /// Original opaque session handle.
    pub session: String,
    /// True while the kernel retains the native writer, not proof it remains usable after failure.
    pub active: bool,
    /// Original acquired writer coordinates, not refreshed authority.
    pub acquired_header: Option<store::JournalHeader>,
    /// Exact requested operation, retained even if planning or generation refused.
    pub id: source::Sha256Digest,
    /// Original full/compact documentation JSON unchanged, including exact native jobs,
    /// retained stage paths and uncertainty. Absent only before delivery or on explicit Contract.
    pub documentation: Option<build::documentation_output::Encoded>,
    /// Native configured-output phase and full result or original failure.
    pub outcome: DocumentationMaterializationOutcome,
}

fn contract(
    custody: build::documentation_output::Custody<'_>,
) -> DocumentationMaterializationOutcome {
    DocumentationMaterializationOutcome::Contract {
        evidence: serde_json::to_value(custody).map_err(|cause| DocumentationContractError {
            cause,
            custody: format!("{custody:?}"),
        }),
    }
}

impl<R, E> Command for MaterializeDocumentation<R>
where
    R: FnOnce(&str, &Selection, Option<store::JournalHeader>) -> Result<(), E>,
{
    type Outcome = DocumentationMaterializationReport;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let Self {
            handle,
            documentation,
            id,
            protected,
            limits,
            options,
            reserve,
        } = self;
        let mut entries = lock(&kernel.store_session)?;
        let held = entries.get_mut(&handle).ok_or(Failure::Unknown)?;
        let writer = match &mut held.native {
            Native::Writer(writer) => writer,
            Native::Connection(_) => return Err(Failure::NotWriter),
        };
        reserve(&handle, &held.selection, held.acquired_header).map_err(Failure::Reservation)?;
        use DocumentationMaterializationFailure as F;
        use DocumentationMaterializationOutcome as O;
        let (response, outcome) = if !options.allow_write {
            (
                None,
                O::NotAttempted(F::Permission(build::MaterializationError::Permission)),
            )
        } else if !options.history.allow_record {
            (
                None,
                O::NotAttempted(F::Permission(build::MaterializationError::Store(
                    store::Error::MaterializationPermission,
                ))),
            )
        } else if let Err(error) = documentation.require_configured_output() {
            (None, O::NotAttempted(F::Request(error)))
        } else {
            documentation
                .consume_execution(|execution| match execution {
                    build::documentation_output::Execution::Before(error) => {
                        (None, O::NotAttempted(F::Before(error)))
                    }
                    build::documentation_output::Execution::Delivered {
                        response,
                        assembly,
                        ..
                    } => {
                        let outcome = if response.is_error {
                            O::NotAttempted(F::Delivery)
                        } else if let Some(assembly) = assembly {
                            match writer.plan_documentation_materialization(
                                assembly,
                                &protected,
                                limits,
                                options.evidence_bytes,
                            ) {
                                Ok(plan) => match writer.materialize(plan, id, options) {
                                    Ok(result) => O::Materialized(Box::new(result)),
                                    Err(error) => O::WriteError(error),
                                },
                                Err(error) => O::PlanError(error),
                            }
                        } else {
                            // No native assembly may be reconstructed from the encoded report.
                            O::NotAttempted(F::MissingAssembly)
                        };
                        (Some(response), outcome)
                    }
                    build::documentation_output::Execution::Contract { custody } => {
                        (None, contract(custody))
                    }
                    build::documentation_output::Execution::Complete { report, assembly } => {
                        // consume_execution always reserves delivery. Keep the real native
                        // completed facts if that invariant is ever violated; do not dispatch.
                        (
                            None,
                            contract(build::documentation_output::Custody::Complete {
                                jobs: report.jobs,
                                output_identity: assembly.output().inventory().identity(),
                                target: None,
                            }),
                        )
                    }
                })
                .unwrap_or_else(|error| (None, O::NotAttempted(F::Request(error))))
        };
        Ok(DocumentationMaterializationReport {
            session: handle,
            active: true,
            acquired_header: held.acquired_header,
            id,
            documentation: response,
            outcome,
        })
    }
}
