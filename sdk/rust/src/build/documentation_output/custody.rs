//! Original native custody and compact failure projection.

use super::*;

mod reservation;
pub use reservation::{ContractError, Encoded, Reservation};

/// Match the complete native generation/staging selection before any effects.
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    /// Complete selected job ceiling, at most 128; zero admits imported-only assembly.
    pub jobs: usize,
    /// Exact canonical Unix stage-path bytes, independently enforced before native creation.
    pub directory_bytes: usize,
    /// Exact selected relative path bytes, enforced before generation; root selection uses zero.
    pub entry_bytes: usize,
}

/// Borrow original native results; no field here grants retention or publication authority.
#[derive(Clone, Copy, Debug)]
pub enum Custody<'a> {
    /// Original native preparation outcome, alongside every completed generator job.
    Preparation {
        /// All completed native generator jobs, including those preceding a preparation failure.
        jobs: &'a [DocumentationJob],
        /// Exact assembled provider-input tree, not hosted HTML.
        output_identity: TreeIdentity,
        /// Original caller-selected journal coordinates and local permissions.
        selection: &'a PreparationSelection,
        /// Original native receipt or error, including partial phase custody.
        result: &'a Result<store::PreparationReceipt, super::PreparationError>,
    },
    /// Completed prefix and active failure from the generator application owner.
    GenerationFailed(&'a DocumentationGenerationError),
    /// Generation completed, but target assembly failed.
    AssemblyFailed {
        /// Exact completed native jobs.
        jobs: &'a [DocumentationJob],
        /// Original target failure.
        error: &'a DocumentationError,
    },
    /// An assembled target was not successfully retained.
    StageFailed {
        /// Exact completed native jobs.
        jobs: &'a [DocumentationJob],
        /// Complete assembled inventory identity, not proof of a successful stage.
        output_identity: TreeIdentity,
        /// Original stage failure, including any unverified residual path.
        error: &'a stage::Error,
    },
    /// Complete local assembly, optionally retained by the native stage owner.
    Complete {
        /// Exact completed native jobs; imports have no synthetic job.
        jobs: &'a [DocumentationJob],
        /// Original assembled inventory identity.
        output_identity: TreeIdentity,
        /// Actual retention receipt, never a configured destination or predicted path.
        target: Option<&'a stage::RetainedStage>,
    },
}

impl<'a> Custody<'a> {
    fn jobs(self) -> &'a [DocumentationJob] {
        match self {
            Self::GenerationFailed(error) => &error.completed,
            Self::AssemblyFailed { jobs, .. }
            | Self::Preparation { jobs, .. }
            | Self::StageFailed { jobs, .. }
            | Self::Complete { jobs, .. } => jobs,
        }
    }

    fn retained(self) -> &'a [PathBuf] {
        match self {
            Self::GenerationFailed(error) => &error.retained,
            _ => &[],
        }
    }
}

impl Serialize for Custody<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Reuse the native lossless phase projection even when a caller/native
        // invariant broke the reservation. Serialization preserves evidence;
        // it does not promise that this violated result fits a bounded carrier.
        let mut evidence = compact(*self);
        evidence.code = "documentation_response_contract";
        evidence.serialize(serializer)
    }
}

#[derive(Serialize, JsonSchema)]
struct Job<'a> {
    #[schemars(range(max = usize::MAX))]
    module: usize,
    #[schemars(range(max = usize::MAX))]
    reference: usize,
    input_unix_bytes: &'a [u8],
    output_unix_bytes: &'a [u8],
    input_identity: TreeIdentity,
    output_identity: TreeIdentity,
    selection: &'a Selection,
    content: Content,
    version: Option<&'a generator::Invocation>,
    generation: Option<&'a generator::Invocation>,
    validation: Option<&'a generator::Validation>,
}

impl<'a> From<&'a DocumentationJob> for Job<'a> {
    fn from(job: &'a DocumentationJob) -> Self {
        Self {
            module: job.module,
            reference: job.reference,
            input_unix_bytes: raw(&job.input),
            output_unix_bytes: raw(&job.output),
            input_identity: job.evidence.input,
            output_identity: job.output_identity,
            selection: &job.selection,
            content: job.content,
            version: job.evidence.version.as_ref(),
            generation: job.evidence.generation.as_ref(),
            validation: job.evidence.validation.as_ref(),
        }
    }
}

enum Jobs<'a> {
    Native(&'a [DocumentationJob]),
    Witness(&'a Job<'a>, usize),
}
impl Serialize for Jobs<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let length = match self {
            Self::Native(jobs) => jobs.len(),
            Self::Witness(_, n) => *n,
        };
        let mut sequence = serializer.serialize_seq(Some(length))?;
        match self {
            Self::Native(jobs) => {
                for job in *jobs {
                    sequence.serialize_element(&Job::from(job))?;
                }
            }
            Self::Witness(job, n) => {
                for _ in 0..*n {
                    sequence.serialize_element(job)?;
                }
            }
        }
        sequence.end()
    }
}

enum Paths<'a> {
    Native(&'a [PathBuf]),
    Witness(&'a [u8], usize),
}
impl Serialize for Paths<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let length = match self {
            Self::Native(paths) => paths.len(),
            Self::Witness(_, n) => *n,
        };
        let mut sequence = serializer.serialize_seq(Some(length))?;
        match self {
            Self::Native(paths) => {
                for path in *paths {
                    sequence.serialize_element(raw(path))?;
                }
            }
            Self::Witness(path, n) => {
                for _ in 0..*n {
                    sequence.serialize_element(path)?;
                }
            }
        }
        sequence.end()
    }
}

const TARGET_CONTEXT_BYTES: usize = 1024;
const TARGET_REASON_BYTES: usize = 256;

/// Fixed diagnostic bounds are included in the pre-effect custody reservation.
/// Context is a logical input name, never an artifact body or filesystem lookup.
#[derive(Serialize, JsonSchema)]
struct TargetDiagnostic<'a> {
    context: Option<&'a str>,
    context_omitted: bool,
    reason: Option<&'a str>,
    reason_omitted: bool,
}

impl<'a> TargetDiagnostic<'a> {
    fn new(context: &'a str, reason: &'a str) -> Self {
        let safe_context = (context.len() <= TARGET_CONTEXT_BYTES
            && context
                .parse::<gazette_core::source::Subdirectory>()
                .is_ok())
        .then_some(context);
        let bounded_reason = (reason.len() <= TARGET_REASON_BYTES).then_some(reason);
        Self {
            context: safe_context,
            context_omitted: safe_context.is_none(),
            reason: bounded_reason,
            reason_omitted: bounded_reason.is_none(),
        }
    }
}

#[derive(Default, Serialize, JsonSchema)]
struct Failure<'a> {
    code: Option<&'a str>,
    target_diagnostic: Option<TargetDiagnostic<'a>>,
    generator_phase: Option<generator::Phase>,
    process_failure: Option<process::Failure>,
    cleanup: Option<process::Cleanup>,
    input_unix_bytes: Option<&'a [u8]>,
    output_unix_bytes: Option<&'a [u8]>,
    input_identity: Option<TreeIdentity>,
    version: Option<&'a generator::Invocation>,
    generation: Option<&'a generator::Invocation>,
    validation: Option<&'a generator::Validation>,
    residue_unix_bytes: Option<&'a [u8]>,
    residue_cause: Option<&'a str>,
    residue_cleanup: Option<&'a str>,
}

impl<'a> Failure<'a> {
    fn target(&mut self, error: &'a DocumentationError) {
        self.code = Some(error.code());
        if let DocumentationError::Target(gazette_core::documentation::Error::Invalid {
            context,
            reason,
        }) = error
        {
            self.target_diagnostic = Some(TargetDiagnostic::new(context, reason));
        }
    }

    fn stage(&mut self, error: &'a stage::Error) {
        if let stage::Error::Residue {
            cause,
            path,
            cleanup,
        } = error
        {
            self.residue_unix_bytes = Some(raw(path));
            self.residue_cause = Some(cause.code());
            self.residue_cleanup = cleanup.as_ref().map(|error| error.code());
        }
    }
    fn process(&mut self, error: &process::Error) {
        self.process_failure = Some(error.failure);
        self.cleanup = Some(error.cleanup);
    }
}

#[derive(Serialize, JsonSchema)]
pub(super) struct Compact<'a> {
    code: &'static str,
    #[schemars(with = "Vec<Job<'a>>")]
    completed: Jobs<'a>,
    #[schemars(with = "Vec<Vec<u8>>")]
    retained_unix_bytes: Paths<'a>,
    failure: Failure<'a>,
    output_identity: Option<TreeIdentity>,
    target_unix_bytes: Option<&'a [u8]>,
    preparation: Option<PreparationCustody<'a>>,
}

/// Compact native phase custody reuses the shared preparation receipt pin.
#[derive(Serialize, JsonSchema)]
struct PreparationCustody<'a> {
    journal_unix_bytes: &'a [u8],
    input_expected_header: store::JournalHeader,
    id: Sha256Digest,
    receipt: Option<RecoveryPin<'a>>,
    journal: Option<store::JournalHeader>,
    writer_acquired: bool,
    generation: Option<store::GenerationEffect>,
    record: Option<store::PreparationEffect>,
    stage_unix_bytes: Option<&'a [u8]>,
}

fn compact(custody: Custody<'_>) -> Compact<'_> {
    let mut output = Compact {
        code: "documentation_response_unavailable",
        completed: Jobs::Native(custody.jobs()),
        retained_unix_bytes: Paths::Native(custody.retained()),
        failure: Failure::default(),
        output_identity: None,
        target_unix_bytes: None,
        preparation: None,
    };
    match custody {
        Custody::Preparation {
            output_identity,
            selection,
            result,
            ..
        } => {
            output.output_identity = Some(output_identity);
            let mut preparation = PreparationCustody {
                journal_unix_bytes: raw(&selection.journal),
                input_expected_header: selection.expected,
                id: selection.id,
                receipt: None,
                journal: None,
                writer_acquired: false,
                generation: None,
                record: None,
                stage_unix_bytes: None,
            };
            match result {
                Ok(receipt) => {
                    preparation.receipt = Some(RecoveryPin::from(receipt));
                    preparation.journal = Some(receipt.journal);
                    preparation.writer_acquired = true;
                }
                Err(error) => {
                    preparation.receipt = error.receipt.as_deref().map(RecoveryPin::from);
                    preparation.journal = error.journal;
                    preparation.writer_acquired = error.writer_acquired;
                    preparation.stage_unix_bytes = error.stage.as_deref().map(raw);
                    use super::PreparationFailure as F;
                    output.failure.code = Some(match error.cause.as_ref() {
                        F::Writer(error) => {
                            preparation.generation = Some(error.generation);
                            error.cause.code()
                        }
                        F::Record(error) => {
                            preparation.record = Some(error.effect);
                            error.cause.code()
                        }
                        F::Journal(error) => error.code(),
                        F::Stage(error) => {
                            output.failure.stage(error);
                            "documentation_preparation_stage"
                        }
                        F::Preview => "documentation_preparation_preview",
                        F::Selection => "documentation_preparation_selection",
                        F::Deployability(error) => error.code(),
                        F::Permission => "documentation_preparation_permission",
                        F::Evidence(_) => "documentation_preparation_evidence",
                        F::Separation(_) => "documentation_preparation_separation",
                        F::Replay => "documentation_preparation_replay",
                        F::Mismatch => "documentation_preparation_mismatch",
                        F::Disclosure(_) | F::Policy(_) | F::Qualification => {
                            "documentation_preparation_unexpected_check"
                        }
                    });
                }
            }
            output.preparation = Some(preparation);
        }
        Custody::GenerationFailed(error) => {
            output.failure.code = Some(error.code());
            match &error.cause {
                DocumentationGenerationCause::Process(error) => output.failure.process(error),
                DocumentationGenerationCause::Stage(error) => output.failure.stage(error),
                DocumentationGenerationCause::Generator(error) => {
                    output.failure.generator_phase = Some(error.phase);
                    output.failure.input_unix_bytes = Some(raw(&error.input));
                    output.failure.output_unix_bytes = Some(raw(&error.output));
                    output.failure.input_identity = Some(error.evidence.input);
                    output.failure.version = error.evidence.version.as_ref();
                    output.failure.generation = error.evidence.generation.as_ref();
                    output.failure.validation = error.evidence.validation.as_ref();
                    if let generator::Cause::Process(error) = &error.cause {
                        output.failure.process(error);
                    }
                }
                DocumentationGenerationCause::Selection(_)
                | DocumentationGenerationCause::Binding(_)
                | DocumentationGenerationCause::Limit => (),
            }
        }
        Custody::AssemblyFailed { error, .. } => output.failure.target(error),
        Custody::StageFailed {
            output_identity,
            error,
            ..
        } => {
            output.output_identity = Some(output_identity);
            output.failure.code = Some("documentation_stage");
            output.failure.stage(error);
        }
        Custody::Complete {
            output_identity,
            target,
            ..
        } => {
            output.output_identity = Some(output_identity);
            output.target_unix_bytes = target.map(|target| raw(target.path()));
        }
    }
    output
}

fn raw(path: &Path) -> &[u8] {
    path.as_os_str().as_bytes()
}
