//! Explicit detached recovery preparation, separate from receipt delivery.

use gazette_sdk::store::RestoreOptions;
use std::{path::PathBuf, process::ExitCode};

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(flatten)]
    backup: super::verify_backup::Input,
    /// Existing private recovery parent, outside authored, staging and published output.
    parent: PathBuf,
    #[command(flatten)]
    protected: super::store::LifecycleProtectedRootsArgs,
    /// Allow a fresh detached recovery copy, never live-store replacement or activation.
    #[arg(long)]
    prepare_restore: bool,
    #[command(flatten)]
    output: super::store_output::OutputArgs,
}

pub(super) fn run(args: Args) -> ExitCode {
    let reservation = match args.output.reserve() {
        Ok(reservation) => reservation,
        Err(code) => return super::store_output::failure(code),
    };
    let (backup, pin, verification) = args.backup.into_request();
    let options = RestoreOptions {
        allow_prepare: args.prepare_restore,
        verification,
        output: args.output.limits(),
    };
    let result = crate::command::Kernel::new().execute(crate::command::store::PrepareRestore {
        backup,
        manifest_sha256: pin,
        parent: args.parent,
        protected: args.protected.into_command(),
        options,
    });
    args.output.deliver(&reservation.restore(&result))
}
