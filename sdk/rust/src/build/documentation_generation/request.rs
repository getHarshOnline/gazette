//! Caller-selected retained staging and execution bounds, never workspace-implied authority.

use crate::{build::DocumentationGenerationOptions, process, source};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};

/// Explicit selection of existing configured documentation generators for local publication.
/// Selecting this parent authorizes fresh private stages there; execution permission is separate.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentationGenerationRequest {
    /// Existing absolute private parent, disjoint from every captured source/configuration root.
    pub parent: PathBuf,
    /// Permit the configured pinned generators to execute.
    #[serde(default)]
    pub allow_execute: bool,
    /// Deadline per native invocation, not a promise to interrupt native process creation.
    #[serde(default = "timeout")]
    #[schemars(range(min = 1, max = 300000))]
    pub timeout_ms: u64,
    /// Whole-set generated job ceiling, including all selected documentation owners.
    #[serde(default = "jobs")]
    #[schemars(range(min = 1, max = 128))]
    pub jobs: usize,
    /// Whole-set retained native response ceiling, reserved before the relevant effects.
    #[serde(default = "response")]
    #[schemars(range(min = 1, max = 16777216))]
    pub response_bytes: usize,
}
const fn timeout() -> u64 {
    30_000
}
const fn jobs() -> usize {
    128
}
const fn response() -> usize {
    16 << 20
}
impl DocumentationGenerationRequest {
    /// Select staging explicitly, without granting process execution.
    pub fn new(parent: PathBuf) -> Self {
        Self {
            parent,
            allow_execute: false,
            timeout_ms: timeout(),
            jobs: jobs(),
            response_bytes: response(),
        }
    }
    /// Validate the explicit selection without reading roots or running a configured command.
    ///
    /// # Errors
    /// Refuses missing/relative parents or unsupported whole-operation ceilings.
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.parent.is_absolute()
            || self.parent.as_os_str().len() > 2048
            || !(1..=300_000).contains(&self.timeout_ms)
            || !(1..=128).contains(&self.jobs)
            || !(1..=16 << 20).contains(&self.response_bytes)
        {
            return Err("invalid documentation generation parent or bounds");
        }
        Ok(())
    }
    pub(super) fn options(
        &self,
        input: source::Limits,
        output: source::Limits,
        jobs: usize,
    ) -> DocumentationGenerationOptions {
        DocumentationGenerationOptions {
            input,
            output,
            execution: process::Options {
                allow_execute: self.allow_execute,
                timeout: Duration::from_millis(self.timeout_ms),
                ..Default::default()
            },
            max_capture_bytes: 16 << 20,
            max_jobs: jobs,
            directory_bytes: 4096,
            entry_bytes: 4096,
        }
    }
}
