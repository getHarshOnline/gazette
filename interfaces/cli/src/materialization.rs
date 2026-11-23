//! Historical configured-output inspection; never current destination custody.

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::store;

mod execute;
pub(super) use execute::{Args as ExecuteArgs, run as execute};
mod documentation;
pub(super) use documentation::{Args as DocumentationArgs, run as documentation};

#[derive(clap::Args)]
pub(super) struct InspectArgs {
    /// Existing private database file; never created, migrated or repaired.
    path: PathBuf,
    /// Exact canonical destination coordinate retained in history; no destination is opened.
    #[arg(long)]
    output_root: PathBuf,
    /// Independently retained complete journal coordinates; omitted means observation only.
    #[arg(long, value_parser = super::journal::parse_header)]
    expected_header: Option<store::JournalHeader>,
    #[command(flatten)]
    read: super::store::ReadArgs,
}

pub(super) fn inspect(args: InspectArgs) -> ExitCode {
    if !args.path.is_absolute() || !args.output_root.is_absolute() {
        let _ = writeln!(
            io::stderr().lock(),
            "gazette: materialization_input: absolute database and historical output coordinates required; no database opened"
        );
        return ExitCode::FAILURE;
    }
    match crate::command::Kernel::new().execute(crate::command::store::InspectMaterialization {
        path: args.path,
        root: args.output_root,
        expected: args.expected_header,
        options: args.read.options(),
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no complete history report returned",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
