//! Authored-manual and generated-reference commands shared by CLI and MCP.

use super::{Command, Kernel};
use gazette_sdk::{build, process, source, toml, workspace::Key};
use std::{path::PathBuf, time::Duration};

/// Stable documentation command failure.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl ToString) -> Self {
        Self {
            code,
            message: message.to_string(),
        }
    }

    /// Stable native failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Redacted operator-facing diagnostic.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for Failure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// Explicit native documentation-generator selection.
pub struct Generation {
    /// Existing private parent for fresh generator stages.
    pub parent: PathBuf,
    /// Generator-input capture ceilings.
    pub input: source::Limits,
    /// Generated-output capture ceilings.
    pub output: source::Limits,
    /// Permission to execute configured native commands.
    pub allow_execute: bool,
    /// Complete command deadline.
    pub timeout: Duration,
    /// Version and generator stdout ceiling.
    pub max_capture_bytes: usize,
    /// Complete configured generator-job ceiling.
    pub max_jobs: usize,
    /// Canonical retained-directory byte ceiling.
    pub directory_bytes: usize,
    /// Selected artifact entry byte ceiling.
    pub entry_bytes: usize,
}

/// Optional local documentation-repository retention effect.
pub enum Retention {
    /// Keep the assembled tree in memory only.
    None,
    /// Retain one fresh private target stage.
    Stage {
        /// Existing private staging parent.
        parent: PathBuf,
        /// Canonical retained-directory byte ceiling.
        directory_bytes: usize,
    },
    /// Record one durable local preparation.
    Preparation {
        /// Exact native preparation coordinates and permissions.
        selection: build::documentation_output::PreparationSelection,
        /// Canonical retained-directory byte ceiling.
        directory_bytes: usize,
    },
}

/// Effect-free documentation intent check over supplied workspace bytes.
pub struct Check {
    /// Complete workspace TOML.
    pub source: String,
    /// Exact configured publication site.
    pub site: Key,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
}

impl Command for Check {
    type Outcome = build::DocumentationCheck;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        build::check_documentation(&self.source, &self.site, self.syntax)
            .map_err(|error| Failure::new(error.code(), error))
    }
}

/// Complete local documentation-assembly command. Its encoded result is
/// native custody, not transport framing: effectful execution reserves it before
/// starting generators or retained writes.
pub struct Assemble {
    /// Explicit private preview hyperlink environment; never a maintained target setting.
    pub preview_link_bases: Option<gazette_sdk::publication::routing::PreviewLinkBases>,
    /// Shared exact supplied/local workspace selection; no implicit native root.
    pub input: build::WorkspaceInput,
    /// Exact configured publication site.
    pub site: Key,
    /// Explicit local-operation acknowledgement.
    pub local: bool,
    /// Permission for configured native source reads.
    pub allow_read: bool,
    /// Permission for selected fresh retention effects.
    pub allow_stage: bool,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Complete selected-source acquisition ceilings.
    pub acquisition: source::PreparationLimits,
    /// Assembled generated-tree ceilings.
    pub output: source::Limits,
    /// Optional configured native generation.
    pub generation: Option<Generation>,
    /// Optional local retention effect.
    pub retention: Retention,
    /// Complete pre-reserved custody response ceiling.
    pub response_bytes: usize,
}

/// Pre-reserved native documentation report or compact custody evidence.
pub struct Outcome(build::documentation_output::Encoded);

impl Outcome {
    /// Exact native custody bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }

    /// Whether the custody represents a failed operation.
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        self.0.is_error
    }

    /// Consume the outcome and return exact native custody bytes.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0.bytes
    }
}

impl Assemble {
    /// Consume the single native documentation sequence inside its retained-result lifetime.
    /// Other commands reuse this owner instead of reacquiring inputs or remapping generators.
    pub(super) fn consume_execution<T>(
        &self,
        consume: impl FnOnce(build::documentation_output::Execution<'_>) -> T,
    ) -> Result<T, Failure> {
        let effectful = self.generation.is_some() || !matches!(self.retention, Retention::None);
        if !self.local
            || !self.allow_read
            || (effectful && !self.allow_stage)
            || self
                .generation
                .as_ref()
                .is_some_and(|generation| !generation.allow_execute)
        {
            return Err(Failure::new(
                "documentation_permission",
                "explicit local/read/stage/execute permissions are independent; no source acquired, stage created or command started",
            ));
        }
        if let Retention::Preparation {
            selection,
            directory_bytes,
        } = &self.retention
        {
            selection.validate(*directory_bytes).map_err(|code| {
                Failure::new(
                    code,
                    "exact preparation coordinates and independent permissions are required before native work",
                )
            })?;
        }
        let generation = self.generation.as_ref().map(|generation| {
            build::documentation_output::GenerationSelection {
                parent: &generation.parent,
                options: build::DocumentationGenerationOptions {
                    input: generation.input,
                    output: generation.output,
                    execution: process::Options {
                        allow_execute: generation.allow_execute,
                        timeout: generation.timeout,
                        ..Default::default()
                    },
                    max_capture_bytes: generation.max_capture_bytes,
                    max_jobs: generation.max_jobs,
                    directory_bytes: generation.directory_bytes,
                    entry_bytes: generation.entry_bytes,
                },
            }
        });
        let retention = match &self.retention {
            Retention::None => build::documentation_output::RetentionSelection::None,
            Retention::Stage {
                parent,
                directory_bytes,
            } => build::documentation_output::RetentionSelection::Stage {
                parent,
                directory_bytes: *directory_bytes,
            },
            Retention::Preparation { selection, .. } => {
                build::documentation_output::RetentionSelection::Preparation {
                    selection,
                    coordinator: None,
                    allow_anchor_write: false,
                }
            }
        };
        Ok(build::documentation_output::Operation {
            preview_link_bases: self.preview_link_bases.as_ref(),
            input: &self.input,
            site: &self.site,
            syntax: self.syntax,
            acquisition: self.acquisition,
            output: self.output,
            generation,
            retention,
            response_bytes: self.response_bytes,
            reserve_read_only_response: true,
        }
        .execute_with_delivery(consume))
    }

    /// Configured output is a distinct local effect, not an additional stage/preparation.
    pub(super) fn require_configured_output(&self) -> Result<(), Failure> {
        if !matches!(self.retention, Retention::None) {
            return Err(Failure::new(
                "documentation_materialization_selection",
                "configured output cannot also select fresh staging or preparation; no generator started",
            ));
        }
        Ok(())
    }
}

impl Command for Assemble {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        self.consume_execution(|execution| match execution {
            build::documentation_output::Execution::Before(error) => {
                Err(Failure::new(error.code(), error))
            }
            build::documentation_output::Execution::Delivered { response, .. } => {
                Ok(Outcome(response))
            }
            build::documentation_output::Execution::Complete { .. } => Err(Failure::new(
                "documentation_response_contract",
                "read-only response was not natively reserved",
            )),
            build::documentation_output::Execution::Contract { custody } => Err(Failure::new(
                "documentation_response_contract",
                format!("native custody contract failed after work: {custody:?}; do not retry"),
            )),
        })?
    }
}
