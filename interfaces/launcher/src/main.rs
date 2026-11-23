//! Builds the single `gazette` executable from the command line, MCP server and language
//! server.
//!
//! This binary builds on `gazette-cli`, `gazette-mcp` and `gazette-lsp`, and nothing
//! builds on it. It reads the invocation `gazette-cli` parses and starts the matching
//! interface.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match gazette_cli::run() {
        gazette_cli::Invocation::Application(status) => status,
        gazette_cli::Invocation::Lsp => match gazette_lsp::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "gazette: LSP session failed; no publication operation performed"
                );
                ExitCode::FAILURE
            }
        },
        gazette_cli::Invocation::Mcp(args) => {
            let defaults = gazette_mcp::TransportLimits::default();
            let limits = match gazette_mcp::TransportLimits::new(
                args.max_frame_bytes.unwrap_or(defaults.max_frame_bytes()),
                args.max_response_bytes
                    .unwrap_or(defaults.max_response_bytes()),
            ) {
                Ok(limits) => limits,
                Err(error) => {
                    let _ = writeln!(io::stderr().lock(), "gazette: {error}");
                    return ExitCode::from(2);
                }
            };
            match gazette_mcp::run_with_limits(limits) {
                Ok(()) => ExitCode::SUCCESS,
                Err(_) => {
                    let _ = writeln!(
                        io::stderr().lock(),
                        "gazette: MCP transport failed; admitted operations may have run or remain active; preserve original requests and inspect native outcomes before further work; do not infer rollback or retry"
                    );
                    ExitCode::FAILURE
                }
            }
        }
    }
}
