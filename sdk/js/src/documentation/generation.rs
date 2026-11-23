//! Native job execution and shared custody delivery; no second generator or recovery owner.

use gazette_sdk::{build::documentation_output as output, process, source, toml};
use napi::Result;
use serde::Deserialize;
use std::{path::PathBuf, time::Duration};

use crate::{Limits, query, transport};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Generation {
    parent: PathBuf,
    max_jobs: usize,
    directory_bytes: usize,
    entry_bytes: usize,
    #[serde(default)]
    allow_execute: bool,
    #[serde(default = "timeout")]
    timeout_ms: u64,
    #[serde(default = "capture")]
    max_capture_bytes: usize,
    #[serde(default)]
    input: source::Limits,
}

fn timeout() -> u64 {
    30_000
}
fn capture() -> usize {
    1 << 20
}

pub(super) struct Request {
    generation: Generation,
}

impl Request {
    pub(super) fn parse(
        json: &str,
        maximum: usize,
        allow_stage: bool,
        _result: usize,
    ) -> Result<Self> {
        let value: serde_json::Value = query(
            json,
            &Limits {
                syntax: toml::Limits::default(),
                query: maximum,
                result: 0,
            },
        )?;
        if value.get("input").is_some_and(|value| !value.is_object()) {
            return Err(transport("node_query"));
        }
        let generation: Generation =
            serde_json::from_value(value).map_err(|_| transport("node_query"))?;
        if !allow_stage || !generation.allow_execute {
            return Err(transport("documentation_permission"));
        }
        if !generation.parent.is_absolute()
            || !(1..=300_000).contains(&generation.timeout_ms)
            || generation.max_capture_bytes > 64 << 20
        {
            return Err(transport("documentation_generation_limits"));
        }
        Ok(Self { generation })
    }

    pub(super) fn selection(&self, limits: source::Limits) -> output::GenerationSelection<'_> {
        output::GenerationSelection {
            parent: &self.generation.parent,
            options: gazette_sdk::build::DocumentationGenerationOptions {
                input: self.generation.input,
                output: limits,
                execution: process::Options {
                    allow_execute: self.generation.allow_execute,
                    timeout: Duration::from_millis(self.generation.timeout_ms),
                    ..Default::default()
                },
                max_capture_bytes: self.generation.max_capture_bytes,
                max_jobs: self.generation.max_jobs,
                directory_bytes: self.generation.directory_bytes,
                entry_bytes: self.generation.entry_bytes,
            },
        }
    }
}
