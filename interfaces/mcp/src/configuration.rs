//! Configuration discovery and supplied-source execution share native query types.

use std::path::PathBuf;

use gazette_cli::sdk::{
    build, configuration as sdk_configuration, schema, source, toml, workspace,
};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{encode, output_schema};
use gazette_cli::command::{
    Kernel,
    configuration::{
        LoadWorkspaceCommand, LoadWorkspaceRequest, WorkspaceCommand, WorkspaceOperation,
        WorkspaceRequest,
    },
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Check,
    Load,
    Schema,
    Field,
    Composition,
}

impl Route {
    pub(super) const ALL: [Self; 5] = [
        Self::Check,
        Self::Load,
        Self::Schema,
        Self::Composition,
        Self::Field,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Check => "check_workspace",
            Self::Load => "load_workspace",
            Self::Schema => "contract_schema",
            Self::Field => "workspace_field_origin",
            Self::Composition => "workspace_composition_origin",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema) = match self {
            Self::Check => (
                "Use check_workspace first, to learn whether a publication workspace and its pages are sound before you build, preview or release. One call checks a publication workspace and its pages. Choose a workspace file or folder, or use the client folder, current folder, then saved default. Reads configured sources without rendering or publishing. Supplying source instead checks detached configuration only. If it returns a diagnostic, report its file, line and identity and fix the source rather than retrying unchanged. Whether the checked site is published stays with its publisher.",
                schemars::schema_for!(CheckRequest),
            ),
            Self::Load => (
                "Use load_workspace to read what one workspace declares when you already know its root. One call loads that exact modular workspace from an explicit absolute native root and root-relative workspace document. Reads only that document, its declared non-recursive includes, its selected machine binding, and necessary ancestor directories under caller limits and explicit permission. No cwd or ancestor discovery, build, write, registry or publication effect occurs. If you do not know the root, ask the user for it; this tool never searches for one.",
                schemars::schema_for!(NativeWorkspace),
            ),
            Self::Schema => (
                "Use contract_schema when you need the exact fields Gazette accepts in a Theme manifest, template context, workspace, authored content or legal source. One call returns a Draft 2020-12 schema generated from that native contract. Performs no native reads or effects; legal-source structure does not establish a composed notice or legal approval. A schema describes structure and cannot check a particular file; use check_workspace for that. Whether a legal source is approved stays with the publisher.",
                schemars::schema_for!(SchemaRequest),
            ),
            Self::Field => (
                "Use workspace_field_origin to find where a field was declared. One call finds an explicit declaration's original UTF-8 byte range using the native FieldQuery. Defaults and inferred fields receive no fabricated range. No repository is opened.",
                schemars::schema_for!(Query<workspace::FieldQuery>),
            ),
            Self::Composition => (
                "Use workspace_composition_origin to learn which declaration wins a site's composition choice, and why. One call explains the winning workspace/site composition selection using the native CompositionOriginQuery and retained TOML. No repository is opened.",
                schemars::schema_for!(Query<workspace::CompositionOriginQuery>),
            ),
        };
        let output = match self {
            Self::Check => output_schema::<Outcome<CheckResult>>(),
            Self::Load => output_schema::<Outcome<sdk_configuration::LoadedWorkspace>>(),
            Self::Schema => output_schema::<std::collections::BTreeMap<String, Value>>(),
            Self::Field => output_schema::<Outcome<workspace::FieldOrigin>>(),
            Self::Composition => output_schema::<Outcome<workspace::CompositionFieldOrigin>>(),
        };
        schema
            .ensure_object()
            .insert("type".into(), Value::String("object".into()));
        Tool::new(self.name(), description, schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(true)
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false),
            )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        if arguments
            .get("limits")
            .is_some_and(|value| !value.is_object())
        {
            return Err(ErrorData::invalid_params("limits must be an object", None));
        }
        if !matches!(self, Self::Check | Self::Load | Self::Schema)
            && arguments
                .get("query")
                .is_none_or(|value| !value.is_object())
        {
            return Err(ErrorData::invalid_params(
                "query must be an SDK query object",
                None,
            ));
        }
        let value = Value::Object(arguments);
        match self {
            Self::Check => serde_json::from_value::<CheckRequest>(value).map(|input| match input {
                CheckRequest::Supplied(input) => Input::Supplied {
                    source: input.source,
                    limits: input.limits,
                    query: Operation::Check,
                },
                CheckRequest::Local { workspace } => Input::Check {
                    workspace,
                    client: None,
                },
            }),
            Self::Load => serde_json::from_value::<NativeWorkspace>(value).map(Input::Native),
            Self::Schema => serde_json::from_value::<SchemaRequest>(value)
                .map(|input| Input::Schema(input.contract)),
            Self::Field => {
                serde_json::from_value::<Query<workspace::FieldQuery>>(value).map(|input| {
                    Input::Supplied {
                        source: input.source,
                        limits: input.limits,
                        query: Operation::Field(input.query),
                    }
                })
            }
            Self::Composition => {
                serde_json::from_value::<Query<workspace::CompositionOriginQuery>>(value).map(
                    |input| Input::Supplied {
                        source: input.source,
                        limits: input.limits,
                        query: Operation::Composition(input.query),
                    },
                )
            }
        }
        .map_err(|error| {
            ErrorData::invalid_params(format!("Invalid tool arguments: {error}"), None)
        })
    }
}

// Native values and failures preserve their existing untagged wire shapes.
#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum Outcome<T> {
    Value(T),
    Failure(workspace::Failure<'static>),
}

#[derive(JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum CheckResult {
    Configuration(Box<workspace::Compilation>),
    Workspace(build::WorkspaceCheck),
}

#[derive(Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
enum CheckRequest {
    Supplied(Source),
    Local {
        /// Workspace file or folder; omit to use the shared lookup order.
        workspace: Option<PathBuf>,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Source {
    /// Complete workspace TOML. UTF-8 bytes are bounded by `limits.source_bytes`.
    source: String,
    /// Native TOML parser ceilings; omitted fields retain the SDK defaults.
    #[serde(default)]
    limits: toml::Limits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SchemaRequest {
    /// Exact native contract whose current schema is required.
    contract: schema::Contract,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeWorkspace {
    /// Explicit permission for the exact bounded native workspace reads.
    allow_native_reads: bool,
    /// Explicit absolute native root; never inferred from cwd or ancestors.
    root: PathBuf,
    /// Exact root-relative workspace document whose strict includes select the remaining files.
    workspace: source::EntryPath,
    /// Native capture and complete document-set ceilings; omitted fields retain SDK defaults.
    #[serde(default)]
    limits: sdk_configuration::WorkspaceLoadLimits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Query<Q> {
    /// Complete workspace TOML. Paths are declarations, not acquisition requests.
    source: String,
    /// Exact compiler-owned query; discovery does not duplicate its variant catalog.
    query: Q,
    /// Native TOML parser ceilings; omitted fields retain the SDK defaults.
    #[serde(default)]
    limits: toml::Limits,
}

pub(super) enum Input {
    Check {
        workspace: Option<PathBuf>,
        client: Option<PathBuf>,
    },
    Supplied {
        source: String,
        limits: toml::Limits,
        query: Operation,
    },
    Native(NativeWorkspace),
    Schema(schema::Contract),
}

pub(super) enum Operation {
    Check,
    Field(workspace::FieldQuery),
    Composition(workspace::CompositionOriginQuery),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let result = match self {
            Self::Check { workspace, client } => {
                let location = std::env::current_dir().and_then(|start| {
                    sdk_configuration::locate_user_workspace(
                        workspace.as_deref(),
                        client.as_deref(),
                        &start,
                    )
                });
                let location = match location {
                    Ok(Some(location)) => location,
                    Ok(None) => {
                        return crate::tools::failure(
                            "workspace_not_found",
                            "No gazette.toml found. Open a publication folder or pass workspace.",
                            maximum,
                        );
                    }
                    Err(error) => {
                        return crate::tools::failure(
                            "workspace_location",
                            &error.to_string(),
                            maximum,
                        );
                    }
                };
                let report = build::check_workspace(
                    location,
                    Default::default(),
                    build::PublicationSetAcquisitionLimits::default().sources,
                    Default::default(),
                );
                return encode(&report, !report.is_valid(), maximum);
            }
            Self::Supplied {
                source,
                limits,
                query,
            } => {
                let operation = match query {
                    Operation::Check => WorkspaceOperation::Compile,
                    Operation::Field(query) => WorkspaceOperation::Field(query),
                    Operation::Composition(query) => WorkspaceOperation::CompositionOrigin(query),
                };
                Kernel::new().execute(WorkspaceCommand(WorkspaceRequest {
                    source,
                    limits,
                    operation,
                }))
            }
            Self::Native(input) => {
                Kernel::new().execute(LoadWorkspaceCommand(LoadWorkspaceRequest {
                    root: input.root,
                    workspace: input.workspace,
                    allow_native_reads: input.allow_native_reads,
                    limits: input.limits,
                }))
            }
            Self::Schema(contract) => return encode(&schema::contract(contract), false, maximum),
        };
        match result {
            Ok(outcome) => encode(&outcome, false, maximum),
            Err(error) => match error.structured() {
                Some(failure) => encode(failure, true, maximum),
                None => crate::tools::failure(error.code(), error.message(), maximum),
            },
        }
    }
}
