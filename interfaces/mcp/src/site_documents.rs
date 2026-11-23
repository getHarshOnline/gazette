//! Selected project-documentation phases over the existing SDK acquisition owner.

use gazette_cli::sdk::{build, corpus, source, toml, workspace::Key};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use gazette_cli::command::{
    Kernel,
    configuration::{SiteDocumentsCommand, SiteDocumentsOperation, SiteDocumentsRequest},
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Inspect,
    Documents,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Inspect, Self::Documents];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Inspect => "inspect_site",
            Self::Documents => "parse_site_documents",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema, output) = match self {
            Self::Inspect => (
                "Use inspect_site to see which project-documentation placements a site enables, with their pins. One call acquires the exact site's enabled project-documentation placements through the SDK used by CLI check --acquire-site. Returns the complete private workspace, logical placement targets, exact repository/gitlink pins and inventories. Disabled and other-site placements are not read. This is not complete authored-publication or Theme acquisition and does not parse document contents. Requires allow_read; no network, source writes, staging, database, program execution or publication effects. Use parse_site_documents to parse their contents.",
                schemars::schema_for!(Request<Inspect>),
                crate::tools::output_schema::<build::Inspection<'static>>(),
            ),
            Self::Documents => (
                "Use parse_site_documents to see how a site's project documentation parses. One call acquires the selected site's enabled project-documentation placements once, then parses every retained file through the SDK used by CLI check --acquire-site --parse-documents. Returns complete source-bound Markdown, explicitly framed TOML-document and notebook syntax plus opaque asset identities, preserving logical placement paths and native byte coordinates. Invalid supported syntax never falls back to assets; failures return no successful subset. Requires allow_read; no source rereads within a call, notebook kernels, Theme/resource execution, site rendering, writes, network or admission/certification. Results may contain private source text and metadata.",
                schemars::schema_for!(Request<Documents>),
                crate::tools::output_schema::<build::DocumentInspection<'static>>(),
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
        // Serde also accepts positional sequences for some native structs; the
        // advertised transport is named objects, including all nested limit owners.
        for path in [
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/corpus",
            "/corpus/document",
            "/corpus/document/metadata",
            "/corpus/document/markdown",
            "/corpus/notebook",
        ] {
            if value.pointer(path).is_some_and(|value| !value.is_object()) {
                return Err(invalid());
            }
        }
        match self {
            Self::Inspect => serde_json::from_value::<Request<Inspect>>(value)
                .map(|request| request.into_input(|_| None)),
            Self::Documents => serde_json::from_value::<Request<Documents>>(value)
                .map(|request| request.into_input(|phase| Some(phase.corpus))),
        }
        .map_err(|_| invalid())
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid site-documentation arguments; consult this tool's inputSchema; no source acquisition started",
        None,
    )
}

fn default_acquisition() -> source::PreparationLimits {
    build::PublicationSetAcquisitionLimits::default().sources
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request<P> {
    /// Detached workspace TOML or an explicit native workspace selection; never inferred.
    source: build::WorkspaceSource,
    /// Exact configured consuming site; no default selection.
    site: Key,
    /// Explicit permission for the selected configured local sources; omitted means denied.
    #[serde(default)]
    allow_read: bool,
    /// Native workspace source-byte, node and depth ceilings.
    #[serde(default)]
    syntax: toml::Limits,
    /// Aggregate placement capture and per-source Git/content ceilings.
    #[serde(default = "default_acquisition")]
    acquisition: source::PreparationLimits,
    #[serde(flatten)]
    phase: P,
}

#[derive(Deserialize, JsonSchema)]
struct Inspect {}

#[derive(Deserialize, JsonSchema)]
struct Documents {
    /// Complete corpus bounds, including independent document/Markdown/TOML/notebook ceilings.
    #[serde(default)]
    corpus: corpus::Limits,
}

pub(super) struct Input {
    request: Request<Option<corpus::Limits>>,
}

impl<P> Request<P> {
    fn into_input(self, phase: impl FnOnce(P) -> Option<corpus::Limits>) -> Input {
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
            Some(limits) => SiteDocumentsOperation::Parse(limits),
            None => SiteDocumentsOperation::Inspect,
        };
        match Kernel::new().execute(SiteDocumentsCommand(SiteDocumentsRequest {
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
#[path = "../tests/internal/site_documents.rs"]
mod tests;
