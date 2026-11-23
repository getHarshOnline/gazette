//! Direct source-set checks and fresh retention through the shared Rust SDK.

use crate::command::{Kernel, rendered_output};
use gazette_sdk::source::{Inventory, PreparationLimits, RevisionLimits, input};
use serde::Serialize;
use std::{
    fmt::Display,
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Explicit authored input root; never modified.
    authored: PathBuf,
    /// Existing trusted staging parent, disjoint from every input root.
    parent: PathBuf,
    /// Explicit workspace identity, not a path or approval.
    #[arg(long)]
    workspace: String,
    /// Explicit site identity within that workspace.
    #[arg(long)]
    site: String,
    /// Repeat a strict JSON object containing target, project, deployment and source.
    #[arg(long, value_parser = input::parse, required_unless_present = "authored_only", conflicts_with = "authored_only")]
    placement: Vec<input::Placement>,
    /// Explicitly select no project placements; authored input still participates.
    #[arg(long)]
    authored_only: bool,
    /// Maximum placements; bounds repeated native acquisition work before reads.
    #[arg(long, default_value_t = 128)]
    max_sources: usize,
    /// Maximum decoded Git metadata bytes per source.
    #[arg(long, default_value_t = RevisionLimits::default().metadata_bytes)]
    max_git_metadata_bytes: usize,
    /// Limits apply per input, across project inputs, and to the complete output.
    #[command(flatten)]
    limits: super::SourceLimits,
}

#[derive(clap::Args)]
pub(super) struct CreateArgs {
    #[command(flatten)]
    pub input: Args,
    /// Permit fresh creation and synchronized retention beneath PARENT.
    #[arg(long, required = true)]
    pub create_stage: bool,
    /// Create exactly this fresh child of PARENT; collisions never select another name.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Serialize)]
struct Report<'a> {
    workspace: &'a str,
    site: &'a str,
    inventory: &'a Inventory,
    #[serde(flatten)]
    outcome: Outcome<'a>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome<'a> {
    Checked,
    Retained { directory: &'a Path },
}

pub(super) fn run(args: Args, create: bool, name: Option<&str>) -> ExitCode {
    if args.placement.is_empty() != args.authored_only {
        return failure("select placements or explicitly use --authored-only; no stage created");
    }
    let limits = args.limits.into_limits();
    let (context, requests) = match input::bind(args.workspace, args.site, args.placement) {
        Ok(bound) => bound,
        Err(error) => return failure(format_args!("{error}; no stage created")),
    };
    let input = rendered_output::StageInput {
        allow_read: true,
        context,
        placements: requests,
        preparation: PreparationLimits {
            sources: args.max_sources,
            entries: limits.entries,
            bytes: limits.total_bytes,
            per_source: RevisionLimits {
                content: limits,
                metadata_bytes: args.max_git_metadata_bytes,
            },
        },
        authored: args.authored,
        parent: args.parent,
        limits,
    };
    if !create {
        return match Kernel::new().execute(rendered_output::CheckStage(input)) {
            Ok(outcome) => super::write_json(&Report {
                workspace: &outcome.context().workspace,
                site: &outcome.context().site,
                inventory: outcome.inventory(),
                outcome: Outcome::Checked,
            }),
            Err(error) => failure(error),
        };
    }
    let outcome = match Kernel::new().execute(rendered_output::CreateStage {
        input,
        name: name.map(str::to_owned),
        directory_bytes: None,
    }) {
        Ok(outcome) => outcome,
        Err(error) => return failure(error),
    };
    let retained = outcome.retained();
    let status = super::write_json_outcome(
        &Report {
            workspace: &outcome.context().workspace,
            site: &outcome.context().site,
            inventory: retained.inventory(),
            outcome: Outcome::Retained {
                directory: retained.path(),
            },
        },
        false,
    );
    if status != ExitCode::SUCCESS {
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: stage retained at {:?}, but receipt delivery failed; inspect it before retrying; no cleanup attempted",
            retained.path()
        );
    }
    status
}

fn failure(error: impl Display) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: source_staging: {error}");
    ExitCode::FAILURE
}
