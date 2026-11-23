//! Shared explicit native diagram acquisition options; command selection remains workspace-owned.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Per-operation consent and bounded capture, never provider selection or native containment.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiagramGeneration {
    /// Permit the workspace-selected pinned renderer's native effects.
    pub allow_execute: bool,
    /// Complete DOT stdin ceiling per invocation; at most 64 MiB.
    #[schemars(range(min = 1, max = 67108864))]
    pub max_input_bytes: usize,
    /// Shared native deadline, 1–300,000 milliseconds.
    #[schemars(range(min = 1, max = 300000))]
    pub timeout_ms: u64,
    /// Complete unique diagram count, 1–256.
    #[schemars(range(min = 1, max = 256))]
    pub diagrams: usize,
    /// Complete captured PNG ceiling, 1–64 MiB.
    #[schemars(range(min = 1, max = 67108864))]
    pub output_bytes: usize,
    /// Bounded stderr, 0–64 KiB; any observed diagnostic refuses success.
    #[schemars(range(max = 65536))]
    pub diagnostic_bytes: usize,
}

impl Default for DiagramGeneration {
    fn default() -> Self {
        let native = super::DiagramGenerationOptions::default();
        Self {
            allow_execute: false,
            max_input_bytes: native.execution.max_input_bytes,
            timeout_ms: 30_000,
            diagrams: native.diagrams,
            output_bytes: native.output_bytes,
            diagnostic_bytes: native.diagnostic_bytes,
        }
    }
}

impl DiagramGeneration {
    pub(super) fn validate(self) -> Result<(), &'static str> {
        if !(1..=64 << 20).contains(&self.max_input_bytes)
            || !(1..=300_000).contains(&self.timeout_ms)
            || !(1..=256).contains(&self.diagrams)
            || !(1..=64 << 20).contains(&self.output_bytes)
            || self.diagnostic_bytes > 64 << 10
        {
            return Err("diagram generation requires bounded native execution and capture limits");
        }
        Ok(())
    }
    pub(super) fn options(self) -> super::DiagramGenerationOptions {
        super::DiagramGenerationOptions {
            execution: gazette_core::process::Options {
                allow_execute: self.allow_execute,
                max_input_bytes: self.max_input_bytes,
                timeout: Duration::from_millis(self.timeout_ms),
            },
            diagrams: self.diagrams,
            output_bytes: self.output_bytes,
            diagnostic_bytes: self.diagnostic_bytes,
            raster: Default::default(),
        }
    }
}

/// Capture selected diagrams once with the native runner and existing PNG pixel decoder.
///
/// # Errors
/// Preserves complete input/binding, process and PNG failures without fallback or retry.
pub fn acquire_diagrams(
    inputs: &mut super::AcquiredPublicationSet,
    corpus: crate::corpus::Limits,
    options: super::DiagramGenerationOptions,
) -> Result<(), super::DiagramGenerationError> {
    inputs.acquire_diagrams(
        &gazette_adapter_process::Runner,
        &crate::raster::inspect,
        corpus,
        &gazette_adapter_markdown::Comrak,
        options,
    )
}
