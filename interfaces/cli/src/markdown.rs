//! Bounded stdin or pinned-tree transport for the same pure Markdown compiler.

use std::io::{self, Write};
use std::process::ExitCode;

use gazette_sdk::{markdown::Limits, toml};

use crate::command::{
    Kernel,
    inspection::{
        FrontMatter as CommandFrontMatter, MarkdownCommand, MarkdownInput, MarkdownLimits,
        MarkdownRequest,
    },
};

mod input;

#[derive(clap::Args)]
pub(super) struct Args {
    #[command(flatten)]
    input: input::Input,
    /// Interpret explicit +++ TOML front matter; absent means Markdown body only.
    #[arg(long, value_enum)]
    front_matter: Option<FrontMatter>,
    /// Maximum TOML metadata bytes before the closing delimiter.
    #[arg(long, requires = "front_matter", default_value_t = toml::Limits::default().source_bytes)]
    max_metadata_bytes: usize,
    /// Maximum TOML nodes, including the root and containers.
    #[arg(long, requires = "front_matter", default_value_t = toml::Limits::default().nodes)]
    max_metadata_nodes: usize,
    /// Maximum TOML depth below its root table.
    #[arg(long, requires = "front_matter", default_value_t = toml::Limits::default().depth)]
    max_metadata_depth: usize,
    /// Also return a bounded HTML fragment; raw HTML and unsafe URLs fail closed.
    #[arg(long)]
    html: bool,
    /// Maximum selected UTF-8 document bytes; stdin may read one extra byte to detect growth.
    #[arg(long, default_value_t = Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Maximum Markdown syntax nodes, including the document root.
    #[arg(long, default_value_t = Limits::default().nodes)]
    max_nodes: usize,
    /// Maximum Markdown syntax depth below the document root.
    #[arg(long, default_value_t = Limits::default().depth)]
    max_depth: usize,
    /// Maximum retained Markdown analysis string bytes, independently of projections.
    #[arg(long, default_value_t = Limits::default().fact_bytes)]
    max_fact_bytes: usize,
    /// Maximum Markdown inspection string bytes and, independently, optional HTML bytes.
    #[arg(long, default_value_t = Limits::default().output_bytes)]
    max_output_bytes: usize,
}

#[derive(Clone, clap::ValueEnum)]
enum FrontMatter {
    Toml,
}

pub(super) fn run(args: Args) -> ExitCode {
    let loaded = match args.input.load(args.max_source_bytes) {
        Ok(loaded) => loaded,
        Err(error) => return failure(error.code, &error.message),
    };
    let limits = Limits {
        source_bytes: args.max_source_bytes,
        nodes: args.max_nodes,
        depth: args.max_depth,
        fact_bytes: args.max_fact_bytes,
        output_bytes: args.max_output_bytes,
    };
    let request = MarkdownRequest {
        input: MarkdownInput::Loaded(loaded),
        front_matter: args.front_matter.map(|_| CommandFrontMatter::Toml),
        html: args.html,
        limits: MarkdownLimits {
            markdown: limits,
            metadata: toml::Limits {
                source_bytes: args.max_metadata_bytes,
                nodes: args.max_metadata_nodes,
                depth: args.max_metadata_depth,
            },
        },
    };
    match Kernel::new().execute(MarkdownCommand(request)) {
        Ok(outcome) => super::write_json(&outcome),
        Err(error) => failure(error.code(), &error),
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no report produced"
    );
    ExitCode::FAILURE
}
