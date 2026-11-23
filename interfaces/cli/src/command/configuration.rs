//! Configuration and composition commands shared by CLI and MCP.

use std::{fmt, path::PathBuf};

use gazette_sdk::{
    build, configuration as sdk_configuration, corpus, publication::content, render, source, toml,
    workspace,
};
use serde::Serialize;
use serde_json::Value;

use super::{Command, Kernel};

/// Failure retained before a CLI or protocol chooses its presentation.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
    structured: Option<Value>,
}

impl Failure {
    fn native(code: &'static str, error: &impl fmt::Display) -> Self {
        Self {
            code,
            message: error.to_string(),
            structured: None,
        }
    }

    fn workspace(error: &workspace::Error) -> Self {
        let failure = workspace::Failure::from(error);
        Self {
            code: failure.code,
            message: failure.message.clone(),
            structured: serde_json::to_value(failure).ok(),
        }
    }

    fn origin(error: workspace::OriginError) -> Self {
        let failure = workspace::Failure::from(error);
        Self {
            code: failure.code,
            message: failure.message.clone(),
            structured: serde_json::to_value(failure).ok(),
        }
    }

    fn permission(code: &'static str, message: &'static str) -> Self {
        Self {
            code,
            message: message.into(),
            structured: None,
        }
    }

    fn projection(error: serde_json::Error) -> Self {
        Self {
            code: "command_projection",
            message: error.to_string(),
            structured: None,
        }
    }

    /// Stable native failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// Private native explanation for the selected transport.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Workspace diagnostics retain their exact established wire projection.
    #[must_use]
    pub fn structured(&self) -> Option<&Value> {
        self.structured.as_ref()
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// One owned transport-neutral result. Its variant records the executed phase.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Outcome {
    /// Exact native configuration inventory and compiled modular workspace.
    LoadedWorkspace(Box<sdk_configuration::LoadedWorkspace>),
    /// Complete workspace compilation.
    Workspace(Box<workspace::Compilation>),
    /// Exact explicit field origin.
    Field(workspace::FieldOrigin),
    /// Effective composition selection origin.
    CompositionOrigin(workspace::CompositionFieldOrigin),
    /// Exact acquired composition evidence.
    Composition(Value),
    /// Checked composition metadata evidence.
    CompositionMetadata(Value),
    /// Compiled template evidence.
    Templates(Value),
    /// Exact selected-site acquisition evidence.
    Site(Value),
    /// Parsed selected-site document evidence.
    SiteDocuments(Value),
    /// Resolved private publication evidence.
    Publication(Value),
    /// Complete private publication review.
    Review(Value),
    /// Complete private registry projection.
    Registry(Value),
}

/// Exact explicit native modular-workspace request.
pub struct LoadWorkspaceRequest {
    /// Absolute native root; never inferred from process state.
    pub root: PathBuf,
    /// Exact root-relative workspace document.
    pub workspace: source::EntryPath,
    /// Explicit permission for the selected native reads.
    pub allow_native_reads: bool,
    /// Native capture and complete document-set ceilings.
    pub limits: sdk_configuration::WorkspaceLoadLimits,
}

/// Load the explicitly selected modular workspace through its one SDK owner.
pub struct LoadWorkspaceCommand(pub LoadWorkspaceRequest);

impl Command for LoadWorkspaceCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::permission(
                "workspace_load_permission",
                "Explicit native workspace read permission required; no source acquisition started",
            ));
        }
        sdk_configuration::load_workspace(&request.root, &request.workspace, request.limits)
            .map(Box::new)
            .map(Outcome::LoadedWorkspace)
            .map_err(|error| Failure::native(error.code(), &error))
    }
}

/// Pure workspace operation over explicitly supplied TOML.
pub enum WorkspaceOperation {
    /// Compile the complete workspace.
    Compile,
    /// Find one explicit declaration.
    Field(workspace::FieldQuery),
    /// Explain one effective composition selection.
    CompositionOrigin(workspace::CompositionOriginQuery),
}

/// Exact supplied workspace command.
pub struct WorkspaceRequest {
    /// Complete UTF-8 TOML source.
    pub source: String,
    /// Native parser ceilings.
    pub limits: toml::Limits,
    /// Selected pure operation.
    pub operation: WorkspaceOperation,
}

/// Execute one supplied-workspace operation.
pub struct WorkspaceCommand(pub WorkspaceRequest);

impl Command for WorkspaceCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if matches!(&request.operation, WorkspaceOperation::Compile) {
            return workspace::compile(&request.source, request.limits)
                .map(Box::new)
                .map(Outcome::Workspace)
                .map_err(|error| Failure::workspace(&error));
        }
        let parsed = workspace::ParsedWorkspace::parse(&request.source, request.limits)
            .map_err(|error| Failure::workspace(&error))?;
        match request.operation {
            WorkspaceOperation::Compile => unreachable!("compile returned before parsed queries"),
            WorkspaceOperation::Field(query) => parsed
                .origin(&query)
                .map(Outcome::Field)
                .map_err(Failure::origin),
            WorkspaceOperation::CompositionOrigin(query) => parsed
                .composition_origin(&query)
                .map(Outcome::CompositionOrigin)
                .map_err(Failure::origin),
        }
    }
}

/// Selected composition phase.
pub enum CompositionOperation {
    /// Retain exact source evidence only.
    Inspect,
    /// Validate all selected presentation declarations and inputs.
    Check,
    /// Validate metadata and compile the selected templates.
    Compile(render::Limits),
}

/// Exact configured composition command.
pub struct CompositionRequest {
    /// Complete UTF-8 workspace TOML.
    pub source: build::WorkspaceSource,
    /// Exact configured site identity.
    pub site: workspace::Key,
    /// Explicit permission for configured native source reads.
    pub allow_native_reads: bool,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Composition source-acquisition ceilings.
    pub acquisition: source::PreparationLimits,
    /// Selected composition phase.
    pub operation: CompositionOperation,
}

/// Execute one composition phase without transport-specific orchestration.
pub struct CompositionCommand(pub CompositionRequest);

impl Command for CompositionCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::permission(
                "composition_permission",
                "Explicit configured-source read permission required; no source acquisition started",
            ));
        }
        let acquired = build::acquire_selected_composition(
            &request.source,
            &request.site,
            request.syntax,
            request.acquisition,
        )
        .map_err(|error| Failure::native(error.code(), &error))?;
        match request.operation {
            CompositionOperation::Inspect => serde_json::to_value(acquired.inspect())
                .map(Outcome::Composition)
                .map_err(Failure::projection),
            CompositionOperation::Check => acquired
                .check_metadata(request.syntax)
                .map_err(|error| Failure::native(error.code(), &error))
                .and_then(|metadata| {
                    serde_json::to_value(metadata.inspect()).map_err(Failure::projection)
                })
                .map(Outcome::CompositionMetadata),
            CompositionOperation::Compile(limits) => acquired
                .check_metadata(request.syntax)
                .map_err(|error| Failure::native(error.code(), &error))
                .and_then(|metadata| {
                    build::compile_templates(&metadata, limits)
                        .map_err(|error| Failure::native(error.code(), &error))
                })
                .and_then(|compiled| {
                    serde_json::to_value(compiled.evidence()).map_err(Failure::projection)
                })
                .map(Outcome::Templates),
        }
    }
}

/// Selected project-documentation phase.
pub enum SiteDocumentsOperation {
    /// Retain exact placement evidence only.
    Inspect,
    /// Parse the complete retained corpus.
    Parse(corpus::Limits),
}

/// Exact selected-site documentation command.
pub struct SiteDocumentsRequest {
    /// Detached UTF-8 workspace TOML or an explicitly selected native workspace.
    pub source: build::WorkspaceSource,
    /// Exact configured site identity.
    pub site: workspace::Key,
    /// Explicit permission for configured native source reads.
    pub allow_native_reads: bool,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Site source-acquisition ceilings.
    pub acquisition: source::PreparationLimits,
    /// Selected documentation phase.
    pub operation: SiteDocumentsOperation,
}

/// Execute selected-site acquisition and optional syntax inspection once.
pub struct SiteDocumentsCommand(pub SiteDocumentsRequest);

impl Command for SiteDocumentsCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::permission(
                "site_permission",
                "Explicit configured-source read permission required; no source acquisition started",
            ));
        }
        let acquired = build::acquire_selected_site(
            &request.source,
            &request.site,
            request.syntax,
            request.acquisition,
        )
        .map_err(|error| Failure::native(error.code(), &error))?;
        match request.operation {
            SiteDocumentsOperation::Inspect => serde_json::to_value(acquired.inspect())
                .map(Outcome::Site)
                .map_err(Failure::projection),
            SiteDocumentsOperation::Parse(limits) => build::parse_documents(&acquired, limits)
                .map_err(|error| Failure::native(error.code(), &error))
                .and_then(|parsed| {
                    let report = parsed
                        .inspect()
                        .map_err(|error| Failure::native(error.code(), &error))?;
                    serde_json::to_value(report).map_err(Failure::projection)
                })
                .map(Outcome::SiteDocuments),
        }
    }
}

/// Selected private publication projection.
pub enum PublicationOperation {
    /// Inspect the resolved structural model.
    Inspect,
    /// Produce the complete private review.
    Review(build::ReviewLimits),
    /// Project the complete private registry.
    Registry(build::RegistryLimits),
}

/// Exact private publication command.
pub struct PublicationRequest {
    /// Complete UTF-8 workspace TOML.
    pub source: build::WorkspaceSource,
    /// Exact configured site identity.
    pub site: workspace::Key,
    /// Explicit permission for configured native source reads.
    pub allow_native_reads: bool,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Publication source-acquisition ceilings.
    pub acquisition: source::PreparationLimits,
    /// Retained authored-corpus ceilings.
    pub corpus: corpus::Limits,
    /// Resolved content-graph ceilings.
    pub graph: content::Limits,
    /// Explicit source-bound typesetting candidates for read-only review only.
    /// No typesetter is executed, and decoded coordinates are not trusted facts.
    pub retained_math: Option<corpus::RetainedMath>,
    /// Selected private publication projection.
    pub operation: PublicationOperation,
}

/// Execute one source-bound publication projection.
pub struct PublicationCommand(pub PublicationRequest);

impl Command for PublicationCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::permission(
                "publication_permission",
                "Explicit configured-source read permission required; no source acquisition started",
            ));
        }
        if let Some(results) = &request.retained_math {
            let PublicationOperation::Review(limits) = request.operation else {
                return Err(Failure::permission(
                    "publication_review_math_input",
                    "Retained math is an explicit review input, not an inspection or registry option",
                ));
            };
            let mut bounded = render::LimitedWriter::new(std::io::sink(), limits.bytes);
            serde_json::to_writer(&mut bounded, results)
                .map_err(|error| Failure::native("publication_review_math_limit", &error))?;
        }
        let acquired = build::acquire_selected_publication(
            &request.source,
            &request.site,
            request.syntax,
            request.acquisition,
        )
        .map_err(|error| Failure::native(error.code(), &error))?;
        let parsed = build::parse_publication(&acquired, request.corpus)
            .map_err(|error| Failure::native(error.code(), &error))?;
        let parsed = if let Some(results) = request.retained_math {
            parsed
                .with_math(results)
                .map_err(|error| Failure::native(error.code(), &error))?
        } else {
            if matches!(request.operation, PublicationOperation::Review(_))
                && let Some((path, _)) = parsed.corpus().files().iter().find(|(_, file)| {
                    file.math_sources()
                        .any(|(_, _, facts)| facts.iter().any(|fact| fact.emitted))
                })
            {
                return Err(Failure::native(
                    "publication_review_math_required",
                    &format!(
                        "{path}: review requires complete source-bound retained math results; supply the results map from an explicitly authorized render. Review does not run a typesetter"
                    ),
                ));
            }
            parsed
        };
        let resolved = parsed
            .resolve(request.graph)
            .map_err(|error| Failure::native(error.code(), &error))?;
        match request.operation {
            PublicationOperation::Inspect => serde_json::to_value(resolved.inspect())
                .map(Outcome::Publication)
                .map_err(Failure::projection),
            PublicationOperation::Review(limits) => build::review(&resolved, limits)
                .map_err(|error| Failure::native(error.code(), &error))
                .and_then(|value| serde_json::to_value(value).map_err(Failure::projection))
                .map(Outcome::Review),
            PublicationOperation::Registry(limits) => resolved
                .registry(limits)
                .map_err(|error| Failure::native(error.code(), &error))
                .and_then(|value| serde_json::to_value(value).map_err(Failure::projection))
                .map(Outcome::Registry),
        }
    }
}
