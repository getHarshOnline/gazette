//! Fresh backup invocation; its durable output is separate from delivery of the receipt.

use gazette_sdk::store::BackupOptions;
use std::{path::PathBuf, process::ExitCode};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing local source database; never initialized or repaired.
    source: PathBuf,
    /// Existing trusted private backup parent, outside authored or publication output.
    parent: PathBuf,
    #[command(flatten)]
    protected: super::store::LifecycleProtectedRootsArgs,
    /// Explicitly permit creation of a new private backup directory.
    #[arg(long)]
    create_backup: bool,
    #[command(flatten)]
    read: super::store::ReadArgs,
    /// Maximum logical and final copied database bytes.
    #[arg(long, default_value_t = BackupOptions::default().database_bytes)]
    max_backup_bytes: u64,
    /// Positive page count per native copy step.
    #[arg(long, default_value_t = BackupOptions::default().pages_per_step)]
    pages_per_step: i32,
    /// Maximum native copy steps; busy results are not silently retried.
    #[arg(long, default_value_t = BackupOptions::default().steps)]
    max_backup_steps: u32,
    #[command(flatten)]
    output: super::store_output::OutputArgs,
}

pub(super) fn run(args: Args) -> ExitCode {
    let reservation = match args.output.reserve() {
        Ok(reservation) => reservation,
        Err(code) => return super::store_output::failure(code),
    };
    let options = BackupOptions {
        allow_create: args.create_backup,
        read: args.read.options(),
        database_bytes: args.max_backup_bytes,
        pages_per_step: args.pages_per_step,
        steps: args.max_backup_steps,
        output: args.output.limits(),
    };
    let result = crate::command::Kernel::new().execute(crate::command::store::Backup {
        source: args.source,
        parent: args.parent,
        protected: args.protected.into_command(),
        options,
    });
    args.output.deliver(&reservation.backup(&result))
}
