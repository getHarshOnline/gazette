//! Read-only workspace compilation from one standard local manifest or explicit supplied input.

use std::{
    env,
    io::{self, Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use gazette_sdk::{build, configuration, corpus, publication::content, source, toml, workspace};

use crate::command::{
    Kernel,
    configuration::{
        CompositionCommand, CompositionOperation, CompositionRequest, LoadWorkspaceCommand,
        LoadWorkspaceRequest, PublicationCommand, PublicationOperation, PublicationRequest,
        SiteDocumentsCommand, SiteDocumentsOperation, SiteDocumentsRequest, WorkspaceCommand,
        WorkspaceOperation, WorkspaceRequest,
    },
};

const STANDARD_WORKSPACE: &str = "gazette.toml";
mod origin;
mod why;

#[derive(clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
#[command(group(clap::ArgGroup::new("corpus_mode").args(["parse_documents", "publication"])))]
#[command(group(clap::ArgGroup::new("native_inspection").args(["acquire_site", "acquire_composition", "publication", "why"])))]
#[command(group(clap::ArgGroup::new("publication_review").args(["review", "max_review_bytes", "max_review_uses", "retained_math_stdin"]).multiple(true)))]
#[command(group(clap::ArgGroup::new("publication_registry").args(["registry", "max_registry_bytes"]).multiple(true)))]
pub(super) struct Args {
    /// Read workspace TOML from standard input instead of finding a local workspace.
    #[arg(long, conflicts_with = "workspace_root")]
    stdin: bool,
    /// Explain a publication field's value and its site, publisher or default origin.
    #[arg(long, requires = "site", conflicts_with_all = ["origin", "composition_origin", "acquire_site", "parse_documents", "check_composition", "compile_templates"])]
    why: Option<String>,
    /// Site whose publication field is explained by --why.
    #[arg(long, requires = "why")]
    site: Option<workspace::Key>,
    /// Validate a complete local publication-set recipe through the exact build path, without retaining output.
    #[command(subcommand)]
    operation: Option<Operation>,
    /// Choose a workspace folder instead of searching from the current folder.
    #[arg(long)]
    workspace_root: Option<PathBuf>,
    /// Exact root-relative configuration file selected with --workspace-root.
    #[arg(long, requires = "workspace_root", default_value = STANDARD_WORKSPACE)]
    workspace_file: source::EntryPath,
    /// Locate an explicit field: JSON {"kind":"brand","identity":"example:brand","field":["name"]}.
    // Conflict every effect-specific selector explicitly: a conflict on a
    // required argument can otherwise suppress Clap's indirect requirements.
    #[arg(long, conflicts_with_all = [
        "acquire_site", "acquire_composition", "publication", "check_composition", "compile_templates", "parse_documents", "max_corpus_files", "max_corpus_bytes",
        "max_corpus_nodes", "max_corpus_metadata_nodes", "max_placements",
        "max_acquired_entries", "max_acquired_bytes", "publication_review", "publication_registry",
    ])]
    origin: Option<String>,
    /// Explain an effective selection: JSON {"site":"site:one","field":{"kind":"theme"},"path":["source"]}.
    #[arg(long, conflicts_with_all = [
        "origin", "acquire_site", "acquire_composition", "publication", "check_composition", "compile_templates", "parse_documents", "max_corpus_files", "max_corpus_bytes",
        "max_corpus_nodes", "max_corpus_metadata_nodes", "max_placements",
        "max_acquired_entries", "max_acquired_bytes", "publication_review", "publication_registry",
    ])]
    composition_origin: Option<String>,
    /// Also read and retain this exact site's pinned documentation; no writes, network or certification.
    #[arg(long, group = "acquisition", conflicts_with_all = ["publication_review", "publication_registry"])]
    acquire_site: Option<workspace::Key>,
    /// Retain this site's selected pinned Theme/capability sources; not compatibility or a build.
    #[arg(long, group = "acquisition", conflicts_with_all = [
        "parse_documents", "max_corpus_files", "max_corpus_bytes",
        "max_corpus_nodes", "max_corpus_metadata_nodes", "publication_review", "publication_registry",
    ])]
    acquire_composition: Option<workspace::Key>,
    /// Resolve all configured publication content from pinned inputs; private unadmitted evidence, no rendering or writes.
    #[arg(long, group = "acquisition", conflicts_with_all = [
        "origin", "composition_origin", "acquire_site", "acquire_composition",
        "check_composition", "compile_templates", "parse_documents",
    ])]
    publication: Option<workspace::Key>,
    /// Review exact unadmitted projections and bindings; no approval, templates or resource execution.
    #[arg(
        long,
        requires = "publication",
        conflicts_with = "publication_registry"
    )]
    review: bool,
    /// Read complete source-bound math-result JSON from stdin for --review; no typesetter execution.
    #[arg(long, requires_all = ["review", "workspace_root"])]
    retained_math_stdin: bool,
    /// Inspect the complete private registry projection; no database connection or writes.
    #[arg(long, requires = "publication", conflicts_with = "publication_review")]
    registry: bool,
    /// Maximum compact registry JSON bytes, excluding display whitespace; no partial projection.
    #[arg(long, requires = "registry", default_value_t = build::RegistryLimits::default().bytes)]
    max_registry_bytes: usize,
    /// Maximum aggregate review bytes, independent of acquisition and corpus limits.
    #[arg(long, requires = "review", default_value_t = build::ReviewLimits::default().bytes)]
    max_review_bytes: usize,
    /// Maximum retained review uses; exhaustion returns no partial review.
    #[arg(long, requires = "review", default_value_t = build::ReviewLimits::default().uses)]
    max_review_uses: usize,
    /// Validate captured presentation declarations and selected files; no provider execution.
    #[arg(long, requires = "acquire_composition", conflicts_with_all = [
        "acquire_site", "parse_documents", "max_corpus_files", "max_corpus_bytes",
        "max_corpus_nodes", "max_corpus_metadata_nodes", "publication_review", "publication_registry",
    ])]
    check_composition: bool,
    /// Check metadata and compile the selected retained Theme templates; no build or writes.
    #[arg(long, requires = "acquire_composition", conflicts_with_all = [
        "origin", "composition_origin", "check_composition", "acquire_site", "parse_documents",
        "max_corpus_files", "max_corpus_bytes", "max_corpus_nodes", "max_corpus_metadata_nodes", "publication_review", "publication_registry",
    ])]
    compile_templates: bool,
    /// Validate acquired Markdown/notebook syntax and account for assets; not rendering or publication approval.
    #[arg(long, requires = "acquire_site", conflicts_with_all = ["publication_review", "publication_registry"])]
    parse_documents: bool,
    /// Maximum files in the syntax corpus, including opaque assets.
    #[arg(long, requires = "corpus_mode", default_value_t = corpus::Limits::default().files)]
    max_corpus_files: usize,
    /// Maximum aggregate corpus source bytes, including assets.
    #[arg(long, requires = "corpus_mode", default_value_t = corpus::Limits::default().source_bytes)]
    max_corpus_bytes: usize,
    /// Maximum retained Markdown syntax nodes across all documents.
    #[arg(long, requires = "corpus_mode", default_value_t = corpus::Limits::default().markdown_nodes)]
    max_corpus_nodes: usize,
    /// Maximum retained TOML nodes across all document frontmatter.
    #[arg(long, requires = "corpus_mode", default_value_t = corpus::Limits::default().metadata_nodes)]
    max_corpus_metadata_nodes: usize,
    /// Maximum documentation placements or unique composition/publication source bindings.
    #[arg(long, requires = "acquisition", default_value_t = 256)]
    max_placements: usize,
    /// Maximum retained entries across the selected acquisition.
    #[arg(long, requires = "acquisition", default_value_t = source::Limits::default().entries)]
    max_acquired_entries: usize,
    /// Maximum retained bytes across the selected acquisition.
    #[arg(long, requires = "acquisition", default_value_t = source::Limits::default().total_bytes)]
    max_acquired_bytes: u64,
    /// Maximum bytes in one workspace TOML document or supplied input.
    #[arg(long, default_value_t = toml::Limits::default().source_bytes)]
    max_source_bytes: usize,
    /// Maximum syntax nodes, bounding declarations before reference resolution.
    #[arg(long, default_value_t = toml::Limits::default().nodes)]
    max_nodes: usize,
    /// Maximum TOML nesting below the root.
    #[arg(long, default_value_t = toml::Limits::default().depth)]
    max_depth: usize,
}

#[derive(clap::Subcommand)]
enum Operation {
    /// Render the exact requested site set in memory; no stage, certification, release or publication effect.
    Build(super::render_sites::Args),
    /// Load one explicit modular workspace; no cwd discovery, build, write or publication effect.
    Workspace(LoadWorkspaceArgs),
}

#[derive(clap::Args)]
struct LoadWorkspaceArgs {
    /// Explicit absolute native root containing the selected configuration documents.
    root: PathBuf,
    /// Exact root-relative workspace document; includes are declared by this file only.
    #[arg(long)]
    workspace: source::EntryPath,
    /// Permit the exact bounded native reads selected by ROOT and WORKSPACE.
    #[arg(long)]
    allow_read: bool,
    /// Maximum selected files and necessary ancestor directories.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().source.entries)]
    max_entries: usize,
    /// Maximum selected root-relative path depth.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().source.depth)]
    max_path_depth: usize,
    /// Maximum distinct workspace documents.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.documents)]
    max_documents: usize,
    /// Maximum source bytes in one workspace document.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.document.source_bytes)]
    max_document_bytes: usize,
    /// Maximum source bytes across the complete workspace document set.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.source_bytes)]
    max_total_bytes: usize,
    /// Maximum combined root-relative document-path bytes.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.path_bytes)]
    max_path_bytes: usize,
    /// Maximum syntax nodes in one workspace document.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.document.nodes)]
    max_document_nodes: usize,
    /// Maximum syntax nodes across all workspace documents.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.nodes)]
    max_total_nodes: usize,
    /// Maximum TOML nesting below each document root.
    #[arg(long, default_value_t = configuration::WorkspaceLoadLimits::default().syntax.document.depth)]
    max_depth: usize,
}

impl LoadWorkspaceArgs {
    fn limits(&self) -> configuration::WorkspaceLoadLimits {
        configuration::WorkspaceLoadLimits {
            source: source::Limits {
                entries: self.max_entries,
                depth: self.max_path_depth,
                file_bytes: self.max_document_bytes as u64,
                total_bytes: self.max_total_bytes as u64,
            },
            syntax: toml::SetLimits {
                documents: self.max_documents,
                source_bytes: self.max_total_bytes,
                path_bytes: self.max_path_bytes,
                nodes: self.max_total_nodes,
                document: toml::Limits {
                    source_bytes: self.max_document_bytes,
                    nodes: self.max_document_nodes,
                    depth: self.max_depth,
                },
            },
        }
    }
}

pub(super) fn run(args: Args) -> ExitCode {
    if args.why.is_some() {
        return why::run(&args);
    }
    let plain_workspace_check = is_plain_workspace_check(&args);
    match args.operation {
        Some(Operation::Build(build)) => return super::render_sites::run(build),
        Some(Operation::Workspace(input)) => {
            let limits = input.limits();
            return load_workspace(input.root, input.workspace, input.allow_read, limits);
        }
        None => {}
    }
    let limits = toml::Limits {
        source_bytes: args.max_source_bytes,
        nodes: args.max_nodes,
        depth: args.max_depth,
    };
    if plain_workspace_check && !args.stdin {
        return load_current_workspace(&args);
    }
    if !args.stdin && (args.origin.is_some() || args.composition_origin.is_some()) {
        return origin::run(&args);
    }
    let location = if args.stdin {
        None
    } else {
        match locate_workspace(&args) {
            Ok(location) => Some(location),
            Err(error) => return failure("workspace_location", &error),
        }
    };
    let mut bytes = Vec::new();
    let retained_math = if args.retained_math_stdin {
        let mut input = Vec::new();
        if let Err(error) = io::stdin()
            .lock()
            .take((args.max_review_bytes as u64).saturating_add(1))
            .read_to_end(&mut input)
        {
            return failure("publication_review_math_input", &error);
        }
        if input.len() > args.max_review_bytes {
            return failure(
                "publication_review_math_limit",
                &"retained math JSON byte ceiling exceeded",
            );
        }
        let value = match serde_json::from_slice(&input) {
            Ok(value) => value,
            Err(error) => return failure("publication_review_math_input", &error),
        };
        match corpus::decode_retained_math(value) {
            Ok(results) => Some(results),
            Err(error) => return failure("publication_review_math_input", &error),
        }
    } else {
        None
    };
    if args.stdin
        && let Err(error) = io::stdin()
            .lock()
            .take((limits.source_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
    {
        return failure("workspace_input", &error);
    }
    // Enforce the byte ceiling before decoding, matching the pure parser's precedence.
    if bytes.len() > limits.source_bytes {
        return failure("toml_limit", &"TOML source bytes ceiling exceeded");
    }
    let source = match std::str::from_utf8(&bytes) {
        Ok(source) => source,
        Err(error) => return failure("workspace_utf8", &error),
    };
    let inspection_source = match location {
        Some(location) => build::WorkspaceSource::Selected(build::WorkspaceInput::Local {
            root: location.root,
            workspace: location.workspace,
            limits: workspace_limits(&args),
        }),
        None => source.into(),
    };
    let kernel = Kernel::new();
    if let Some(query) = args.origin {
        let query: workspace::FieldQuery = match serde_json::from_str(&query) {
            Ok(query) => query,
            Err(error) => return failure("workspace_origin_query", &error),
        };
        return execute(
            &kernel,
            WorkspaceCommand(WorkspaceRequest {
                source: source.into(),
                limits,
                operation: WorkspaceOperation::Field(query),
            }),
        );
    }
    if let Some(query) = args.composition_origin {
        let query: workspace::CompositionOriginQuery = match serde_json::from_str(&query) {
            Ok(query) => query,
            Err(error) => return failure("workspace_origin_query", &error),
        };
        return execute(
            &kernel,
            WorkspaceCommand(WorkspaceRequest {
                source: source.into(),
                limits,
                operation: WorkspaceOperation::CompositionOrigin(query),
            }),
        );
    }
    let acquisition_limits = source::PreparationLimits {
        sources: args.max_placements,
        entries: args.max_acquired_entries,
        bytes: args.max_acquired_bytes,
        per_source: source::RevisionLimits::default(),
    };
    let corpus_limits = corpus::Limits {
        files: args.max_corpus_files,
        source_bytes: args.max_corpus_bytes,
        markdown_nodes: args.max_corpus_nodes,
        metadata_nodes: args.max_corpus_metadata_nodes,
        ..corpus::Limits::default()
    };
    if let Some(site) = args.publication {
        let operation = if args.registry {
            PublicationOperation::Registry(build::RegistryLimits {
                bytes: args.max_registry_bytes,
            })
        } else if args.review {
            PublicationOperation::Review(build::ReviewLimits {
                bytes: args.max_review_bytes,
                uses: args.max_review_uses,
                ..build::ReviewLimits::default()
            })
        } else {
            PublicationOperation::Inspect
        };
        return execute(
            &kernel,
            PublicationCommand(PublicationRequest {
                source: inspection_source,
                site,
                allow_native_reads: true,
                syntax: limits,
                acquisition: acquisition_limits,
                corpus: corpus_limits,
                graph: content::Limits::default(),
                retained_math,
                operation,
            }),
        );
    }
    if let Some(site) = args.acquire_composition {
        let operation = if args.compile_templates {
            CompositionOperation::Compile(gazette_sdk::render::Limits::default())
        } else if args.check_composition {
            CompositionOperation::Check
        } else {
            CompositionOperation::Inspect
        };
        return execute(
            &kernel,
            CompositionCommand(CompositionRequest {
                source: inspection_source,
                site,
                allow_native_reads: true,
                syntax: limits,
                acquisition: acquisition_limits,
                operation,
            }),
        );
    }
    if let Some(site) = args.acquire_site {
        let operation = if args.parse_documents {
            SiteDocumentsOperation::Parse(corpus_limits)
        } else {
            SiteDocumentsOperation::Inspect
        };
        return execute(
            &kernel,
            SiteDocumentsCommand(SiteDocumentsRequest {
                source: inspection_source,
                site,
                allow_native_reads: true,
                syntax: limits,
                acquisition: acquisition_limits,
                operation,
            }),
        );
    }
    execute(
        &kernel,
        WorkspaceCommand(WorkspaceRequest {
            source: source.into(),
            limits,
            operation: WorkspaceOperation::Compile,
        }),
    )
}

fn is_plain_workspace_check(args: &Args) -> bool {
    args.why.is_none()
        && args.origin.is_none()
        && args.composition_origin.is_none()
        && args.acquire_site.is_none()
        && args.acquire_composition.is_none()
        && args.publication.is_none()
        && !args.review
        && !args.registry
        && !args.check_composition
        && !args.compile_templates
        && !args.parse_documents
}

fn locate_workspace(args: &Args) -> Result<configuration::WorkspaceLocation, io::Error> {
    let start = env::current_dir()?;
    let request = args
        .workspace_root
        .as_ref()
        .map(|root| root.join(args.workspace_file.as_str()));
    let mut location = configuration::locate_user_workspace(request.as_deref(), None, &start)?.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no gazette.toml found; open a publication folder or choose one with --workspace-root",
        )
    })?;
    if let Some(root) = &args.workspace_root {
        location.root = root.canonicalize()?;
        location.workspace = args.workspace_file.clone();
    }
    Ok(location)
}

fn workspace_limits(args: &Args) -> configuration::WorkspaceLoadLimits {
    configuration::WorkspaceLoadLimits {
        source: source::Limits {
            file_bytes: args.max_source_bytes as u64,
            ..Default::default()
        },
        syntax: toml::SetLimits {
            document: toml::Limits {
                source_bytes: args.max_source_bytes,
                nodes: args.max_nodes,
                depth: args.max_depth,
            },
            ..Default::default()
        },
    }
}

fn load_current_workspace(args: &Args) -> ExitCode {
    let location = match locate_workspace(args) {
        Ok(location) => location,
        Err(error) => return failure("workspace_location", &error),
    };
    let report = build::check_workspace(
        location,
        workspace_limits(args),
        build::PublicationSetAcquisitionLimits::default().sources,
        build::CorpusLimits::default(),
    );
    for diagnostic in &report.diagnostics {
        eprintln!("{diagnostic}");
    }
    let output = super::write_json(&report);
    if !report.is_valid() {
        ExitCode::FAILURE
    } else {
        output
    }
}

fn load_workspace(
    root: PathBuf,
    workspace: source::EntryPath,
    allow_native_reads: bool,
    limits: configuration::WorkspaceLoadLimits,
) -> ExitCode {
    execute(
        &Kernel::new(),
        LoadWorkspaceCommand(LoadWorkspaceRequest {
            root,
            workspace,
            allow_native_reads,
            limits,
        }),
    )
}

fn write_located<T: serde::Serialize>(
    location: &configuration::WorkspaceLocation,
    result: &T,
) -> ExitCode {
    #[derive(serde::Serialize)]
    struct Located<'a, T> {
        workspace_location: &'a configuration::WorkspaceLocation,
        #[serde(flatten)]
        result: &'a T,
    }
    super::write_json(&Located {
        workspace_location: location,
        result,
    })
}

fn execute<C>(kernel: &Kernel, command: C) -> ExitCode
where
    C: crate::command::Command<Failure = crate::command::configuration::Failure>,
    C::Outcome: serde::Serialize,
{
    match kernel.execute(command) {
        Ok(outcome) => super::write_json(&outcome),
        Err(error) => {
            let code = if matches!(
                error.code(),
                "origin_unknown_identity" | "origin_no_explicit_field"
            ) {
                "workspace_origin"
            } else {
                error.code()
            };
            failure(code, &error)
        }
    }
}

fn failure(code: &str, error: &impl std::fmt::Display) -> ExitCode {
    let _ = writeln!(
        io::stderr().lock(),
        "gazette: {code}: {error}; no successful check report produced"
    );
    ExitCode::FAILURE
}
