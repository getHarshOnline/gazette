//! One discoverable catalog; operation modules project their native SDK owners.

use gazette_cli::{command::metadata, sdk::render::LimitedWriter};
use rmcp::model::{CallToolResult, ContentBlock, ErrorData, JsonObject, MetaObject, Tool};
use schemars::JsonSchema;
use serde::Serialize;
use std::sync::Arc;

use crate::{
    artifact, authoring, commit_write, composition, configuration, coordinator, delivery,
    documentation, documentation_check, documentation_materialization, github, inspect,
    journal_commit, materialization, observation, preparation, preparation_history,
    preparation_inspection, preview, probe_output, publication, publication_check, qualification,
    readback, recovery_session, release, release_session, rendered_site, site_documents, source,
    stage, stage_check, storage, store_lifecycle, store_session,
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Artifact(artifact::Route),
    CommitWrite(commit_write::Route),
    JournalCommit(journal_commit::Route),
    Configuration(configuration::Route),
    Composition(composition::Route),
    SiteDocuments(site_documents::Route),
    Storage(storage::Route),
    StoreLifecycle(store_lifecycle::Route),
    StoreSession(store_session::Route),
    Materialization,
    DocumentationMaterialization,
    Coordinator(coordinator::Route),
    Markdown,
    Authoring(authoring::Route),
    Source(source::Route),
    StageCheck,
    Stage,
    ProbeOutput,
    ReleaseVerification,
    ReleaseSession(release_session::Route),
    RecoverySession(recovery_session::Route),
    Readback,
    Github,
    Delivery,
    Observation(observation::Route),
    Preparation,
    PreparationHistory(preparation_history::Route),
    PreparationInspection(preparation_inspection::Route),
    Publication,
    Qualification,
    RenderedSite(rendered_site::Route),
    Preview(preview::Route),
    DocumentationCheck,
    Documentation,
    PublicationCheck(publication_check::Route),
}

impl Route {
    pub(super) fn all() -> Vec<Self> {
        let mut routes: Vec<_> = configuration::Route::ALL
            .into_iter()
            .map(Self::Configuration)
            .chain(artifact::Route::ALL.into_iter().map(Self::Artifact))
            .chain(commit_write::Route::ALL.into_iter().map(Self::CommitWrite))
            .chain(
                journal_commit::Route::ALL
                    .into_iter()
                    .map(Self::JournalCommit),
            )
            .chain(composition::Route::ALL.into_iter().map(Self::Composition))
            .chain(
                site_documents::Route::ALL
                    .into_iter()
                    .map(Self::SiteDocuments),
            )
            .chain(std::iter::once(Self::Markdown))
            .chain(storage::Route::ALL.into_iter().map(Self::Storage))
            .chain(
                store_lifecycle::Route::ALL
                    .into_iter()
                    .map(Self::StoreLifecycle),
            )
            .chain(coordinator::Route::ALL.into_iter().map(Self::Coordinator))
            .chain(
                store_session::Route::ALL
                    .into_iter()
                    .map(Self::StoreSession),
            )
            .chain(std::iter::once(Self::Materialization))
            .chain(std::iter::once(Self::DocumentationMaterialization))
            .chain(authoring::Route::ALL.into_iter().map(Self::Authoring))
            .chain(source::Route::ALL.into_iter().map(Self::Source))
            .chain(std::iter::once(Self::StageCheck))
            .chain(std::iter::once(Self::Stage))
            .chain(std::iter::once(Self::ProbeOutput))
            .chain(std::iter::once(Self::ReleaseVerification))
            .chain(
                release_session::Route::ALL
                    .into_iter()
                    .map(Self::ReleaseSession),
            )
            .chain(
                recovery_session::Route::ALL
                    .into_iter()
                    .map(Self::RecoverySession),
            )
            .chain(std::iter::once(Self::Readback))
            .chain(std::iter::once(Self::Github))
            .chain(std::iter::once(Self::Delivery))
            .chain(observation::Route::ALL.into_iter().map(Self::Observation))
            .chain(std::iter::once(Self::Preparation))
            .chain(std::iter::once(Self::Qualification))
            .chain(
                preparation_history::Route::ALL
                    .into_iter()
                    .map(Self::PreparationHistory),
            )
            .chain(
                preparation_inspection::Route::ALL
                    .into_iter()
                    .map(Self::PreparationInspection),
            )
            .chain(std::iter::once(Self::Publication))
            .chain(
                rendered_site::Route::ALL
                    .into_iter()
                    .map(Self::RenderedSite),
            )
            .chain(preview::Route::ALL.into_iter().map(Self::Preview))
            .chain(std::iter::once(Self::DocumentationCheck))
            .chain(std::iter::once(Self::Documentation))
            .chain(
                publication_check::Route::ALL
                    .into_iter()
                    .map(Self::PublicationCheck),
            )
            .collect();
        routes.sort_by_key(|route| {
            metadata::operation(Route::name(route))
                .expect("every routed tool has canonical operation metadata")
                .discovery_key()
        });
        routes
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Artifact(route) => route.name(),
            Self::CommitWrite(route) => route.name(),
            Self::JournalCommit(route) => route.name(),
            Self::Configuration(route) => route.name(),
            Self::Composition(route) => route.name(),
            Self::SiteDocuments(route) => route.name(),
            Self::Storage(route) => route.name(),
            Self::StoreLifecycle(route) => route.name(),
            Self::StoreSession(route) => route.name(),
            Self::Materialization => "materialize_sites",
            Self::DocumentationMaterialization => "materialize_docs",
            Self::Coordinator(route) => route.name(),
            Self::Markdown => "inspect_markdown",
            Self::Authoring(route) => route.name(),
            Self::Source(route) => route.name(),
            Self::StageCheck => "check_stage",
            Self::Stage => "stage",
            Self::ProbeOutput => "probe_output",
            Self::ReleaseVerification => "verify_release_authorization",
            Self::ReleaseSession(route) => route.name(),
            Self::RecoverySession(route) => route.name(),
            Self::Readback => "observe_release_targets",
            Self::Github => "observe_github_pages_build",
            Self::Delivery => "observe_delivery",
            Self::Observation(route) => route.name(),
            Self::Preparation => "prepare_sites",
            Self::PreparationHistory(route) => route.name(),
            Self::PreparationInspection(route) => route.name(),
            Self::Publication => "render_sites",
            Self::Qualification => "qualify_sites",
            Self::RenderedSite(route) => route.name(),
            Self::Preview(route) => route.name(),
            Self::DocumentationCheck => "check_documentation",
            Self::Documentation => "assemble_docs",
            Self::PublicationCheck(route) => route.name(),
        }
    }

    pub(super) fn named(name: &str) -> Option<Self> {
        Self::all().into_iter().find(|route| route.name() == name)
    }

    pub(super) fn descriptor(self) -> Tool {
        let mut tool = match self {
            Self::Artifact(route) => route.descriptor(),
            Self::CommitWrite(route) => route.descriptor(),
            Self::JournalCommit(route) => route.descriptor(),
            Self::Configuration(route) => route.descriptor(),
            Self::Composition(route) => route.descriptor(),
            Self::SiteDocuments(route) => route.descriptor(),
            Self::Storage(route) => route.descriptor(),
            Self::StoreLifecycle(route) => route.descriptor(),
            Self::StoreSession(route) => route.descriptor(),
            Self::Materialization => materialization::descriptor(),
            Self::DocumentationMaterialization => documentation_materialization::descriptor(),
            Self::Coordinator(route) => route.descriptor(),
            Self::Markdown => inspect::descriptor(),
            Self::Authoring(route) => route.descriptor(),
            Self::Source(route) => route.descriptor(),
            Self::StageCheck => stage_check::descriptor(),
            Self::Stage => stage::descriptor(),
            Self::ProbeOutput => probe_output::descriptor(),
            Self::ReleaseVerification => release::descriptor(),
            Self::ReleaseSession(route) => route.descriptor(),
            Self::RecoverySession(route) => route.descriptor(),
            Self::Readback => readback::descriptor(),
            Self::Github => github::descriptor(),
            Self::Delivery => delivery::descriptor(),
            Self::Observation(route) => route.descriptor(),
            Self::Preparation => preparation::descriptor(),
            Self::PreparationHistory(route) => route.descriptor(),
            Self::PreparationInspection(route) => route.descriptor(),
            Self::Publication => publication::descriptor(),
            Self::Qualification => qualification::descriptor(),
            Self::RenderedSite(route) => route.descriptor(),
            Self::Preview(route) => route.descriptor(),
            Self::DocumentationCheck => documentation_check::descriptor(),
            Self::Documentation => documentation::descriptor(),
            Self::PublicationCheck(route) => route.descriptor(),
        };
        let discovery = metadata::operation(self.name())
            .expect("every routed tool has canonical operation metadata");
        let capability = discovery.capability;
        tool.title.get_or_insert_with(|| {
            format!("{} · {}", capability.title(), human_title(self.name()))
        });
        let metadata = tool.meta.get_or_insert_with(MetaObject::new);
        let value = serde_json::Value::String(capability.key().to_owned());
        if let Some(existing) = metadata
            .0
            .insert("io.rtepl.gazette/capability".to_owned(), value.clone())
        {
            assert_eq!(
                existing, value,
                "Gazette capability metadata must have one owner"
            );
        }
        for (key, value) in [
            ("intent", discovery.intent.key()),
            ("effect", discovery.effect.key()),
            ("prominence", discovery.prominence.key()),
            ("closure", discovery.closure.key()),
        ] {
            assert!(
                metadata
                    .0
                    .insert(
                        format!("io.rtepl.gazette/{key}"),
                        serde_json::Value::String(value.to_owned()),
                    )
                    .is_none(),
                "Gazette discovery metadata must have one owner"
            );
        }
        assert!(
            metadata
                .0
                .insert(
                    "io.rtepl.gazette/potentialEffects".to_owned(),
                    serde_json::Value::Array(
                        discovery
                            .potential_effects
                            .iter()
                            .map(|effect| serde_json::Value::String(effect.key().to_owned()))
                            .collect(),
                    ),
                )
                .is_none(),
            "Gazette discovery metadata must have one owner"
        );
        if !discovery.relations.is_empty() {
            metadata.0.insert(
                "io.rtepl.gazette/relations".to_owned(),
                serde_json::Value::Array(
                    discovery
                        .relations
                        .iter()
                        .map(|relation| {
                            serde_json::json!({
                                "kind": relation.kind.key(),
                                "operation": relation.operation,
                            })
                        })
                        .collect(),
                ),
            );
        }
        if let Some(owner) = discovery.terminal_result_owner {
            metadata.0.insert(
                "io.rtepl.gazette/terminalResultOwner".to_owned(),
                serde_json::Value::String(owner.to_owned()),
            );
        }
        tool
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        match self {
            Self::Artifact(route) => route.decode(arguments).map(Input::Artifact),
            Self::CommitWrite(route) => route.decode(arguments).map(Input::CommitWrite),
            Self::JournalCommit(route) => route.decode(arguments).map(Input::JournalCommit),
            Self::Configuration(route) => route.decode(arguments).map(Input::Configuration),
            Self::Composition(route) => route.decode(arguments).map(Input::Composition),
            Self::SiteDocuments(route) => route.decode(arguments).map(Input::SiteDocuments),
            Self::Storage(route) => route.decode(arguments).map(Input::Storage),
            Self::StoreLifecycle(route) => route.decode(arguments).map(Input::StoreLifecycle),
            Self::StoreSession(route) => route.decode(arguments).map(Input::StoreSession),
            Self::Materialization => materialization::decode(arguments).map(Input::Materialization),
            Self::DocumentationMaterialization => documentation_materialization::decode(arguments)
                .map(Input::DocumentationMaterialization),
            Self::Coordinator(route) => route.decode(arguments).map(Input::Coordinator),
            Self::Markdown => inspect::decode(arguments).map(Input::Markdown),
            Self::Authoring(route) => route.decode(arguments).map(Input::Authoring),
            Self::Source(route) => route.decode(arguments).map(Input::Source),
            Self::StageCheck => stage_check::decode(arguments).map(Input::StageCheck),
            Self::Stage => stage::decode(arguments).map(Input::Stage),
            Self::ProbeOutput => probe_output::decode(arguments).map(Input::ProbeOutput),
            Self::ReleaseVerification => release::decode(arguments).map(Input::ReleaseVerification),
            Self::ReleaseSession(route) => route.decode(arguments).map(Input::ReleaseSession),
            Self::RecoverySession(route) => route.decode(arguments).map(Input::RecoverySession),
            Self::Readback => readback::decode(arguments).map(Input::Readback),
            Self::Github => github::decode(arguments).map(Input::Github),
            Self::Delivery => delivery::decode(arguments).map(Input::Delivery),
            Self::Observation(route) => route.decode(arguments).map(Input::Observation),
            Self::Preparation => preparation::decode(arguments).map(Input::Preparation),
            Self::PreparationHistory(route) => {
                route.decode(arguments).map(Input::PreparationHistory)
            }
            Self::PreparationInspection(route) => {
                route.decode(arguments).map(Input::PreparationInspection)
            }
            Self::Publication => publication::decode(arguments).map(Input::Publication),
            Self::Qualification => qualification::decode(arguments).map(Input::Qualification),
            Self::RenderedSite(route) => route.decode(arguments).map(Input::RenderedSite),
            Self::Preview(route) => route.decode(arguments).map(Input::Preview),
            Self::DocumentationCheck => {
                documentation_check::decode(arguments).map(Input::DocumentationCheck)
            }
            Self::Documentation => documentation::decode(arguments).map(Input::Documentation),
            Self::PublicationCheck(route) => route.decode(arguments).map(Input::PublicationCheck),
        }
    }
}

fn human_title(name: &str) -> String {
    let mut title = name.replace('_', " ");
    if let Some(first) = title.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    title
}

pub(super) enum Input {
    Artifact(artifact::Input),
    CommitWrite(commit_write::Input),
    JournalCommit(journal_commit::Input),
    Configuration(configuration::Input),
    Composition(composition::Input),
    SiteDocuments(site_documents::Input),
    Storage(storage::Input),
    StoreLifecycle(store_lifecycle::Input),
    StoreSession(store_session::Input),
    Materialization(materialization::Input),
    DocumentationMaterialization(documentation_materialization::Input),
    Coordinator(coordinator::Input),
    Markdown(inspect::Markdown),
    Authoring(authoring::Input),
    Source(source::Input),
    StageCheck(stage_check::Input),
    Stage(stage::Input),
    ProbeOutput(probe_output::Input),
    ReleaseVerification(release::Input),
    ReleaseSession(release_session::Input),
    RecoverySession(recovery_session::Input),
    Readback(readback::Input),
    Github(github::Input),
    Delivery(delivery::Input),
    Observation(observation::Input),
    Preparation(preparation::Input),
    PreparationHistory(preparation_history::Input),
    PreparationInspection(preparation_inspection::Input),
    Publication(publication::Input),
    Qualification(qualification::Input),
    RenderedSite(rendered_site::Input),
    Preview(preview::Input),
    DocumentationCheck(documentation_check::Input),
    Documentation(documentation::Input),
    PublicationCheck(publication_check::Input),
}

impl Input {
    pub(super) fn execute(
        self,
        authoring: &authoring::Sessions,
        release: &release_session::Sessions,
        recovery: &recovery_session::Sessions,
        store: &store_session::Sessions,
        preview: &preview::Sessions,
        maximum: usize,
    ) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Artifact(input) => input.execute(maximum),
            Self::CommitWrite(input) => input.execute(maximum),
            Self::JournalCommit(input) => input.execute(maximum),
            Self::Configuration(input) => input.execute(maximum),
            Self::Composition(input) => input.execute(maximum),
            Self::SiteDocuments(input) => input.execute(maximum),
            Self::Storage(input) => input.execute(maximum),
            Self::StoreLifecycle(input) => input.execute(maximum),
            Self::StoreSession(input) => store.execute(input, maximum),
            Self::Materialization(input) => store.materialize(input, maximum),
            Self::DocumentationMaterialization(input) => {
                store.materialize_documentation(input, maximum)
            }
            Self::Coordinator(input) => input.execute(maximum),
            Self::Markdown(input) => input.execute(maximum),
            Self::Authoring(input) => authoring.execute(input, maximum),
            Self::Source(input) => input.execute(maximum),
            Self::StageCheck(input) => input.execute(maximum),
            Self::Stage(input) => input.execute(maximum),
            Self::ProbeOutput(input) => input.execute(maximum),
            Self::ReleaseVerification(input) => input.execute(maximum),
            Self::ReleaseSession(input) => release.execute(input, maximum),
            Self::RecoverySession(input) => recovery.execute(input, maximum),
            Self::Readback(input) => input.execute(maximum),
            Self::Github(input) => input.execute(maximum),
            Self::Delivery(input) => input.execute(maximum),
            Self::Observation(input) => input.execute(maximum),
            Self::Preparation(input) => input.execute(maximum),
            Self::PreparationHistory(input) => input.execute(maximum),
            Self::PreparationInspection(input) => input.execute(maximum),
            Self::Publication(input) => input.execute(maximum),
            Self::Qualification(input) => input.execute(maximum),
            Self::RenderedSite(input) => input.execute(maximum),
            Self::Preview(input) => preview.execute(input, maximum),
            Self::DocumentationCheck(input) => input.execute(maximum),
            Self::Documentation(input) => input.execute(maximum),
            Self::PublicationCheck(input) => input.execute(maximum),
        }
    }
}

pub(super) fn failure(
    code: &str,
    message: &str,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    encode(&ToolFailure { code, message }, true, maximum)
}

/// Encode the same failure owner without silently substituting a context-free limit record.
pub(super) fn failure_complete(
    code: &str,
    message: &str,
    maximum: usize,
) -> Result<CallToolResult, EncodingError> {
    encode_complete(&ToolFailure { code, message }, true, maximum)
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ToolFailure<'a> {
    code: &'a str,
    message: &'a str,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OutputLimit {
    code: OutputLimitCode,
}

#[derive(Serialize, JsonSchema)]
enum OutputLimitCode {
    #[serde(rename = "mcp_output_limit")]
    Exceeded,
}

/// Describe emitted values, including shared failures, using serialization rules.
/// This is a wire schema, not a second validator or native acceptance decision.
pub(super) fn output_schema<T: JsonSchema>() -> Arc<JsonObject> {
    // This sum is schema-only: operations serialize their actual native values.
    #[derive(JsonSchema)]
    #[serde(untagged)]
    #[allow(dead_code)]
    enum Output<T> {
        Value(T),
        Failure(ToolFailure<'static>),
        Limit(OutputLimit),
    }
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<Output<T>>();
    schema
        .ensure_object()
        .insert("type".into(), "object".into());
    Arc::new(std::mem::take(schema.ensure_object()))
}

/// Extend an SDK-owned root schema with the existing pre-effect MCP failures.
pub(super) fn native_output_schema(mut native: JsonObject) -> Arc<JsonObject> {
    let mut shared = output_schema::<()>().as_ref().clone();
    if let Some(serde_json::Value::Object(definitions)) = native.remove("$defs") {
        let target = shared
            .entry("$defs")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .expect("schema definitions");
        for (name, definition) in definitions {
            if let Some(existing) = target.insert(name, definition.clone()) {
                assert_eq!(
                    existing, definition,
                    "native and transport schema names must not conflict"
                );
            }
        }
    }
    shared["anyOf"]
        .as_array_mut()
        .expect("shared result alternatives")[0] = serde_json::Value::Object(native);
    Arc::new(shared)
}

/// Bound serialization before allocating a JSON value or readable projection.
pub(super) fn encode(
    value: &impl Serialize,
    failed: bool,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match encode_complete(value, failed, maximum) {
        Ok(result) => Ok(result),
        Err(EncodingError::Limit) => {
            let mut result = CallToolResult::error(vec![ContentBlock::text(
                "Gazette result exceeds the MCP output ceiling; narrow the request or use the direct SDK.",
            )]);
            result.structured_content = Some(
                serde_json::to_value(OutputLimit {
                    code: OutputLimitCode::Exceeded,
                })
                .expect("fixed output-limit record"),
            );
            Ok(result)
        }
        Err(EncodingError::Invalid) => Err(ErrorData::internal_error(
            "SDK result could not be encoded",
            None,
        )),
    }
}

pub(super) enum EncodingError {
    Limit,
    Invalid,
}

/// No replacement advice: effectful operations must select their own prebounded failure evidence.
pub(super) fn encode_complete(
    value: &impl Serialize,
    failed: bool,
    maximum: usize,
) -> Result<CallToolResult, EncodingError> {
    let mut buffer = LimitedWriter::new(Vec::new(), maximum);
    if serde_json::to_writer(&mut buffer, value).is_err() {
        if buffer.exceeded() {
            return Err(EncodingError::Limit);
        }
        return Err(EncodingError::Invalid);
    }
    let bytes = buffer.into_inner();
    encoded(bytes, failed)
}

/// Project already bounded serialized bytes without rerunning the native operation.
pub(super) fn encoded(bytes: Vec<u8>, failed: bool) -> Result<CallToolResult, EncodingError> {
    let structured = serde_json::from_slice(&bytes).map_err(|_| EncodingError::Invalid)?;
    let text = String::from_utf8(bytes).map_err(|_| EncodingError::Invalid)?;
    let mut result = if failed {
        CallToolResult::error(vec![ContentBlock::text(text)])
    } else {
        CallToolResult::success(vec![ContentBlock::text(text)])
    };
    result.structured_content = Some(structured);
    Ok(result)
}
