//! Read-only native artifact verification through the existing SDK owners.

use std::path::PathBuf;

use gazette_cli::sdk::{release_commit, source};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{encode, failure, output_schema};
use gazette_cli::command::{
    Kernel,
    inspection::{GeneratedTreeCommand, GeneratedTreeRequest},
    release_artifact as command,
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    GeneratedTree,
    ReleaseCommit,
    ReleaseCustody,
}

impl Route {
    pub(super) const ALL: [Self; 3] = [
        Self::GeneratedTree,
        Self::ReleaseCommit,
        Self::ReleaseCustody,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::GeneratedTree => "verify_generated_tree",
            Self::ReleaseCommit => "verify_release_commit",
            Self::ReleaseCustody => "inspect_release_custody",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema, output) = match self {
            Self::GeneratedTree => (
                "Use verify_generated_tree to confirm that a local generated-documentation tree is the one a retained manifest describes. One call verifies an existing local generated-documentation tree against an independently retained manifest SHA-256 and exact placement binding. Returns the native manifest and complete inventory; no generation, repair, retention, source admission or publication. Requires an explicit absolute root and allow_native_reads=true. Results may disclose selected path names and source lineage; annotations do not grant host permission. Publishing the documentation stays with the publisher.",
                schemars::schema_for!(GeneratedTree),
                output_schema::<source::GeneratedTreeInspection<'static>>(),
            ),
            Self::ReleaseCommit => (
                "Use verify_release_commit to confirm that a local Git commit holds exactly the output and metadata you expect. One call verifies complete existing local Git commit objects against the exact commit, output tree and explicit metadata recipe. Select an independent bare repository; no worktree, branch, HEAD, network, writes or release authority. metadata_json preserves exact JSON for native duplicate-safe parsing and requires explicit parent, including null for a root commit. Requires an absolute root and allow_native_reads=true; annotations do not grant host permission. Whether the commit is released stays with the publisher.",
                schemars::schema_for!(Commit),
                output_schema::<release_commit::CommitReport>(),
            ),
            Self::ReleaseCustody => (
                "Use inspect_release_custody to check that a retained custody reference still resolves to complete, matching commit objects. One call inspects the exact immutable Gazette custody reference and independently verifies its complete commit objects in an explicitly selected local bare repository. No reference creation, synchronization, journal, worktree or network effects. Observation does not prove prior durability, preparation, approval or release. metadata_json uses the same exact native recipe parser as commit verification. Requires an absolute root and allow_native_reads=true; annotations do not grant host permission. Whether the commit is released stays with the publisher.",
                schemars::schema_for!(Custody),
                output_schema::<release_commit::CustodyReport>(),
            ),
        };
        metadata_schema(&mut schema);
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
}

/// Both read-only and effectful transports describe the same exact native recipe.
pub(super) fn metadata_schema(schema: &mut schemars::Schema) {
    if let Some(metadata) = schema
        .ensure_object()
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut("metadata_json"))
        .and_then(Value::as_object_mut)
    {
        metadata.insert(
            "contentMediaType".into(),
            Value::String("application/json".into()),
        );
        // Inline native subschemas so references cannot accidentally resolve
        // against the outer tool schema. This is discovery, not a validator.
        metadata.insert(
            "contentSchema".into(),
            serde_json::to_value(
                schemars::generate::SchemaSettings::draft2020_12()
                    .for_serialize()
                    .with(|settings| settings.inline_subschemas = true)
                    .into_generator()
                    .into_root_schema_for::<release_commit::CommitMetadata>(),
            )
            .expect("native recipe schema is JSON"),
        );
    }
}

impl Route {
    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        for path in [
            "/limits",
            "/limits/content",
            "/expected_binding",
            "/expected_tree",
        ] {
            if value.pointer(path).is_some_and(|item| !item.is_object()) {
                return Err(ErrorData::invalid_params(
                    "Artifact fields require named objects; consult inputSchema",
                    None,
                ));
            }
        }
        let input = match self {
            Self::GeneratedTree => serde_json::from_value(value).map(Input::GeneratedTree),
            Self::ReleaseCommit => serde_json::from_value(value).map(Input::ReleaseCommit),
            Self::ReleaseCustody => serde_json::from_value(value).map(Input::ReleaseCustody),
        }
        .map_err(|_| {
            ErrorData::invalid_params("Invalid artifact arguments; consult inputSchema", None)
        })?;
        let (root, permitted) = match &input {
            Input::GeneratedTree(input) => (&input.root, input.allow_native_reads),
            Input::ReleaseCommit(input) => (&input.root, input.allow_native_reads),
            Input::ReleaseCustody(input) => (&input.root, input.allow_native_reads),
        };
        if !permitted {
            return Err(ErrorData::invalid_params(
                "Explicit allow_native_reads=true is required; no source was opened",
                None,
            ));
        }
        if !root.is_absolute() {
            return Err(ErrorData::invalid_params(
                "root must be an explicit absolute local path",
                None,
            ));
        }
        Ok(input)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct GeneratedTree {
    /// Required native-read opt-in, independent of host permission.
    allow_native_reads: bool,
    /// Existing absolute generated-tree root; never created or repaired.
    root: PathBuf,
    /// Independently selected placement, never inferred from the manifest.
    expected_binding: source::GeneratedTreeBinding,
    /// Independently retained digest of the exact manifest bytes.
    manifest_sha256: source::Sha256Digest,
    /// Native complete-tree ceilings; omitted fields use shared defaults.
    #[serde(default)]
    limits: source::Limits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Commit {
    /// Required native-read opt-in, independent of host permission.
    allow_native_reads: bool,
    /// Explicit absolute independent bare repository; no current-directory inference.
    root: PathBuf,
    /// Exact commit object, never a moving reference.
    commit: source::CommitId,
    /// Independently expected complete output tree.
    expected_tree: source::TreeIdentity,
    /// Exact UTF-8 JSON recipe, including explicit parent; duplicate keys are rejected natively.
    metadata_json: String,
    /// Native recipe input ceiling in UTF-8 bytes, separate from object metadata limits.
    #[serde(default = "metadata_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_metadata_bytes: usize,
    /// Complete content and decoded Git metadata ceilings; native defaults apply.
    #[serde(default)]
    limits: source::RevisionLimits,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Custody {
    /// Required native-read opt-in, independent of host permission.
    allow_native_reads: bool,
    /// Explicit absolute independent bare repository; never created or repaired.
    root: PathBuf,
    /// Exact immutable custody coordinate, not an arbitrary Git reference.
    custody_id: source::Sha256Digest,
    /// Exact expected commit object.
    commit: source::CommitId,
    /// Independently expected complete output tree.
    expected_tree: source::TreeIdentity,
    /// Exact UTF-8 JSON recipe with explicit parent; parsed by the native recipe owner.
    metadata_json: String,
    /// Native recipe UTF-8 byte ceiling, independent of object metadata limits.
    #[serde(default = "metadata_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_metadata_bytes: usize,
    /// Complete content and decoded Git metadata ceilings; native defaults apply.
    #[serde(default)]
    limits: source::RevisionLimits,
}

pub(super) const fn metadata_bytes() -> usize {
    16 * 1024
}

pub(super) enum Input {
    GeneratedTree(GeneratedTree),
    ReleaseCommit(Commit),
    ReleaseCustody(Custody),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::GeneratedTree(input) => {
                match Kernel::new().execute(GeneratedTreeCommand(GeneratedTreeRequest {
                    allow_native_reads: input.allow_native_reads,
                    root: input.root,
                    expected_binding: input.expected_binding,
                    manifest_sha256: input.manifest_sha256,
                    limits: input.limits,
                })) {
                    Ok(value) => encode(&value, false, maximum),
                    Err(error) => failure(error.code(), error.message(), maximum),
                }
            }
            Self::ReleaseCommit(input) => {
                match Kernel::new().execute(command::VerifyCommit {
                    allow_native_reads: input.allow_native_reads,
                    repository: input.root,
                    commit: input.commit,
                    expected_tree: input.expected_tree,
                    metadata_json: input.metadata_json.into_bytes(),
                    max_metadata_bytes: input.max_metadata_bytes,
                    limits: input.limits,
                }) {
                    Ok(value) => encode(&value, false, maximum),
                    Err(error) => failure(error.code(), error.message_text(), maximum),
                }
            }
            Self::ReleaseCustody(input) => {
                match Kernel::new().execute(command::InspectCustody {
                    allow_native_reads: input.allow_native_reads,
                    repository: input.root,
                    custody_id: input.custody_id,
                    commit: input.commit,
                    expected_tree: input.expected_tree,
                    metadata_json: input.metadata_json.into_bytes(),
                    max_metadata_bytes: input.max_metadata_bytes,
                    limits: input.limits,
                }) {
                    Ok(value) => encode(&value, false, maximum),
                    Err(error) => failure(error.code(), error.message_text(), maximum),
                }
            }
        }
    }
}
