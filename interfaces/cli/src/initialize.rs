//! Explicit fresh journal creation, separate from connecting or inspecting.

use gazette_sdk::store::{InitializeOptions, SessionOptions};
use std::{path::PathBuf, process::ExitCode};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing empty private directory, outside source, staging and published output.
    root: PathBuf,
    /// Permit creation of a fresh journal; never reuse, overwrite or migrate a store.
    #[arg(long)]
    create_store: bool,
    /// Permit an exclusive local directory reservation during initialization.
    #[arg(long)]
    reserve_local_coordination: bool,
    /// Maximum main database bytes during creation; journal/metadata overhead is additional.
    #[arg(long, default_value_t = InitializeOptions::default().database_bytes)]
    max_database_bytes: u64,
    #[command(flatten)]
    read: super::store::ReadArgs,
    #[command(flatten)]
    output: super::store_output::OutputArgs,
}

pub(super) fn run(args: Args) -> ExitCode {
    let reservation = match args.output.reserve() {
        Ok(reservation) => reservation,
        Err(code) => return super::store_output::failure(code),
    };
    let options = InitializeOptions {
        allow_create: args.create_store,
        session: SessionOptions {
            allow_local_reservation: args.reserve_local_coordination,
            read: args.read.options(),
        },
        database_bytes: args.max_database_bytes,
        output: args.output.limits(),
    };
    let result = crate::command::Kernel::new().execute(crate::command::store::Initialize {
        directory: args.root,
        options,
    });
    args.output.deliver(&reservation.initialize(&result))
}
