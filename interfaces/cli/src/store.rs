//! Existing-store interface with explicit bookkeeping effects.

use gazette_sdk::store::ReadOptions;
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Existing local `SQLite` database in a trusted private directory.
    path: PathBuf,
    #[command(flatten)]
    read: ReadArgs,
}

#[derive(clap::Args)]
pub(super) struct ReadArgs {
    /// Allow `SQLite` WAL/shared-memory coordination files to be maintained; no SQL writes.
    #[arg(long)]
    allow_coordination_files: bool,
    /// Maximum combined database and existing sidecar bytes before connection.
    #[arg(long, default_value_t = ReadOptions::default().storage_bytes)]
    max_storage_bytes: u64,
    /// Native busy retry milliseconds per lock attempt, not a total deadline.
    #[arg(long, default_value_t = ReadOptions::default().busy_timeout_ms)]
    busy_timeout_ms: u32,
    /// Maximum statement/schema SQL bytes handled by the connection.
    #[arg(long, default_value_t = ReadOptions::default().sql_bytes)]
    max_sql_bytes: i32,
    /// Maximum individual value or row bytes handled by the connection.
    #[arg(long, default_value_t = ReadOptions::default().value_bytes)]
    max_value_bytes: i32,
    /// Maximum VM progress callbacks per native connection, including all session observations.
    #[arg(long, default_value_t = ReadOptions::default().vm_steps)]
    max_vm_steps: u64,
}

impl ReadArgs {
    pub(super) fn options(self) -> ReadOptions {
        ReadOptions {
            allow_coordination_files: self.allow_coordination_files,
            storage_bytes: self.max_storage_bytes,
            busy_timeout_ms: self.busy_timeout_ms,
            sql_bytes: self.max_sql_bytes,
            value_bytes: self.max_value_bytes,
            vm_steps: self.max_vm_steps,
        }
    }
}

#[derive(clap::Args)]
pub(super) struct LifecycleProtectedRootsArgs {
    /// Authored source root protected from private store output; repeat for each root.
    #[arg(long = "authored-root", required = true)]
    authored: Vec<PathBuf>,
    /// Staging root protected from private store output; repeat for each root.
    #[arg(long = "staged-root", required = true)]
    staged: Vec<PathBuf>,
    /// Published/generated output root protected from private store output; repeat for each root.
    #[arg(long = "published-root", required = true)]
    published: Vec<PathBuf>,
    /// Live release-journal root protected from private store output; repeat for each root.
    #[arg(long = "live-journal-root", required = true)]
    live_journals: Vec<PathBuf>,
}

impl LifecycleProtectedRootsArgs {
    pub(super) fn into_command(self) -> crate::command::store::LifecycleProtectedRoots {
        crate::command::store::LifecycleProtectedRoots {
            authored: self.authored,
            staged: self.staged,
            published: self.published,
            live_journals: self.live_journals,
        }
    }
}

pub(super) fn run(args: Args) -> ExitCode {
    match crate::command::Kernel::new().execute(crate::command::store::Observe {
        path: args.path,
        options: args.read.options(),
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no store report produced",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
