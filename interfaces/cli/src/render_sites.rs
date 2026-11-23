//! Private multi-site transport over the shared local acquisition and rendering owners.

use std::{
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use crate::command::{Kernel, publication};
pub(super) use build::LocalPublicationRecipe as Request;
use gazette_sdk::{build, toml};

#[derive(clap::Args)]
pub(super) struct Args {
    /// Presentation year default; omission captures the current UTC year once for this command.
    #[arg(long)]
    pub(super) render_year: Option<gazette_sdk::authored::RenderYear>,
    /// Required acknowledgement: private local output, not public admission or certification.
    #[arg(long, required = true)]
    pub(super) local: bool,
    /// Load a native workspace instead of reading a JSON recipe on stdin.
    #[arg(long)]
    pub(super) workspace_root: Option<PathBuf>,
    /// Root configuration document relative to --workspace-root.
    #[arg(long, requires = "workspace_root", default_value = "gazette.toml")]
    pub(super) workspace: gazette_sdk::source::EntryPath,
    /// Override configured math selection (true or false); does not authorize execution.
    #[arg(long, action = clap::ArgAction::Set)]
    pub(super) math: Option<bool>,
    /// Permit this invocation's configured math, sharing-image and selected documentation generation.
    #[arg(long)]
    pub(super) allow_execute: bool,
    /// Existing separate private parent for explicitly selected documentation generation stages.
    #[arg(long)]
    documentation_generation_parent: Option<PathBuf>,
    /// Bound the complete private JSON stdin request before native source reads.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    pub(super) max_source_bytes: usize,
    /// Maximum aggregate authored corpus bytes, including opaque assets, before parsing.
    #[arg(long)]
    max_corpus_bytes: Option<usize>,
    /// Complete source acquisition and repeated documentation staging entry ceiling.
    #[arg(long, default_value_t = build::PublicationSetAcquisitionLimits::default().sources.entries)]
    max_total_source_entries: usize,
    /// Maximum bytes per declared resource input; aggregate and decoder bounds stay separate.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().publication.output.resources.inputs.file_bytes)]
    max_resource_file_bytes: u64,
    /// Maximum combined output bytes across the explicit selected site set.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().bytes)]
    max_total_output_bytes: u64,
    /// Maximum combined output entries across the explicit selected site set.
    #[arg(long, default_value_t = build::LocalRenderLimits::default().entries)]
    max_total_output_entries: usize,
    /// Per-site generated-output bounds, separate from aggregate set bounds.
    #[command(flatten)]
    output: super::SourceLimits,
}

impl Args {
    pub(super) fn request_bytes(
        &self,
        line: bool,
    ) -> Result<Vec<u8>, (&'static str, &'static str)> {
        let bytes = self.raw_request_bytes(line)?;
        let Some(parent) = &self.documentation_generation_parent else {
            return Ok(bytes);
        };
        gazette_sdk::json::parse(&bytes).map_err(|_| {
            (
                "publication_set_input",
                "request must be one exact JSON object without duplicate keys",
            )
        })?;
        let mut value: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| ("publication_set_input", "invalid JSON request"))?;
        let object = value
            .as_object_mut()
            .ok_or(("publication_set_input", "request must be an object"))?;
        if object.contains_key("documentation_generation") {
            return Err((
                "documentation_generation_selection",
                "choose documentation generation in the request or the flag, not both",
            ));
        }
        object.insert(
            "documentation_generation".into(),
            serde_json::json!({"parent":parent,"allow_execute":self.allow_execute}),
        );
        let encoded = serde_json::to_vec(&value)
            .map_err(|_| ("publication_set_input", "request could not be encoded"))?;
        if encoded.len() > self.max_source_bytes {
            return Err((
                "publication_set_input_limit",
                "complete request exceeds source bytes ceiling",
            ));
        }
        Ok(encoded)
    }
    fn raw_request_bytes(&self, line: bool) -> Result<Vec<u8>, (&'static str, &'static str)> {
        if let Some(root) = &self.workspace_root {
            return serde_json::to_vec(&serde_json::json!({
                "format": "gazette-local-publication-set/1",
                "workspace": {"kind": "local", "root": root, "workspace": self.workspace},
            }))
            .map_err(|_| {
                (
                    "publication_set_input",
                    "workspace request could not be encoded",
                )
            });
        }
        let mut bytes = Vec::new();
        let input = io::stdin();
        let mut bounded = input
            .lock()
            .take((self.max_source_bytes as u64).saturating_add(1));
        if line {
            use std::io::BufRead;
            bounded.read_until(b'\n', &mut bytes)
        } else {
            bounded.read_to_end(&mut bytes)
        }
        .map_err(|_| {
            (
                "publication_set_input",
                "private JSON request could not be read",
            )
        })?;
        if bytes.len() > self.max_source_bytes {
            return Err((
                "publication_set_input_limit",
                "complete request exceeds source bytes ceiling",
            ));
        }
        Ok(bytes)
    }

    pub(super) fn request(
        &self,
        line: bool,
        sites: usize,
    ) -> Result<Request, (&'static str, &'static str)> {
        Request::parse_json(&self.request_bytes(line)?, self.max_source_bytes, sites)
            .and_then(|request| request.with_math_execution(self.math, self.allow_execute))
            .map_err(|reason| ("publication_set_input", reason))
    }

    // One CLI mapping feeds rendering and the SDK-owned preview lifecycle.
    pub(super) fn limits(
        self,
    ) -> (
        gazette_sdk::source::Limits,
        build::LocalRenderLimits,
        toml::Limits,
        build::PublicationSetAcquisitionLimits,
    ) {
        let mut acquisition = build::PublicationSetAcquisitionLimits::default();
        acquisition.sources.entries = self.max_total_source_entries;
        (
            self.output.into_limits(),
            build::PublicationSetRenderLimits {
                publication: build::PublicationRenderLimits {
                    corpus: build::CorpusLimits {
                        source_bytes: self.max_corpus_bytes,
                        ..Default::default()
                    },
                    output: build::PublicationOutputLimits {
                        resources: gazette_sdk::resource::Limits {
                            inputs: gazette_sdk::source::Limits {
                                file_bytes: self.max_resource_file_bytes,
                                ..gazette_sdk::resource::Limits::default().inputs
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                },
                bytes: self.max_total_output_bytes,
                entries: self.max_total_output_entries,
            },
            toml::Limits {
                source_bytes: self.max_source_bytes,
                ..Default::default()
            },
            acquisition,
        )
    }
}

pub(super) fn run(args: Args) -> ExitCode {
    let bytes = match args.request_bytes(false) {
        Ok(bytes) => bytes,
        Err((code, reason)) => return failure(code, &reason),
    };
    let render_year = match crate::calendar::resolve(args.render_year) {
        Ok(year) => year,
        Err(error) => return failure("render_year", &error),
    };
    let local = args.local;
    let math = args.math;
    let allow_math_execution = args.allow_execute;
    let max_input_bytes = args.max_source_bytes;
    let (output_limits, render_limits, syntax, acquisition) = args.limits();
    match Kernel::new().execute(publication::Render {
        default_render_year: Some(render_year),
        recipe_json: bytes,
        math,
        allow_math_execution,
        local,
        allow_read: true,
        max_input_bytes,
        syntax,
        acquisition,
        output: output_limits,
        limits: render_limits,
    }) {
        Ok(outcome) => {
            let delivered = if outcome.report().get("documentation").is_some() {
                match serde_json::to_vec_pretty(outcome.report()) {
                    Ok(mut bytes) => {
                        bytes.push(b'\n');
                        match crate::deliver_json(&bytes) {
                            Ok(()) => ExitCode::SUCCESS,
                            Err(error) => {
                                let mut diagnostic = io::stderr().lock();
                                let _ = writeln!(
                                    diagnostic,
                                    "gazette: output_io: {error}; original documentation and publication evidence follows; inspect before retrying"
                                );
                                let _ = diagnostic.write_all(&bytes);
                                ExitCode::FAILURE
                            }
                        }
                    }
                    Err(error) => failure("documentation_publication_response", &error),
                }
            } else {
                super::write_json(outcome.report())
            };
            if delivered == ExitCode::SUCCESS && outcome.is_complete() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => failure(error.code(), &error),
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(io::stderr().lock(), "gazette: {code}: {error}");
    ExitCode::FAILURE
}

#[cfg(test)]
#[path = "../tests/unit/render_sites.rs"]
mod tests;
