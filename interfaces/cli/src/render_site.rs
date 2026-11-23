//! Direct local compilation; native effects require an explicit output parent.

mod documentation;
pub(crate) mod probe;

use crate::command::{Kernel, rendered_output};
use gazette_sdk::{source, toml, workspace};
use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Explicit presentation year; omission captures the current UTC year at this boundary.
    #[arg(long)]
    render_year: Option<gazette_sdk::authored::RenderYear>,
    /// Required acknowledgement: draft-inclusive local output, not public admission or certification.
    #[arg(long, required = true)]
    local: bool,
    /// Override the workspace's aggregate corpus byte limit.
    #[arg(long)]
    max_corpus_bytes: Option<usize>,
    /// Override configured math selection without granting execution permission.
    #[arg(long, action = clap::ArgAction::Set)]
    math: Option<bool>,
    /// Permit this call's configured math and sharing-image generation.
    #[arg(long)]
    allow_execute: bool,
    /// Existing private parent for configured documentation generator stages.
    /// Execution still requires --allow-execute.
    #[arg(long)]
    documentation_generation_parent: Option<PathBuf>,
    /// Exact configured publication site; all configured authored sources participate in graph resolution.
    #[arg(long)]
    site: workspace::Key,
    /// Optional site-relative debug model artifact; any selected collision fails.
    #[arg(long)]
    model_output: Option<source::EntryPath>,
    /// Reserved local graph-view path; the Theme supplies its graph render entry.
    #[arg(long)]
    graph_output: Option<source::EntryPath>,
    /// Create and retain a fresh private tree beneath this existing parent; never replace a tree.
    #[arg(long)]
    output_parent: Option<PathBuf>,
    /// Maximum canonical retained-directory bytes, enforced before native creation.
    #[arg(long, default_value_t = 16 << 10)]
    directory_bytes: usize,
    /// Inspect retained output with private stdin JSON rules; partial evidence, never certification or staging.
    #[arg(long, conflicts_with = "output_parent")]
    probe_disclosure: bool,
    #[command(flatten)]
    probe: probe::Args,
    /// Bound complete stdin bytes (workspace TOML, or the probe JSON request) before native reads.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Bounds for the complete generated output, independent of default input/compiler bounds.
    #[command(flatten)]
    output: super::SourceLimits,
}

pub(super) fn run(args: Args) -> ExitCode {
    if !args.local {
        return failure(
            "publication_mode",
            &"--local is required; public admission is not implemented",
        );
    }
    let render_year = match crate::calendar::resolve(args.render_year) {
        Ok(year) => year,
        Err(error) => return failure("render_year", &error),
    };
    let probe_limits = args.probe.limits();
    if args.probe_disclosure
        && let Err(error) = probe_limits.validate()
    {
        return failure(error.code(), &error);
    }
    let syntax = toml::Limits {
        source_bytes: args.max_source_bytes,
        ..Default::default()
    };
    let mut bytes = Vec::new();
    if let Err(error) = io::stdin()
        .lock()
        .take((syntax.source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
    {
        return failure("workspace_input", &error);
    }
    if bytes.len() > syntax.source_bytes {
        return failure(
            if args.probe_disclosure {
                "disclosure_probe_input_limit"
            } else {
                "toml_limit"
            },
            &"stdin source bytes ceiling exceeded",
        );
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) => return failure("workspace_utf8", &error),
    };
    let request = if args.probe_disclosure {
        match probe::Input::parse(text) {
            Ok(request) => Some(request),
            Err(error) => return failure(error.code(), &error),
        }
    } else {
        None
    };
    let workspace = request
        .as_ref()
        .map_or(text, |request| request.workspace.as_str());
    let acquisition = source::PreparationLimits {
        sources: 256,
        entries: source::Limits::default().entries,
        bytes: source::Limits::default().total_bytes,
        per_source: source::RevisionLimits::default(),
    };
    let output_limits = args.output.into_limits();
    let command = rendered_output::RenderSite {
        local: args.local,
        corpus_bytes: args.max_corpus_bytes,
        math: args.math,
        allow_execute: args.allow_execute,
        source: workspace.to_owned(),
        site: args.site,
        syntax,
        acquisition,
        options: rendered_output::OwnedSiteRenderOptions {
            render_year: Some(render_year),
            model_output: args.model_output,
            graph_output: args.graph_output,
            limits: output_limits,
        },
        output_parent: args.output_parent,
        directory_bytes: args.directory_bytes,
        disclosure: request.map(|request| (request, probe_limits)),
    };
    if let Some(parent) = args.documentation_generation_parent {
        let mut request = gazette_sdk::build::DocumentationGenerationRequest::new(parent);
        request.allow_execute = args.allow_execute;
        return match command.with_documentation(&request, documentation::deliver) {
            Ok(status) => status,
            Err(error) => failure(error.code(), &error),
        };
    }
    match Kernel::new().execute(command) {
        Ok(outcome) => super::write_stage_json(outcome.report(), outcome.retained()),
        Err(error) if error.stage_error().is_some() => failure("publication_stage", &error),
        Err(error) => failure(error.code(), &error),
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: {code}: {error}");
    ExitCode::FAILURE
}
