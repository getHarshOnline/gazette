//! Read-only journal inspection, optionally bound to independently retained coordinates.

use gazette_sdk::store::JournalHeader;
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing private journal database; never initialized, migrated or repaired.
    path: PathBuf,
    /// Complete prior header as JSON, retained independently; mismatches fail without writes.
    #[arg(long, value_parser = parse_header)]
    expected_header: Option<JournalHeader>,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

pub(super) fn parse_header(value: &str) -> Result<JournalHeader, String> {
    // A fixed header is small; bound decoding independently of store/SQL limits.
    if value.len() > 4096 {
        return Err("expected journal header exceeds 4096 bytes".into());
    }
    serde_json::from_str(value)
        .map_err(|_| "expected a complete journal header JSON object; no store was opened".into())
}

pub(super) fn run(args: Args) -> ExitCode {
    match crate::command::Kernel::new().execute(crate::command::store::InspectJournal {
        path: args.path,
        expected: args.expected_header,
        options: args.read.options(),
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no journal inspection report produced",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
