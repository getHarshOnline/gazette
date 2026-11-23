//! Local documentation intent checking and assembly through the shared application.

use crate::command::{Kernel, documentation as command};
use gazette_sdk::render::LimitedWriter;
use gazette_sdk::{build, source, toml, workspace};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Explicit public-base to literal-loopback-base JSON map for local manual links.
    /// Does not alter source URLs; incompatible with durable preparation or --check.
    #[arg(long, conflicts_with_all = ["preparation_json", "check"])]
    preview_link_bases_json: Option<String>,
    /// Acknowledge local captured-artifact assembly, not generator execution or hosted publication.
    /// Not required for `--check`, which parses only supplied workspace bytes.
    #[arg(long)]
    local: bool,
    /// Exact site whose brand-owned documentation build is selected.
    #[arg(long)]
    site: workspace::Key,
    /// Read the shared supplied/local `WorkspaceInput` JSON object from stdin instead of TOML.
    /// A local selection binds relative destinations to that explicit workspace owner.
    #[arg(long, conflicts_with = "check")]
    workspace_json: bool,
    /// Validate and print the resolved destination/manual/reference plan without source reads or effects.
    #[arg(
        long,
        conflicts_with_all = ["output_parent", "preparation_json", "generator_parent", "allow_execute"]
    )]
    check: bool,
    /// Retain a fresh private child here; never create or replace the configured repository destination.
    #[arg(long)]
    output_parent: Option<PathBuf>,
    /// Closed JSON selection for durable local preparation: journal/expected/parent/name/id,
    /// `create_stage`, `record_preparation`, writer and `evidence_bytes`. Never certification or release.
    #[arg(long, conflicts_with = "output_parent")]
    preparation_json: Option<String>,
    /// Explicit parent for fresh retained generator input/output stages; no existing tree is reused.
    #[arg(long)]
    generator_parent: Option<PathBuf>,
    /// Permit configured native generator commands (not sandboxed); requires a staging parent.
    #[arg(long, requires = "generator_parent")]
    allow_execute: bool,
    /// Per-command deadline, including complete stdout capture.
    #[arg(long, default_value_t = 30_000)]
    generator_timeout_ms: u64,
    /// Maximum complete version or generator stdout bytes per invocation.
    #[arg(long, default_value_t = 1 << 20)]
    max_generator_capture_bytes: usize,
    /// Complete configured generator count ceiling, checked before staging.
    #[arg(long, default_value_t = 32)]
    max_generator_jobs: usize,
    /// Complete response ceiling; native generation reserves lossless custody delivery first.
    #[arg(long, default_value_t = 16 << 20)]
    max_output_bytes: usize,
    /// Complete stdin workspace TOML ceiling, checked before native acquisition.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Independent limits for generator input stages, including repeated source copies.
    #[command(flatten)]
    generator_input: GeneratorInputLimits,
    /// Independent limits for generated output and the complete assembled tree.
    #[command(flatten)]
    output: super::SourceLimits,
}

#[derive(clap::Args)]
struct GeneratorInputLimits {
    /// Maximum entries across all generator input copies, including directories and hidden files.
    #[arg(long, default_value_t = source::Limits::default().entries)]
    max_generator_input_entries: usize,
    /// Maximum generator input descendant depth; immediate children have depth one.
    #[arg(long, default_value_t = source::Limits::default().depth)]
    max_generator_input_depth: usize,
    /// Maximum bytes in one generator input file.
    #[arg(long, default_value_t = source::Limits::default().file_bytes)]
    max_generator_input_file_bytes: u64,
    /// Maximum combined file bytes across all generator input copies.
    #[arg(long, default_value_t = source::Limits::default().total_bytes)]
    max_generator_input_total_bytes: u64,
}

impl GeneratorInputLimits {
    fn into_limits(self) -> source::Limits {
        source::Limits {
            entries: self.max_generator_input_entries,
            depth: self.max_generator_input_depth,
            file_bytes: self.max_generator_input_file_bytes,
            total_bytes: self.max_generator_input_total_bytes,
        }
    }
}

pub(super) fn run(args: Args) -> ExitCode {
    if let Some(encoded) = args.preview_link_bases_json.as_deref()
        && encoded.len() <= 65536
        && let Err(error) = gazette_sdk::json::parse(encoded.as_bytes())
    {
        return failure("documentation_preview_link_bases", &error);
    }
    let preview_link_bases = match args.preview_link_bases_json.as_deref() {
        Some(encoded) if encoded.len() <= 65536 => match serde_json::from_str::<
            crate::json_input::Object<gazette_sdk::publication::routing::PreviewLinkBases>,
        >(encoded)
        {
            Ok(parsed) => Some(parsed.0),
            Err(error) => return failure("documentation_preview_link_bases", &error),
        },
        Some(_) => {
            return failure(
                "documentation_preview_link_bases",
                &"preview link map exceeds 64 KiB",
            );
        }
        None => None,
    };
    if !args.check && !args.local {
        return failure(
            "documentation_mode",
            &"--local is required; assembly does not establish public admission",
        );
    }
    let preparation = match args.preparation_json.as_deref() {
        Some(encoded) => {
            if encoded.len() > 64 << 10 {
                return failure(
                    "documentation_preparation_selection",
                    &"preparation JSON exceeds 64 KiB",
                );
            }
            let parsed = encoded
                .trim_start()
                .starts_with('{')
                .then(|| {
                    serde_json::from_str::<build::documentation_output::PreparationSelection>(
                        encoded,
                    )
                    .ok()
                })
                .flatten();
            let Some(selected) = parsed else {
                return failure(
                    "documentation_preparation_selection",
                    &"use one closed named preparation JSON object",
                );
            };
            if let Err(code) = selected.validate(16 << 10) {
                return failure(
                    code,
                    &"exact coordinates and all preparation permissions are required before native work",
                );
            }
            Some(selected)
        }
        None => None,
    };
    let mut bytes = Vec::new();
    if let Err(error) = io::stdin()
        .lock()
        .take((args.max_source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
    {
        return failure("workspace_input", &error);
    }
    if bytes.len() > args.max_source_bytes {
        return failure("toml_limit", &"stdin source bytes ceiling exceeded");
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => return failure("workspace_utf8", &error),
    };
    if args.check {
        let outcome = match Kernel::new().execute(command::Check {
            source: text.to_owned(),
            site: args.site,
            syntax: toml::Limits {
                source_bytes: args.max_source_bytes,
                ..Default::default()
            },
        }) {
            Ok(outcome) => outcome,
            Err(error) => return failure(error.code(), &error),
        };
        let mut output = LimitedWriter::new(Vec::new(), args.max_output_bytes);
        if serde_json::to_writer(&mut output, &outcome).is_err()
            || output.exceeded()
            || output.write_all(b"\n").is_err()
        {
            return failure(
                "documentation_check_output_limit",
                &"complete documentation check exceeds the selected output ceiling",
            );
        }
        return match super::deliver_json(&output.into_inner()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure("documentation_check_output_io", &error),
        };
    }
    let input = if args.workspace_json {
        match serde_json::from_str::<crate::json_input::Object<build::WorkspaceInput>>(text) {
            Ok(selected) => selected.0,
            Err(error) => return failure("workspace_selection", &error),
        }
    } else {
        build::WorkspaceInput::Supplied {
            source: text.to_owned(),
        }
    };
    let acquisition = source::PreparationLimits {
        sources: 256,
        entries: source::Limits::default().entries,
        bytes: source::Limits::default().total_bytes,
        per_source: source::RevisionLimits::default(),
    };
    let limits = args.output.into_limits();
    let generation = args.generator_parent.map(|parent| command::Generation {
        parent,
        input: args.generator_input.into_limits(),
        output: limits,
        allow_execute: args.allow_execute,
        timeout: std::time::Duration::from_millis(args.generator_timeout_ms),
        max_capture_bytes: args.max_generator_capture_bytes,
        max_jobs: args.max_generator_jobs,
        directory_bytes: 16 << 10,
        entry_bytes: 4 << 10,
    });
    let retention = if let Some(selected) = preparation {
        command::Retention::Preparation {
            selection: selected,
            directory_bytes: 16 << 10,
        }
    } else if let Some(parent) = args.output_parent {
        command::Retention::Stage {
            parent,
            directory_bytes: 16 << 10,
        }
    } else {
        command::Retention::None
    };
    let outcome = match Kernel::new().execute(command::Assemble {
        preview_link_bases,
        input,
        site: args.site,
        local: args.local,
        allow_read: true,
        allow_stage: true,
        syntax: toml::Limits {
            source_bytes: args.max_source_bytes,
            ..Default::default()
        },
        acquisition,
        output: limits,
        generation,
        retention,
        response_bytes: args.max_output_bytes.saturating_sub(1),
    }) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error.code(), &error),
    };
    deliver_documentation(outcome)
}

fn deliver_documentation(outcome: command::Outcome) -> ExitCode {
    let failed = outcome.is_failure();
    let mut bytes = outcome.into_bytes();
    bytes.push(b'\n');
    if let Err(error) = super::deliver_json(&bytes) {
        let mut stderr = io::stderr().lock();
        let _ = writeln!(
            stderr,
            "gazette: output_io: {error}; exact native custody follows; do not retry or clean up automatically"
        );
        let _ = stderr.write_all(&bytes);
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no complete documentation target returned"
    );
    ExitCode::FAILURE
}
