//! Retained-backup inspection with an explicit caller-selected identity.

use gazette_sdk::{source::Sha256Digest, store::BackupVerifyOptions};
use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
};

#[derive(clap::Args)]
pub(super) struct Input {
    /// Existing private backup directory, never created or repaired.
    directory: PathBuf,
    /// Exact digest retained independently from the original backup receipt.
    #[arg(long)]
    manifest_sha256: Sha256Digest,
    #[command(flatten)]
    read: super::store::ReadArgs,
    /// Maximum backup manifest bytes before JSON decoding.
    #[arg(long, default_value_t = BackupVerifyOptions::default().manifest_bytes)]
    max_manifest_bytes: u64,
}

impl Input {
    pub(super) fn into_request(self) -> (PathBuf, Sha256Digest, BackupVerifyOptions) {
        (
            self.directory,
            self.manifest_sha256,
            BackupVerifyOptions {
                read: self.read.options(),
                manifest_bytes: self.max_manifest_bytes,
            },
        )
    }
}

pub(super) fn run(args: Input) -> ExitCode {
    let (directory, pin, options) = args.into_request();
    match crate::command::Kernel::new().execute(crate::command::store::VerifyBackup {
        directory,
        manifest_sha256: pin,
        options,
    }) {
        Ok(report) => super::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no backup verification report produced",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
