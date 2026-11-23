//! Retained-output transport; the SDK owns capture binding and disclosure scanning.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{source::TreeIdentity, toml};

use crate::{
    command::{Kernel, rendered_output},
    json_input::Object,
    render_site::probe,
};

#[derive(clap::Args)]
#[group(id = "ProbeOutput")]
pub(super) struct Args {
    /// Exact retained output root; no build or discovery is performed.
    #[arg(long)]
    tree: PathBuf,
    /// Independently retained complete {format,sha256} tree identity JSON.
    #[arg(long, value_parser = parse_identity)]
    tree_identity: TreeIdentity,
    /// Maximum private rules JSON bytes before any native tree read.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    #[command(flatten)]
    source: super::SourceLimits,
    #[command(flatten)]
    probe: probe::Args,
}

pub(super) fn run(args: Args) -> ExitCode {
    let limits = args.probe.limits();
    if let Err(error) = limits.validate() {
        return failure(error.code(), &error);
    }
    let mut bytes = Vec::new();
    if io::stdin()
        .lock()
        .take((args.max_source_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return failure(
            "disclosure_output_input",
            &"private stdin could not be read",
        );
    }
    if bytes.len() > args.max_source_bytes {
        return failure(
            "disclosure_output_input_limit",
            &"private stdin byte ceiling exceeded",
        );
    }
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(_) => {
            return failure(
                "disclosure_output_input",
                &"private stdin must be UTF-8 JSON",
            );
        }
    };
    match Kernel::new().execute(rendered_output::ProbeOutput {
        root: args.tree,
        expected_tree: args.tree_identity,
        source_limits: args.source.into_limits(),
        rules_json: text.to_owned(),
        max_rules_bytes: args.max_source_bytes,
        scan_limits: limits,
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => failure(error.code(), &error),
    }
}

fn parse_identity(value: &str) -> Result<TreeIdentity, String> {
    if value.len() > 1024 {
        return Err("tree identity exceeds 1024 bytes".into());
    }
    serde_json::from_str::<Object<TreeIdentity>>(value)
        .map(|Object(identity)| identity)
        .map_err(|_| "expected exact complete tree identity JSON".into())
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: {code}: {error}");
    ExitCode::FAILURE
}
