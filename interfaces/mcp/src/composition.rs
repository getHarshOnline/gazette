//! Distinct retained composition phases over one native source acquisition owner.

use gazette_cli::sdk::{build, render, source, toml, workspace::Key};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use gazette_cli::command::{
    Kernel,
    configuration::{CompositionCommand, CompositionOperation, CompositionRequest},
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Inspect,
    Check,
    Compile,
}

impl Route {
    pub(super) const ALL: [Self; 3] = [Self::Inspect, Self::Check, Self::Compile];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Inspect => "inspect_composition",
            Self::Check => "check_composition",
            Self::Compile => "compile_templates",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema, output) = match self {
            Self::Inspect => (
                "Use inspect_composition to see which Theme and capability sources a site is configured to use, as captured. One call captures the exact site's configured local Theme and capability sources through the SDK used by CLI check --acquire-composition. Returns complete private configuration, pins and retained inventory evidence. Capture does not validate presentation manifests or compile templates, and does not acquire the complete authored publication. Requires allow_read; no network, native writes, subprocess, rendering, policy or release effects. Use check_composition to validate them.",
                schemars::schema_for!(Request<NoCompilation>),
                crate::tools::output_schema::<build::CompositionInspection<'static>>(),
            ),
            Self::Check => (
                "Use check_composition to learn whether a site's Theme and capability manifests are valid. One call captures and validates the selected composition through the SDK used by CLI check --acquire-composition --check-composition. All selected manifests and required files are checked, including optional capability declarations; failure returns no successful subset. Uses syntax limits for the workspace and independently for the complete manifest set, as the CLI does. Returns private MetadataInspection, not template syntax, executable compatibility, rendering or approval. Requires allow_read; no network, native writes, provider execution, subprocess or release effects. Use compile_templates to check template syntax. Approval stays with the publisher.",
                schemars::schema_for!(Request<NoCompilation>),
                crate::tools::output_schema::<build::MetadataInspection<'static>>(),
            ),
            Self::Compile => (
                "Use compile_templates to learn whether a Theme's templates compile. One call captures and checks composition metadata, then actually compiles every selected retained Theme template through the pinned SDK provider used by CLI check --acquire-composition --compile-templates. Returns exact TemplateCompilation evidence; no compiled session is retained. Template syntax, inheritance and macros are checked, but templates are not rendered and capability/resource programs are not compiled or executed. Trusted template compilation is bounded, not sandboxed or executable attestation. Requires allow_read; no network, native writes, subprocess, policy, signing or publication effects.",
                schemars::schema_for!(Request<Compilation>),
                crate::tools::output_schema::<build::TemplateCompilation>(),
            ),
        };
        Tool::new(self.name(), description, schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(true)
                    .destructive(false)
                    .idempotent(true)
                    .open_world(true),
            )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        for path in [
            "/source/limits",
            "/source/limits/source",
            "/source/limits/syntax",
            "/source/limits/syntax/document",
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/templates",
            "/templates/templates",
        ] {
            if value.pointer(path).is_some_and(|value| !value.is_object()) {
                return Err(invalid());
            }
        }
        match self {
            Self::Inspect => serde_json::from_value::<Request<NoCompilation>>(value)
                .map(|request| request.into_input(|_| Step::Inspect)),
            Self::Check => serde_json::from_value::<Request<NoCompilation>>(value)
                .map(|request| request.into_input(|_| Step::Check)),
            Self::Compile => serde_json::from_value::<Request<Compilation>>(value)
                .map(|request| request.into_input(|options| Step::Compile(options.templates))),
        }
        .map_err(|_| invalid())
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid composition arguments; consult this tool's inputSchema; no source acquisition started",
        None,
    )
}

fn default_acquisition() -> source::PreparationLimits {
    build::PublicationSetAcquisitionLimits::default().sources
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request<P> {
    /// Complete workspace TOML, or an explicit supplied/local workspace selection.
    /// Native includes and relative source roots require the local selection.
    source: build::WorkspaceSource,
    /// Exact configured consuming site; no implicit default site.
    site: Key,
    /// Permit configured local source reads. Missing means denied.
    #[serde(default)]
    allow_read: bool,
    /// Workspace syntax bounds; metadata phases apply these independently to all manifests.
    #[serde(default)]
    syntax: toml::Limits,
    /// Whole selected-source capture ceilings and per-source Git/content bounds.
    #[serde(default = "default_acquisition")]
    acquisition: source::PreparationLimits,
    #[serde(flatten)]
    phase: P,
}

#[derive(Deserialize, JsonSchema)]
struct NoCompilation {}

#[derive(Deserialize, JsonSchema)]
struct Compilation {
    /// Exact native template compilation limits, including the retained template subtree.
    /// Context/output limits remain part of the native compilation evidence; no render runs.
    #[serde(default)]
    templates: render::Limits,
}

enum Step {
    Inspect,
    Check,
    Compile(render::Limits),
}

pub(super) struct Input {
    request: Request<Step>,
}

impl<P> Request<P> {
    fn into_input(self, phase: impl FnOnce(P) -> Step) -> Input {
        Input {
            request: Request {
                source: self.source,
                site: self.site,
                allow_read: self.allow_read,
                syntax: self.syntax,
                acquisition: self.acquisition,
                phase: phase(self.phase),
            },
        }
    }
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let request = self.request;
        let operation = match request.phase {
            Step::Inspect => CompositionOperation::Inspect,
            Step::Check => CompositionOperation::Check,
            Step::Compile(limits) => CompositionOperation::Compile(limits),
        };
        match Kernel::new().execute(CompositionCommand(CompositionRequest {
            source: request.source,
            site: request.site,
            allow_native_reads: request.allow_read,
            syntax: request.syntax,
            acquisition: request.acquisition,
            operation,
        })) {
            Ok(outcome) => crate::tools::encode(&outcome, false, maximum),
            Err(error) => crate::tools::failure(error.code(), error.message(), maximum),
        }
    }
}

#[cfg(test)]
#[path = "../tests/internal/composition.rs"]
mod tests;
