//! Explicit native typesetting before pure compilation; no renderer launches a process.

use std::{collections::BTreeMap, fmt};

use gazette_compiler::corpus;
use gazette_core::{
    json,
    markdown::{Identity, Math, MathBinding},
    process::{self, CaptureRequest, Command, Exit, Request},
    source::{EntryPath, Sha256Digest, TreeIdentity},
};
use gazette_ports::process::RetainedCaptureRunner;
use serde::Serialize;

use super::{AcquiredInputs, ParsedPublication, PublicationInputError, serialization::encode};

const FORMULAS: usize = 4096;
const BATCH_FORMULAS: usize = 256;
const INPUT_BYTES: usize = 2 << 20;
const TEX_BYTES: usize = 8 << 10;
const FRAGMENT_BYTES: usize = 128 << 10;
const BATCH_OUTPUT_BYTES: usize = 32 << 20;
const OUTPUT_BYTES: usize = 64 << 20;

/// Invocation permission and finite whole-operation bounds, not resource selection.
#[derive(Clone, Copy, Debug)]
pub struct MathGenerationOptions {
    /// Native execution acknowledgement, per-batch stdin ceiling and deadline.
    pub execution: process::Options,
    /// Maximum emitted formulas across the complete corpus; at most 4096.
    pub formulas: usize,
    /// Maximum aggregate complete stdout bytes; at most 64 MiB.
    pub output_bytes: usize,
}

impl Default for MathGenerationOptions {
    fn default() -> Self {
        Self {
            execution: process::Options {
                max_input_bytes: INPUT_BYTES,
                ..Default::default()
            },
            formulas: FORMULAS,
            output_bytes: OUTPUT_BYTES,
        }
    }
}

/// Private retained candidates and exact process observations, never publication approval.
/// Interpreter identity, retained renderer/import identity and entrypoint are independent.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct MathCapture {
    workspace_sha256: Sha256Digest,
    resource_tree: TreeIdentity,
    resource_entrypoint: EntryPath,
    results: BTreeMap<EntryPath, Vec<corpus::LocatedMathResult>>,
    invocations: Vec<Invocation>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
struct Invocation {
    command_sha256: Sha256Digest,
    request: String,
    response: String,
    exit: Exit,
}

#[derive(Debug)]
struct Attempt {
    command_sha256: Sha256Digest,
    request: String,
    response: Option<Vec<u8>>,
    exit: Option<Exit>,
}

/// Typed refusal; native custody survives without exposing formula text in diagnostics.
#[derive(Debug)]
pub enum MathGenerationCause {
    /// Input parsing failed before generation could be planned.
    Input(Box<PublicationInputError>),
    /// The acquired input already owns a capture; rerunning is not an implicit retry.
    AlreadyCaptured,
    /// Emitted math has no explicitly configured typesetter.
    Unconfigured,
    /// The selected typesetter cannot be bound from retained machine/workspace roots.
    Binding(crate::configuration::RuntimeBindingError),
    /// A named operation ceiling was exceeded before complete success.
    Limit(&'static str),
    /// Finite JSON encoding failed before an invocation.
    Encoding,
    /// Native validation/execution failed, retaining direct-child custody.
    Native(process::Error),
    /// Complete native capture returned a nonzero or abnormal exit.
    Exit(Exit),
    /// Complete stdout was not UTF-8 or did not match the exact response shape/count.
    Response,
}

/// Failed operation with a complete observed prefix and any active invocation.
/// There is no successful result subset and no automatic retry. Failed native
/// captures expose no stdout prefix; a complete invalid response remains private evidence.
#[derive(Debug)]
pub struct MathGenerationError {
    cause: MathGenerationCause,
    completed: Vec<Invocation>,
    active: Option<Attempt>,
}

impl MathGenerationError {
    /// Preserve a pre-generation source failure without any process observation.
    pub fn input(error: PublicationInputError) -> Self {
        Self::before(MathGenerationCause::Input(Box::new(error)))
    }

    fn before(cause: MathGenerationCause) -> Self {
        Self {
            cause,
            completed: Vec::new(),
            active: None,
        }
    }

    /// Exact typed cause, including native direct-child cleanup when applicable.
    pub fn cause(&self) -> &MathGenerationCause {
        &self.cause
    }

    /// Completed request/response pairs, command identities and exits, in invocation order.
    pub fn completed(&self) -> impl Iterator<Item = (&str, &str, Sha256Digest, Exit)> {
        evidence(&self.completed)
    }

    /// Exact active request; absence means failure preceded invocation planning.
    pub fn active_request(&self) -> Option<&str> {
        self.active.as_ref().map(|job| job.request.as_str())
    }

    /// Complete observed bytes, including invalid UTF-8; never a truncated native prefix.
    pub fn active_response(&self) -> Option<&[u8]> {
        self.active.as_ref().and_then(|job| job.response.as_deref())
    }

    /// Native exit when capture completed; otherwise consult the typed native cleanup.
    pub fn active_exit(&self) -> Option<Exit> {
        self.active.as_ref().and_then(|job| job.exit)
    }

    /// Selected command identity for the active job, not interpreter-import attestation.
    pub fn active_command_sha256(&self) -> Option<Sha256Digest> {
        self.active.as_ref().map(|job| job.command_sha256)
    }

    /// Stable diagnostic without source text, command paths or process output.
    pub fn code(&self) -> &'static str {
        match &self.cause {
            MathGenerationCause::Input(error) => error.code(),
            MathGenerationCause::AlreadyCaptured => "math_already_captured",
            MathGenerationCause::Unconfigured => "math_unconfigured",
            MathGenerationCause::Binding(error) => error.code(),
            MathGenerationCause::Limit(_) => "math_limit",
            MathGenerationCause::Encoding => "math_encoding",
            MathGenerationCause::Native(error) => error.code(),
            MathGenerationCause::Exit(_) => "math_exit",
            MathGenerationCause::Response => "math_response",
        }
    }
}

impl fmt::Display for MathGenerationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "math generation {}; {} completed invocations; no complete capture",
            self.code(),
            self.completed.len()
        )?;
        if let MathGenerationCause::Input(input) = &self.cause
            && let Some(limit) = input.source_bytes_limit()
        {
            write!(f, "; before math execution: {limit}")?;
        }
        if let MathGenerationCause::Binding(error) = &self.cause {
            write!(f, "; {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for MathGenerationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.cause {
            MathGenerationCause::Input(error) => Some(error.as_ref()),
            MathGenerationCause::Native(error) => Some(error),
            MathGenerationCause::Binding(error) => Some(error),
            _ => None,
        }
    }
}

/// A retained capture does not belong to these exact workspace/body inputs.
#[derive(Debug)]
pub enum MathBindingError {
    /// Whole workspace identity changed; no path is fabricated for this mismatch.
    Workspace,
    /// Complete corpus/result binding failed at its native source owner.
    Corpus(corpus::Error),
}

impl MathBindingError {
    /// Stable binding failure classification.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Workspace => "math_workspace",
            Self::Corpus(error) => error.code(),
        }
    }
}

impl fmt::Display for MathBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "retained math binding failed: {}", self.code())
    }
}

impl std::error::Error for MathBindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Workspace => None,
            Self::Corpus(error) => Some(error),
        }
    }
}

impl MathCapture {
    /// Resolve complete native formula facts into retained candidates using one selected command.
    /// All requests and input limits are checked before the first process. Each batch
    /// shares the native capture deadline; stdout is bounded per batch and across the run.
    /// No renderer, source parser or template performs this acquisition implicitly.
    ///
    /// # Errors
    /// Preserve complete prior invocations and the failed active job; never return a
    /// successful subset, retry, silently replace a capture or fall back to another renderer.
    pub fn generate(
        publication: &ParsedPublication<'_>,
        runner: &dyn RetainedCaptureRunner,
        options: MathGenerationOptions,
    ) -> Result<Option<Self>, MathGenerationError> {
        if publication.inputs().math_capture().is_some() {
            return Err(MathGenerationError::before(
                MathGenerationCause::AlreadyCaptured,
            ));
        }
        let formulas = formulas(publication, options).map_err(MathGenerationError::before)?;
        if formulas.is_empty() {
            return Ok(None);
        }
        let workspace = publication.inputs().compilation();
        let typesetter = workspace
            .workspace
            .math_typesetter()
            .ok_or_else(|| MathGenerationError::before(MathGenerationCause::Unconfigured))?;
        let resources = publication
            .inputs()
            .sources()
            .get(&typesetter.source)
            .ok_or_else(|| MathGenerationError::before(MathGenerationCause::Unconfigured))?;
        let resource_tree = resources.inventory().identity();
        let resource_input = resources.as_input().map_err(|_| {
            MathGenerationError::before(MathGenerationCause::Native(process::Error::before_start(
                process::Failure::Command,
            )))
        })?;
        if resource_input.file(&typesetter.entrypoint).is_none() {
            return Err(MathGenerationError::before(MathGenerationCause::Native(
                process::Error::before_start(process::Failure::Command),
            )));
        }
        let command = publication
            .inputs()
            .configuration()
            .math_command()
            .map_err(|error| MathGenerationError::before(MathGenerationCause::Binding(error)))?
            .ok_or_else(|| MathGenerationError::before(MathGenerationCause::Unconfigured))?;
        command
            .validate()
            .map_err(|error| MathGenerationError::before(MathGenerationCause::Native(error)))?;
        if command.executable.is_none() {
            return Err(MathGenerationError::before(MathGenerationCause::Native(
                process::Error::before_start(process::Failure::Executable),
            )));
        }
        let command_bytes = encode(command, 512 << 10, "math command bytes")
            .map_err(|_| MathGenerationError::before(MathGenerationCause::Encoding))?;
        let command_sha256 = Sha256Digest::of(&command_bytes);
        let mut capture = Self {
            workspace_sha256: workspace.source_sha256,
            resource_tree,
            resource_entrypoint: typesetter.entrypoint.clone(),
            results: BTreeMap::new(),
            invocations: Vec::new(),
        };
        let jobs = jobs(&formulas, command, options).map_err(MathGenerationError::before)?;
        let mut remaining = options.output_bytes;
        for job in jobs {
            let mut active = Attempt {
                command_sha256,
                request: job.request,
                response: None,
                exit: None,
            };
            let maximum = remaining.min(BATCH_OUTPUT_BYTES);
            let resources = match resources.as_input() {
                Ok(resources) => resources,
                Err(_) => {
                    return Err(MathGenerationError {
                        cause: MathGenerationCause::Native(process::Error::before_start(
                            process::Failure::Command,
                        )),
                        completed: capture.invocations,
                        active: Some(active),
                    });
                }
            };
            let outcome = invoke(
                runner,
                command,
                resources,
                &typesetter.entrypoint,
                options.execution,
                maximum,
                &mut active,
            )
            .and_then(|()| {
                response(
                    active.response.as_deref().unwrap_or_default(),
                    job.formulas.len(),
                )
            });
            let values = match outcome {
                Ok(values) => values,
                Err(cause) => {
                    return Err(MathGenerationError {
                        cause,
                        completed: capture.invocations,
                        active: Some(active),
                    });
                }
            };
            for (formula, mathml) in job.formulas.iter().zip(values) {
                let fragments = capture.results.entry(formula.path.clone()).or_default();
                let binding = MathBinding {
                    occurrence: formula.occurrence.clone(),
                    mathml,
                };
                // The ordered corpus traversal keeps each fragment contiguous,
                // including occurrences split across bounded native batches.
                if let Some(fragment) = fragments.last_mut()
                    && fragment.location == formula.location
                {
                    fragment.result.bindings.push(binding);
                } else {
                    fragments.push(corpus::LocatedMathResult {
                        location: formula.location,
                        result: corpus::MathResult {
                            identity: formula.identity,
                            bindings: vec![binding],
                        },
                    });
                }
            }
            // Preserve evidence even if an internal invariant unexpectedly fails.
            let Some(output) = active.response.take() else {
                return Err(MathGenerationError {
                    cause: MathGenerationCause::Response,
                    completed: capture.invocations,
                    active: Some(active),
                });
            };
            remaining -= output.len();
            let response = match String::from_utf8(output) {
                Ok(response) => response,
                Err(error) => {
                    active.response = Some(error.into_bytes());
                    return Err(MathGenerationError {
                        cause: MathGenerationCause::Response,
                        completed: capture.invocations,
                        active: Some(active),
                    });
                }
            };
            let Some(exit) = active.exit else {
                active.response = Some(response.into_bytes());
                return Err(MathGenerationError {
                    cause: MathGenerationCause::Response,
                    completed: capture.invocations,
                    active: Some(active),
                });
            };
            capture.invocations.push(Invocation {
                command_sha256,
                request: active.request,
                response,
                exit,
            });
        }
        Ok(Some(capture))
    }

    /// Whole configuration identity from which the native command and corpus were selected.
    pub fn workspace_sha256(&self) -> Sha256Digest {
        self.workspace_sha256
    }

    /// Exact retained renderer and import tree used for acquisition.
    pub fn resource_tree(&self) -> TreeIdentity {
        self.resource_tree
    }

    /// Exact entrypoint within the retained resource tree.
    pub fn resource_entrypoint(&self) -> &EntryPath {
        &self.resource_entrypoint
    }

    /// Exact successful requests/responses, command identities and exits, in invocation order.
    pub fn invocations(&self) -> impl Iterator<Item = (&str, &str, Sha256Digest, Exit)> {
        evidence(&self.invocations)
    }

    /// Bind candidates to exact current bodies without executing or reopening any input.
    /// Structural `MathML` admission remains with the Markdown adapter before formatting.
    ///
    /// # Errors
    /// Refuse changed workspace identity or any missing/extra/stale corpus binding.
    pub fn bind<'s>(
        &self,
        inputs: &AcquiredInputs,
        corpus: corpus::ParsedCorpus<'s>,
    ) -> Result<corpus::ParsedCorpus<'s>, MathBindingError> {
        if inputs.compilation().source_sha256 != self.workspace_sha256 {
            return Err(MathBindingError::Workspace);
        }
        corpus
            .with_math(self.results.clone())
            .map_err(MathBindingError::Corpus)
    }
}

fn evidence(values: &[Invocation]) -> impl Iterator<Item = (&str, &str, Sha256Digest, Exit)> {
    values.iter().map(|job| {
        (
            job.request.as_str(),
            job.response.as_str(),
            job.command_sha256,
            job.exit,
        )
    })
}

struct Formula<'a> {
    path: &'a EntryPath,
    location: corpus::MathLocation,
    identity: Identity,
    occurrence: &'a Math,
}

fn formulas<'a>(
    publication: &'a ParsedPublication<'_>,
    options: MathGenerationOptions,
) -> Result<Vec<Formula<'a>>, MathGenerationCause> {
    let mut formulas = Vec::new();
    for (path, file) in publication.corpus().files() {
        for (location, identity, occurrences) in file.math_sources() {
            for occurrence in occurrences.iter().filter(|fact| fact.emitted) {
                if formulas.len() >= options.formulas.min(FORMULAS)
                    || occurrence.tex.len() > TEX_BYTES
                    || occurrence.tex.is_empty()
                {
                    return Err(MathGenerationCause::Limit("math formulas or TeX bytes"));
                }
                formulas.push(Formula {
                    path,
                    location,
                    identity,
                    occurrence,
                });
            }
        }
    }
    if !formulas.is_empty()
        && (options.formulas > FORMULAS
            || options.output_bytes == 0
            || options.output_bytes > OUTPUT_BYTES)
    {
        return Err(MathGenerationCause::Limit("math operation bounds"));
    }
    Ok(formulas)
}

#[derive(Serialize)]
struct RequestFormula<'a> {
    tex: &'a str,
    display: bool,
}

#[derive(Serialize)]
struct BatchRequest<'a> {
    formulas: Vec<RequestFormula<'a>>,
}

struct Job<'a, 'source> {
    formulas: &'a [Formula<'source>],
    request: String,
}

fn jobs<'a, 'source>(
    formulas: &'a [Formula<'source>],
    command: &Command,
    options: MathGenerationOptions,
) -> Result<Vec<Job<'a, 'source>>, MathGenerationCause> {
    options
        .execution
        .validate(command, &[])
        .map_err(MathGenerationCause::Native)?;
    let maximum = options.execution.max_input_bytes.min(INPUT_BYTES);
    let mut jobs = Vec::new();
    let mut start = 0;
    while start < formulas.len() {
        let mut end = (start + BATCH_FORMULAS).min(formulas.len());
        let request = loop {
            let request = BatchRequest {
                formulas: formulas[start..end]
                    .iter()
                    .map(|formula| RequestFormula {
                        tex: &formula.occurrence.tex,
                        display: formula.occurrence.display,
                    })
                    .collect(),
            };
            match encode(&request, maximum, "math stdin bytes") {
                Ok(bytes) => {
                    break String::from_utf8(bytes).map_err(|_| MathGenerationCause::Encoding)?;
                }
                Err(gazette_core::render::Error::Limit(_)) if end - start > 1 => {
                    end = start + (end - start) / 2
                }
                Err(gazette_core::render::Error::Limit(_)) => {
                    return Err(MathGenerationCause::Limit("math stdin bytes"));
                }
                Err(_) => return Err(MathGenerationCause::Encoding),
            }
        };
        CaptureRequest {
            request: Request {
                command,
                input: request.as_bytes(),
                options: options.execution,
            },
            max_output_bytes: options.output_bytes.min(BATCH_OUTPUT_BYTES),
            max_stderr_bytes: None,
        }
        .validate()
        .map_err(MathGenerationCause::Native)?;
        jobs.push(Job {
            formulas: &formulas[start..end],
            request,
        });
        start = end;
    }
    Ok(jobs)
}

fn invoke(
    runner: &dyn RetainedCaptureRunner,
    command: &Command,
    resources: gazette_core::source::TreeInput<'_>,
    entrypoint: &EntryPath,
    options: process::Options,
    maximum: usize,
    active: &mut Attempt,
) -> Result<(), MathGenerationCause> {
    if maximum == 0 {
        return Err(MathGenerationCause::Limit("math stdout bytes"));
    }
    let captured = runner
        .capture_retained(
            CaptureRequest {
                request: Request {
                    command,
                    input: active.request.as_bytes(),
                    options,
                },
                max_output_bytes: maximum,
                max_stderr_bytes: None,
            },
            resources,
            entrypoint,
        )
        .map_err(MathGenerationCause::Native)?;
    active.exit = Some(captured.exit);
    active.response = Some(captured.stdout);
    let bytes = active.response.as_deref().unwrap_or_default();
    if bytes.len() > maximum {
        return Err(MathGenerationCause::Limit("math stdout bytes"));
    }
    if captured.exit.code != Some(0) {
        return Err(MathGenerationCause::Exit(captured.exit));
    }
    std::str::from_utf8(bytes).map_err(|_| MathGenerationCause::Response)?;
    Ok(())
}

fn response(bytes: &[u8], count: usize) -> Result<Vec<String>, MathGenerationCause> {
    let value = json::parse(bytes).map_err(|_| MathGenerationCause::Response)?;
    let object = value
        .as_object()
        .filter(|object| object.len() == 1)
        .ok_or(MathGenerationCause::Response)?;
    let values = object
        .get("mathml")
        .and_then(serde_json::Value::as_array)
        .filter(|values| values.len() == count)
        .ok_or(MathGenerationCause::Response)?;
    values
        .iter()
        .map(|value| {
            let fragment = value
                .as_str()
                .filter(|fragment| !fragment.is_empty())
                .ok_or(MathGenerationCause::Response)?;
            if fragment.len() > FRAGMENT_BYTES {
                return Err(MathGenerationCause::Limit("math fragment bytes"));
            }
            Ok(fragment.to_owned())
        })
        .collect()
}
