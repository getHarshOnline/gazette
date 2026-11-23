//! Explicit native typesetting selection shared by publication recipe transports.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Per-acquisition consent and bounds, not provider selection or trust.
/// Selection may come from the workspace; permission is always supplied by the caller.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MathGeneration {
    /// Permit the workspace-selected command's native effects.
    pub allow_execute: bool,
    /// Complete native request ceiling, never a successful prefix.
    #[schemars(range(min = 1, max = 67108864))]
    pub max_input_bytes: usize,
    /// Native request deadline in milliseconds.
    #[schemars(range(min = 1, max = 300000))]
    pub timeout_ms: u64,
    /// Complete formula count across the captured publication universe.
    #[schemars(range(min = 1, max = 4096))]
    pub formulas: usize,
    /// Aggregate captured typesetting output ceiling.
    #[schemars(range(min = 1, max = 67108864))]
    pub output_bytes: usize,
}

impl Default for MathGeneration {
    fn default() -> Self {
        let native = super::MathGenerationOptions::default();
        Self {
            allow_execute: native.execution.allow_execute,
            max_input_bytes: native.execution.max_input_bytes,
            timeout_ms: u64::try_from(native.execution.timeout.as_millis()).unwrap_or(u64::MAX),
            formulas: native.formulas,
            output_bytes: native.output_bytes,
        }
    }
}

impl MathGeneration {
    /// Resolve invocation selection before workspace defaults; execution permission never comes from configuration.
    pub fn resolve(
        workspace: &crate::workspace::Workspace,
        selected: Option<bool>,
        generation: Option<Self>,
        allow_execute: bool,
    ) -> Option<Self> {
        let enabled = selected
            .or_else(|| generation.map(|_| true))
            .or_else(|| workspace.build().and_then(|build| build.math))
            .unwrap_or(false);
        enabled.then(|| {
            let mut options = generation.unwrap_or_default();
            options.allow_execute |= allow_execute;
            options
        })
    }

    pub(super) fn validate(self) -> Result<(), &'static str> {
        if !(1..=64 << 20).contains(&self.max_input_bytes)
            || !(1..=300_000).contains(&self.timeout_ms)
            || !(1..=4096).contains(&self.formulas)
            || !(1..=64 << 20).contains(&self.output_bytes)
        {
            return Err(
                "math generation requires bounded nonzero native execution and capture limits",
            );
        }
        Ok(())
    }

    /// Native execution and capture options for the resolved request.
    pub fn options(self) -> super::MathGenerationOptions {
        super::MathGenerationOptions {
            execution: gazette_core::process::Options {
                allow_execute: self.allow_execute,
                max_input_bytes: self.max_input_bytes,
                timeout: Duration::from_millis(self.timeout_ms),
            },
            formulas: self.formulas,
            output_bytes: self.output_bytes,
        }
    }
}

/// Acquire complete exact typesetting once with the standard native runner and Markdown owner.
/// Subsequent rendering consumes the retained capture without executing a command.
///
/// # Errors
/// Preserves native command, formula, capture and binding errors; never retries.
pub fn acquire_math(
    inputs: &mut super::AcquiredPublicationSet,
    corpus: crate::corpus::Limits,
    options: super::MathGenerationOptions,
) -> Result<(), super::MathGenerationError> {
    inputs.acquire_math(
        &gazette_adapter_process::Runner,
        corpus,
        &gazette_adapter_markdown::Comrak,
        options,
    )
}
