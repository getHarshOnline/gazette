//! Explicit native reference generation; execution evidence is not target compatibility.

use super::{Format, Selection};
use crate::{process, source, stage::RetainedStage};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, path::PathBuf};

/// Literal command arguments and separately typed native input/output locations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Argument {
    /// Passed unchanged, without shell interpretation or placeholder expansion.
    Literal {
        /// Exact argument bytes after UTF-8 decoding.
        value: String,
    },
    /// Explicit workspace or machine coordinate, bound before native staging.
    Path {
        /// Absolute native path after configuration acquisition; never interpolated.
        path: String,
    },
    /// Existing regular file or directory beneath the exact materialized source.
    Input {
        /// Exact relative source entry, without globbing or traversal.
        path: source::EntryPath,
    },
    /// Complete retained input root, passed as `.` from its exact working directory.
    /// This is explicit root selection for package-oriented tools, not ambient cwd discovery.
    InputRoot,
    /// Exact selected output file or directory; for example `TypeDoc`'s `--json` argument.
    Output,
    /// Complete output directory; for example Rustdoc's `-o` argument.
    OutputRoot,
}

/// Workspace-selected invocation; tool identity is independent of project and target identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields, bound(deserialize = "E: Deserialize<'de>"))]
pub struct Recipe<E = String> {
    /// Absolute native executable path; no ambient PATH lookup.
    pub program: String,
    /// Exact executable bytes, verified before and after each native invocation.
    pub executable: process::ExecutablePin,
    /// Ordered generation arguments; this is not a shell command string.
    pub arguments: Vec<Argument>,
    /// Complete explicit environment; interpreter packages and external inputs need separate qualification.
    #[serde(default)]
    pub environment: BTreeMap<String, E>,
    /// Literal values or explicit paths for observing this same executable's version.
    pub version_arguments: Vec<E>,
    /// Exact digest of the complete expected version stdout bytes.
    ///
    /// This binds the configured recipe to the provider observation accepted by
    /// its owner. Pinning an interpreter executable alone does not pin the
    /// Python or Node package it loads.
    pub expected_version_sha256: source::Sha256Digest,
    /// Optional read-only validation of captured output using this same pinned executable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_arguments: Option<Vec<Argument>>,
    /// Explicit file or directory within the complete generated capture.
    pub output: Selection,
    /// Declared native format, not proof of schema fidelity or target compatibility.
    pub format: Format,
}

impl<E> Recipe<E> {
    /// Convert environment and version-command values while preserving retained-stage arguments.
    /// Portable configuration uses this before native validation; literal values need no expansion.
    ///
    /// # Errors
    /// Returns the caller's first value-conversion failure without a partial recipe.
    pub fn try_map_runtime_values<T, F>(
        &self,
        mut map: impl FnMut(&E, &'static str) -> Result<T, F>,
    ) -> Result<Recipe<T>, F> {
        Ok(Recipe {
            program: self.program.clone(),
            executable: self.executable,
            arguments: self.arguments.clone(),
            environment: self
                .environment
                .iter()
                .map(|(name, value)| Ok((name.clone(), map(value, "environment")?)))
                .collect::<Result<_, F>>()?,
            version_arguments: self
                .version_arguments
                .iter()
                .map(|value| map(value, "version argument"))
                .collect::<Result<_, F>>()?,
            expected_version_sha256: self.expected_version_sha256,
            validation_arguments: self.validation_arguments.clone(),
            output: self.output.clone(),
            format: self.format,
        })
    }
}

impl Recipe {
    /// Check bounded command spelling and explicit source/output use without native effects.
    ///
    /// # Errors
    /// Refuses missing source/output arguments, an empty version command or native command limits.
    pub fn validate(&self) -> Result<(), process::Error> {
        if self.arguments.iter().chain(self.validation_arguments.iter().flatten()).any(
            |argument| matches!(argument, Argument::Path { path } if !std::path::Path::new(path).is_absolute()),
        ) {
            return Err(process::Error::before_start(process::Failure::Command));
        }
        self.validate_commands(process::Command::validate)
    }

    /// Check source/output arguments, pins and text limits before binding the executable path.
    /// Execution must still call `validate` after portable coordinates have been resolved.
    ///
    /// # Errors
    /// Refuses missing explicit inputs/outputs, an empty version command or invalid command text.
    pub fn validate_text(&self) -> Result<(), process::Error> {
        self.validate_commands(process::Command::validate_text)
    }

    fn validate_commands(
        &self,
        validate: impl Fn(&process::Command) -> Result<(), process::Error>,
    ) -> Result<(), process::Error> {
        if self.version_arguments.is_empty()
            || !self
                .arguments
                .iter()
                .any(|arg| matches!(arg, Argument::Input { .. } | Argument::InputRoot))
            || !self
                .arguments
                .iter()
                .any(|arg| matches!(arg, Argument::Output | Argument::OutputRoot))
            || self.validation_arguments.as_ref().is_some_and(|arguments| {
                !arguments
                    .iter()
                    .any(|arg| matches!(arg, Argument::Output | Argument::OutputRoot))
            })
        {
            return Err(process::Error::before_start(process::Failure::Command));
        }
        let lower = |arguments: &[Argument]| {
            arguments
                .iter()
                .map(|arg| match arg {
                    Argument::Literal { value } => value.clone(),
                    Argument::Path { path } => path.clone(),
                    Argument::Input { path } => path.as_str().to_owned(),
                    Argument::InputRoot => ".".to_owned(),
                    Argument::Output => self
                        .output
                        .path()
                        .map_or_else(String::new, ToString::to_string),
                    Argument::OutputRoot => String::new(),
                })
                .collect()
        };
        let mut command = process::Command {
            program: self.program.clone(),
            executable: Some(self.executable),
            arguments: lower(&self.arguments),
            // Native working directories are supplied by the retained-stage request.
            directory: "/".to_owned(),
            environment: self.environment.clone(),
        };
        validate(&command)?;
        command.arguments.clone_from(&self.version_arguments);
        validate(&command)?;
        if let Some(arguments) = &self.validation_arguments {
            command.arguments = lower(arguments);
            validate(&command)?;
        }
        Ok(())
    }
}

/// One already materialized job. Stage creation/retention belongs to the application.
pub struct Request<'a> {
    /// Exact configured recipe.
    pub recipe: &'a Recipe,
    /// Independently retained input tree, rechecked before and after execution.
    pub input: &'a RetainedStage,
    /// Separately retained empty output directory; never automatically cleaned up or reused.
    pub output: &'a RetainedStage,
    /// Complete input re-observation bounds.
    pub input_limits: source::Limits,
    /// Complete generated-tree capture bounds.
    pub output_limits: source::Limits,
    /// Explicit execution permission and per-invocation deadline/input bound.
    pub execution: process::Options,
    /// Complete stdout ceiling per invocation, including version observation.
    pub max_capture_bytes: usize,
}

/// Exact command observation, including a started/uncertain invocation without invented exit.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Invocation {
    /// Digest of the complete lowered native command, including private working directory.
    pub command_sha256: source::Sha256Digest,
    /// Present only after the runner delivered a complete capture and reaped direct-child result.
    pub exit: Option<process::Exit>,
    /// Present only for complete stdout; partial output is never promoted.
    pub stdout_sha256: Option<source::Sha256Digest>,
    /// Complete stdout length, not an artifact-format assertion.
    #[schemars(range(max = usize::MAX))]
    pub stdout_bytes: Option<usize>,
}

/// Native validation observation bound to the complete output presented to the tool.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Validation {
    /// Exact captured output, not a replacement produced by validation.
    pub tree: source::TreeIdentity,
    /// Actual invocation; success alone does not establish target compatibility.
    pub invocation: Invocation,
}

/// Private progress retained across later failures; none of this certifies publication.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct Evidence {
    /// Original retained input identity, not the generator's claim about what it read.
    pub input: source::TreeIdentity,
    /// Version invocation, including uncertainty if capture failed.
    pub version: Option<Invocation>,
    /// Version observation required by the configured recipe.
    pub expected_version_sha256: source::Sha256Digest,
    /// Complete UTF-8 version output, without trimming or inferred tool names.
    pub version_text: Option<String>,
    /// Generation invocation, including uncertainty if capture failed.
    pub generation: Option<Invocation>,
    /// Selected native validation, including uncertainty or refusal after generation.
    pub validation: Option<Validation>,
}

/// Complete retained generated tree and its actual native execution history.
#[derive(Debug)]
pub struct Generated {
    /// All captured output files, including companions; never a selected-file-only replacement.
    pub output: source::CapturedSource,
    /// Exact selected file or directory within that output tree.
    pub selection: Selection,
    /// Declared format; the consuming adapter still validates compatibility.
    pub format: Format,
    /// Completed version, generation and any selected validation observations.
    pub evidence: Evidence,
}

/// Failed native boundary; a phase label never establishes absence of effects.
#[derive(Clone, Copy, Debug, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    /// Request checks and native input/output admission.
    Preflight,
    /// Independently observed native version.
    Version,
    /// Native generation invocation.
    Generation,
    /// Exact input recheck and complete output capture.
    Capture,
    /// Selected native validation and stable input/output re-observation.
    Validation,
}

/// Native failure preserved without a fake successful artifact.
#[derive(Debug)]
pub enum Cause {
    /// Stable generator-contract rejection.
    Invalid(&'static str),
    /// Existing process failure and direct-child custody.
    Process(process::Error),
    /// Confined native source/output observation failed.
    Source(Box<source::Error>),
    /// Native input/output roots overlap or cannot be safely resolved.
    Separation(Box<crate::destination::Error>),
    /// Command-evidence encoding failed.
    Encoding(serde_json::Error),
}

/// Failed job retains exact native locators and every completed invocation observation.
#[derive(Debug)]
pub struct Error {
    /// Failed boundary.
    pub phase: Phase,
    /// Original native cause.
    pub cause: Cause,
    /// Source stage is not deleted or silently restored.
    pub input: PathBuf,
    /// Output may contain unverified residue; no automatic retry or cleanup.
    pub output: PathBuf,
    /// Earlier successful phases and active invocation identities survive.
    pub evidence: Box<Evidence>,
}

impl Error {
    /// Stable redacted class, without native paths or subprocess output.
    pub fn code(&self) -> &'static str {
        match &self.cause {
            Cause::Invalid(code) => code,
            Cause::Process(error) => error.code(),
            Cause::Source(error) => error.code(),
            Cause::Separation(_) => "documentation_generator_separation",
            Cause::Encoding(_) => "documentation_generator_encoding",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} during {:?}; retain native job evidence without retry or cleanup",
            self.code(),
            self.phase
        )
    }
}
impl std::error::Error for Error {}
