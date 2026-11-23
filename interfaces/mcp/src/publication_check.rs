//! Private publication projections share one native acquisition and resolution path.

use gazette_cli::sdk::{build, corpus, publication::content, source, store, toml, workspace::Key};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::path::PathBuf;

use gazette_cli::command::{
    Kernel,
    configuration::{PublicationCommand, PublicationOperation, PublicationRequest},
    coordinator::ReconcileRegistry,
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Inspect,
    Review,
    Registry,
    Reconcile,
}

impl Route {
    pub(super) const ALL: [Self; 4] =
        [Self::Inspect, Self::Review, Self::Registry, Self::Reconcile];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Inspect => "inspect_publication",
            Self::Review => "review_publication",
            Self::Registry => "project_registry",
            Self::Reconcile => "reconcile_registry",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema, output) = match self {
            Self::Inspect => (
                "Use inspect_publication to see the whole publication structure Gazette resolves from the configured sources. One call captures exact configured local sources and resolves the complete publication through the same SDK as CLI check --publication. Returns private configuration, source evidence and structural graph, including drafts and foreign references. No Theme/resource compilation, approval, staging, database or release effects. Use review_publication for the full review and check_workspace for a diagnostic pass.",
                schemars::schema_for!(Request<Inspect>),
                crate::tools::output_schema::<build::PublicationInspection<'static>>(),
            ),
            Self::Review => (
                "Use review_publication to get the full private review of a publication before anyone approves it. One call builds the same complete private review as CLI check --publication --review from captured inputs and one resolved model. Mathematical sources require explicit source-bound retained_math results from an authorized render. Preserves nested uses, source revisions, drafts, assets and uncompiled obligations. Native Markdown/notebook/raster projection runs; typesetters, Theme/resource programs and policy processes do not. A review digest is not approval, certification or publication permission. Approval stays with the publisher.",
                schemars::schema_for!(Request<Review>),
                crate::tools::output_schema::<build::PublicationReview>(),
            ),
            Self::Registry => (
                "Use project_registry to see the registry rows a publication would produce and their structural digest. One call projects the complete configured private registry through the same SDK as CLI check --publication --registry. The selected acquisition site does not filter workspace catalogs or content. Preserves empty content and exact provenance. This computes rows and their structural digest; it never opens, initializes, migrates or reconciles a database and grants no publication authority.",
                schemars::schema_for!(Request<Registry>),
                crate::tools::output_schema::<build::RegistryProjection<'static>>(),
            ),
            Self::Reconcile => (
                "Use reconcile_registry to bring the stored private registry in line with a publication model. One call replaces the complete private registry from one retained publication model through the same SDK as CLI reconcile-registry. Site selects acquisition, not a replacement subset. Requires independent source reads, replacement, generation advance and local coordination permissions. May remove obsolete derived rows and advance generation even when later work fails; preserves durable release history. Exact expected header, no migration, approval, automatic retry or release effect. Failures retain native generation/write/close evidence. Whether the registry is replaced, and any release that follows, stays with the publisher.",
                schemars::schema_for!(Request<Reconcile>),
                crate::tools::output_schema::<build::registry_output::Report<'static>>(),
            ),
        };
        Tool::new(self.name(), description, schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(!matches!(self, Self::Reconcile))
                    .destructive(matches!(self, Self::Reconcile))
                    .idempotent(!matches!(self, Self::Reconcile))
                    .open_world(true),
            )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        // Named native option structs must not accept Serde's positional-array form.
        for path in [
            "/source/limits",
            "/source/limits/source",
            "/source/limits/syntax",
            "/source/limits/syntax/document",
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
            "/corpus",
            "/corpus/document",
            "/corpus/document/metadata",
            "/corpus/document/markdown",
            "/corpus/notebook",
            "/graph",
            "/review",
            "/review/graph",
            "/registry",
            "/expected_header",
            "/options",
            "/options/projection",
            "/options/registry",
            "/options/writer",
            "/options/writer/session",
            "/options/writer/session/read",
        ] {
            if value.pointer(path).is_some_and(|v| !v.is_object()) {
                return Err(invalid());
            }
        }
        match self {
            Self::Inspect => serde_json::from_value::<Request<Inspect>>(value)
                .map(|request| request.into_input(|_| Projection::Inspect)),
            Self::Review => serde_json::from_value::<Request<Review>>(value)
                .map(|request| request.into_input(Projection::Review)),
            Self::Registry => serde_json::from_value::<Request<Registry>>(value).map(|request| {
                request.into_input(|options| Projection::Registry(options.registry))
            }),
            Self::Reconcile => serde_json::from_value::<Request<Reconcile>>(value)
                .map(|request| request.into_input(Projection::Reconcile)),
        }
        .map_err(|_| invalid())
    }
}

fn invalid() -> ErrorData {
    ErrorData::invalid_params(
        "Invalid publication check arguments; consult this tool's inputSchema; no source reads started",
        None,
    )
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Request<P> {
    /// Complete workspace TOML, or an explicit supplied/local workspace selection.
    /// A string is always text; native paths are selected only by the local variant.
    source: build::WorkspaceSource,
    /// Exact configured acquisition site; does not filter the complete registry.
    site: Key,
    /// Independently permit configured local source reads; absent means denied.
    #[serde(default)]
    allow_read: bool,
    /// Complete workspace syntax bounds, before native acquisition.
    #[serde(default)]
    syntax: toml::Limits,
    /// Whole selected-source capture bounds and per-source revision limits.
    #[serde(default = "default_acquisition")]
    acquisition: source::PreparationLimits,
    /// Complete corpus and nested document/notebook parser bounds.
    #[serde(default)]
    corpus: corpus::Limits,
    /// Complete semantic graph bounds before any projection.
    #[serde(default)]
    graph: content::Limits,
    #[serde(flatten)]
    projection: P,
}

#[derive(Deserialize, JsonSchema)]
struct Inspect {}

#[derive(Deserialize, JsonSchema)]
struct Review {
    /// Complete private review encoding, nested uses and graph projection bounds.
    #[serde(default)]
    review: build::ReviewLimits,
    /// Complete retained typesetting candidates keyed by captured logical source.
    /// Native binding validates current source/profile/occurrences before review;
    /// this input neither executes a typesetter nor attests who produced the data.
    #[serde(default, deserialize_with = "retained_math_input")]
    retained_math: Option<corpus::RetainedMath>,
}

fn retained_math_input<'de, D: serde::Deserializer<'de>>(
    input: D,
) -> Result<Option<corpus::RetainedMath>, D::Error> {
    Option::<Value>::deserialize(input)?
        .map(corpus::decode_retained_math)
        .transpose()
        .map_err(serde::de::Error::custom)
}

#[derive(Deserialize, JsonSchema)]
struct Registry {
    /// Complete registry encoding bound; never a database connection or writer option.
    #[serde(default)]
    registry: build::RegistryLimits,
}

#[derive(Deserialize, JsonSchema)]
struct Reconcile {
    /// Exact existing private store root, separate from every captured source.
    root: PathBuf,
    /// Independently retained coordinates; never inferred from the current database.
    expected_header: store::JournalHeader,
    /// Complete-set projection, replacement and writer permission/bounds.
    #[serde(default)]
    options: build::RegistryReconcileOptions,
}

fn default_acquisition() -> source::PreparationLimits {
    build::PublicationSetAcquisitionLimits::default().sources
}

enum Projection {
    Inspect,
    Review(Review),
    Registry(build::RegistryLimits),
    Reconcile(Reconcile),
}

pub(super) struct Input {
    request: Request<Projection>,
}

impl<P> Request<P> {
    fn into_input(self, project: impl FnOnce(P) -> Projection) -> Input {
        Input {
            request: Request {
                source: self.source,
                site: self.site,
                allow_read: self.allow_read,
                syntax: self.syntax,
                acquisition: self.acquisition,
                corpus: self.corpus,
                graph: self.graph,
                projection: project(self.projection),
            },
        }
    }
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let mut request = self.request;
        let retained_math = match &mut request.projection {
            Projection::Review(options) => options.retained_math.take(),
            _ => None,
        };
        let operation = match &request.projection {
            Projection::Inspect => Some(PublicationOperation::Inspect),
            Projection::Review(options) => Some(PublicationOperation::Review(options.review)),
            Projection::Registry(limits) => Some(PublicationOperation::Registry(*limits)),
            Projection::Reconcile(_) => None,
        };
        if let Some(operation) = operation {
            return match Kernel::new().execute(PublicationCommand(PublicationRequest {
                source: request.source,
                site: request.site,
                allow_native_reads: request.allow_read,
                syntax: request.syntax,
                acquisition: request.acquisition,
                corpus: request.corpus,
                graph: request.graph,
                retained_math,
                operation,
            })) {
                Ok(outcome) => crate::tools::encode(&outcome, false, maximum),
                Err(error) => crate::tools::failure(error.code(), error.message(), maximum),
            };
        }
        let Projection::Reconcile(selection) = request.projection else {
            unreachable!("non-effectful publication projections returned above")
        };
        if !selection.root.is_absolute() {
            return Err(invalid());
        }
        if let Err(error) = selection
            .options
            .registry
            .validate()
            .and_then(|_| selection.options.writer.validate())
        {
            return crate::tools::failure(
                error.code(),
                "Registry effect permission/bounds refused before acquisition",
                maximum,
            );
        }
        let reservation = match build::registry_output::Reservation::new(maximum) {
            Ok(reservation) => reservation,
            Err(code) => {
                return crate::tools::failure(
                    code,
                    "Complete response capacity refused before acquisition",
                    maximum,
                );
            }
        };
        let result = Kernel::new().execute(ReconcileRegistry {
            source: request.source,
            site: request.site,
            allow_native_reads: request.allow_read,
            syntax: request.syntax,
            acquisition: request.acquisition,
            corpus: request.corpus,
            graph: request.graph,
            root: selection.root,
            expected: selection.expected_header,
            options: selection.options,
        });
        let result = match result {
            Ok(receipt) => Ok(receipt),
            Err(error) => match error.into_effect() {
                Ok(error) => Err(error),
                Err(error) => {
                    return crate::tools::failure(error.code(), &error.to_string(), maximum);
                }
            },
        };
        let encoded = reservation.finish(&result).map_err(|error| {
            ErrorData::internal_error(
                "Registry delivery contract violated; original outcome retained; no bounded response or retry implied",
                serde_json::to_value(&error.report).ok(),
            )
        })?;
        let structured =
            serde_json::from_slice(&encoded.bytes).expect("shared SDK emitted complete JSON");
        let text = String::from_utf8(encoded.bytes).expect("shared SDK emitted UTF-8 JSON");
        let mut response = if encoded.is_error {
            CallToolResult::error(vec![ContentBlock::text(text)])
        } else {
            CallToolResult::success(vec![ContentBlock::text(text)])
        };
        response.structured_content = Some(structured);
        Ok(response)
    }
}

#[cfg(test)]
#[path = "../tests/internal/publication_check.rs"]
mod tests;
