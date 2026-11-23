//! Read-only generated-documentation evidence through the common native verifier.

use gazette_sdk::source::{GeneratedTreeBinding, Sha256Digest};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{
    Kernel,
    inspection::{GeneratedTreeCommand, GeneratedTreeRequest},
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing generated project-documentation root, never created or repaired.
    root: PathBuf,
    /// Exact workspace/site/project/deployment/target JSON; not inferred from the marker.
    #[arg(long, value_parser = parse_binding)]
    expected_binding: GeneratedTreeBinding,
    /// Manifest digest retained independently from prior staging evidence.
    #[arg(long)]
    manifest_sha256: Sha256Digest,
    #[command(flatten)]
    limits: super::SourceLimits,
}

fn parse_binding(value: &str) -> Result<GeneratedTreeBinding, String> {
    if value.len() > 16 * 1024 {
        return Err("expected binding exceeds 16384 bytes".into());
    }
    serde_json::from_str::<super::json_input::Object<GeneratedTreeBinding>>(value)
        .map(|super::json_input::Object(binding)| binding)
        .map_err(|_| "malformed expected binding".into())
}

pub(super) fn run(args: Args) -> ExitCode {
    match Kernel::new().execute(GeneratedTreeCommand(GeneratedTreeRequest {
        allow_native_reads: true,
        root: args.root,
        expected_binding: args.expected_binding,
        manifest_sha256: args.manifest_sha256,
        limits: args.limits.into_limits(),
    })) {
        Ok(verified) => super::write_json(&verified),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no generated-tree verification report produced; no files changed",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
