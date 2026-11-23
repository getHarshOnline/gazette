//! Durable release observations through native SDK readers, never renewed authority.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{Kernel, observation};
use gazette_sdk::store;

#[derive(clap::Args)]
pub(super) struct Args {
    /// Exact existing private database; never initialized, migrated or repaired.
    path: PathBuf,
    /// Exact durable authorization record identifier; no latest-record inference.
    record_id: String,
    /// Independently retained complete coordinates; omitted means observation only.
    #[arg(long, value_parser = super::journal::parse_header)]
    expected_header: Option<store::JournalHeader>,
    /// Explicitly permit reading private durable release history.
    #[arg(long)]
    allow_read: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

pub(super) enum Kind {
    Authorization,
    Leases,
    Consumption,
    Attempts,
    Reconciliations,
}

pub(super) fn inspect(args: Args, kind: Kind) -> ExitCode {
    let kind = match kind {
        Kind::Authorization => observation::HistoryKind::Authorization,
        Kind::Leases => observation::HistoryKind::Leases,
        Kind::Consumption => observation::HistoryKind::Consumption,
        Kind::Attempts => observation::HistoryKind::Attempts,
        Kind::Reconciliations => observation::HistoryKind::Reconciliations,
    };
    match Kernel::new().execute(observation::InspectHistory {
        allow_native_reads: args.allow_read,
        path: args.path,
        record_id: args.record_id,
        expected_header: args.expected_header,
        read: args.read.options(),
        kind,
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => failure(error.code(), &error),
    }
}

fn failure(code: &str, error: &dyn std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no release history report produced"
    );
    ExitCode::FAILURE
}
