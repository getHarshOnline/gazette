//! Direct selected-directory transport for SDK-owned whole-set TOML inspection.

use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use gazette_sdk::{configuration, source, toml};

use crate::command::{
    Kernel,
    inspection::{TomlCommand, TomlRequest},
};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Explicit configuration directory; every file is TOML, including hidden files.
    root: PathBuf,
    /// Literal relative subdirectory, confined beneath ROOT without following links.
    #[arg(long)]
    subdir: Option<source::Subdirectory>,
    /// Maximum native files and directories captured before parsing.
    #[arg(long, default_value_t = configuration::Limits::default().source.entries)]
    max_entries: usize,
    /// Maximum native descendant depth, not TOML nesting.
    #[arg(long, default_value_t = configuration::Limits::default().source.depth)]
    max_path_depth: usize,
    /// Maximum distinct TOML documents in the complete set.
    #[arg(long, default_value_t = toml::SetLimits::default().documents)]
    max_documents: usize,
    /// Maximum source bytes in one document, enforced during capture and parsing.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_document_bytes: usize,
    /// Maximum source bytes across the whole set, enforced during capture and parsing.
    #[arg(long, default_value_t = toml::SetLimits::default().source_bytes)]
    max_total_bytes: usize,
    /// Maximum combined relative document-path bytes retained by the compiler.
    #[arg(long, default_value_t = toml::SetLimits::default().path_bytes)]
    max_path_bytes: usize,
    /// Maximum syntax nodes in one document, including its root and containers.
    #[arg(long, default_value_t = toml::Limits::default().nodes)]
    max_document_nodes: usize,
    /// Maximum syntax nodes across all documents, including their roots.
    #[arg(long, default_value_t = toml::SetLimits::default().nodes)]
    max_total_nodes: usize,
    /// Maximum TOML nesting below each document root.
    #[arg(long, default_value_t = toml::Limits::default().depth)]
    max_depth: usize,
}

pub(super) fn run(args: Args) -> ExitCode {
    let limits = configuration::Limits {
        source: source::Limits {
            entries: args.max_entries,
            depth: args.max_path_depth,
            file_bytes: args.max_document_bytes as u64,
            total_bytes: args.max_total_bytes as u64,
        },
        syntax: toml::SetLimits {
            documents: args.max_documents,
            source_bytes: args.max_total_bytes,
            path_bytes: args.max_path_bytes,
            nodes: args.max_total_nodes,
            document: toml::Limits {
                source_bytes: args.max_document_bytes,
                nodes: args.max_document_nodes,
                depth: args.max_depth,
            },
        },
    };
    match Kernel::new().execute(TomlCommand(TomlRequest {
        allow_native_reads: true,
        root: args.root,
        subdirectory: args.subdir,
        limits,
    })) {
        Ok(report) => super::write_json(&report),
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "gazette: {}: {error}; no configuration report produced",
                error.code()
            );
            ExitCode::FAILURE
        }
    }
}
