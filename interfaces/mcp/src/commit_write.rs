//! Local object preparation and custody use the existing native owners exactly once.

use std::path::PathBuf;

use gazette_cli::sdk::{release_commit, source};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::tools::{encoded, failure, output_schema};
use gazette_cli::command::{Kernel, release_artifact as command};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Prepare,
    Retain,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Prepare, Self::Retain];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Prepare => "prepare_release_commit",
            Self::Retain => "retain_release_commit",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema, output) = match self {
            Self::Prepare => (
                "Use prepare_release_commit to turn retained output into Git commit objects for a release, without touching any ref. One call prepares exact retained output as detached objects in an existing independent bare Git repository. Requires independent allow_native_reads=true and allow_write_objects=true; explicit absolute root, output_root and protected_roots; exact expected_tree and metadata_json. No refs, custody, worktree, journal, network, signing or release. Native failure includes possible object effects; response loss is not rollback or permission to retry. If it fails, inspect the repository before trying again. Retaining, releasing or deploying the commit stays with the publisher.",
                schemars::schema_for!(Request<Prepare>),
                output_schema::<release_commit::output::CommitReport<'static>>(),
            ),
            Self::Retain => (
                "Use retain_release_commit to keep a verified commit as local custody before a release uses it. One call verifies exact existing commit objects and retains their immutable local custody reference with native storage synchronization. Requires independent allow_native_reads=true and allow_retain=true; absolute root and protected_roots; exact commit, custody_id, expected_tree and metadata_json. No preparation, deployment branch, journal, signing, network or release authority. Failure preserves possible custody effects. Inspect exact native state after response loss; never infer rollback or retry. Releasing or deploying the commit stays with the publisher.",
                schemars::schema_for!(Request<Retain>),
                output_schema::<release_commit::output::CustodyReport<'static>>(),
            ),
        };
        crate::artifact::metadata_schema(&mut schema);
        Tool::new(self.name(), description, schema.ensure_object().clone())
            .with_raw_output_schema(output)
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(true)
                    .idempotent(false)
                    .open_world(true),
            )
    }

    pub(super) fn decode(self, arguments: JsonObject) -> Result<Input, ErrorData> {
        let value = Value::Object(arguments);
        for path in ["/limits", "/limits/content", "/expected_tree"] {
            if value.pointer(path).is_some_and(|item| !item.is_object()) {
                return Err(invalid(
                    "Native limits and identities require named objects",
                ));
            }
        }
        let input = match self {
            Self::Prepare => serde_json::from_value(value).map(Input::Prepare),
            Self::Retain => serde_json::from_value(value).map(Input::Retain),
        }
        .map_err(|_| invalid("Invalid commit arguments; consult inputSchema"))?;
        match &input {
            Input::Prepare(request) => {
                request.check()?;
                if !request.mode.allow_write_objects || !request.mode.output_root.is_absolute() {
                    return Err(invalid(
                        "Explicit allow_write_objects=true and absolute output_root are required",
                    ));
                }
            }
            Input::Retain(request) => {
                request.check()?;
                if !request.mode.allow_retain {
                    return Err(invalid("Explicit allow_retain=true is required"));
                }
            }
        }
        Ok(input)
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Request<Mode> {
    /// Required native-read permission; object/custody writes require their own flag.
    allow_native_reads: bool,
    /// Absolute existing independent bare repository; no discovery or initialization.
    root: PathBuf,
    /// Independently retained complete output inventory identity.
    expected_tree: source::TreeIdentity,
    /// Exact UTF-8 JSON recipe, with explicit parent including null; parsed natively.
    metadata_json: String,
    /// Native recipe byte bound, independent of decoded Git metadata limits.
    #[serde(default = "crate::artifact::metadata_bytes")]
    #[schemars(range(max = usize::MAX))]
    max_metadata_bytes: usize,
    /// Complete explicitly selected protected roots; each must be absolute.
    protected_roots: Vec<PathBuf>,
    /// Native complete content and Git metadata bounds; shared defaults apply.
    #[serde(default)]
    limits: source::RevisionLimits,
    #[serde(flatten)]
    mode: Mode,
}

impl<Mode> Request<Mode> {
    fn check(&self) -> Result<(), ErrorData> {
        if !self.allow_native_reads {
            return Err(invalid(
                "Explicit allow_native_reads=true is required; no native operation attempted",
            ));
        }
        if !self.root.is_absolute() || self.protected_roots.iter().any(|root| !root.is_absolute()) {
            return Err(invalid(
                "root and every protected_roots entry must be absolute",
            ));
        }
        Ok(())
    }
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Prepare {
    /// Independent permission for local Git object writes only.
    allow_write_objects: bool,
    /// Existing absolute retained output, verified against `expected_tree` before writes.
    output_root: PathBuf,
}

#[derive(Deserialize, JsonSchema)]
pub(super) struct Retain {
    /// Independent permission for exact custody creation and synchronization.
    allow_retain: bool,
    /// Exact existing commit; no moving reference accepted.
    commit: source::CommitId,
    /// Exact immutable custody coordinate; never an arbitrary ref name.
    custody_id: source::Sha256Digest,
    /// Aggregate native storage synchronization ceiling.
    #[serde(default = "storage_bytes")]
    #[schemars(range(max = u64::MAX))]
    storage_bytes: u64,
}

fn storage_bytes() -> u64 {
    release_commit::CustodyOptions::default().storage_bytes
}

pub(super) enum Input {
    Prepare(Request<Prepare>),
    Retain(Request<Retain>),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        let outcome = match self {
            Self::Prepare(request) => Kernel::new().execute(command::PrepareCommit {
                allow_native_reads: request.allow_native_reads,
                repository: request.root,
                output_root: request.mode.output_root,
                expected_tree: request.expected_tree,
                metadata_json: request.metadata_json.into_bytes(),
                max_metadata_bytes: request.max_metadata_bytes,
                protected_roots: request.protected_roots,
                options: release_commit::CommitOptions {
                    allow_write_objects: request.mode.allow_write_objects,
                    limits: request.limits,
                },
                response_bytes: maximum,
            }),
            Self::Retain(request) => Kernel::new().execute(command::RetainCommit {
                allow_native_reads: request.allow_native_reads,
                repository: request.root,
                custody_id: request.mode.custody_id,
                commit: request.mode.commit,
                expected_tree: request.expected_tree,
                metadata_json: request.metadata_json.into_bytes(),
                max_metadata_bytes: request.max_metadata_bytes,
                protected_roots: request.protected_roots,
                options: release_commit::CustodyOptions {
                    allow_retain: request.mode.allow_retain,
                    limits: request.limits,
                    storage_bytes: request.mode.storage_bytes,
                },
                response_bytes: maximum,
            }),
        };
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) if error.reconciliation_required() => {
                return Err(ErrorData::internal_error(
                    error.message_text().to_owned(),
                    None,
                ));
            }
            Err(error) => return failure(error.code(), error.message_text(), maximum),
        };
        let failed = outcome.is_failure();
        encoded(outcome.into_bytes(), failed).map_err(|_| ErrorData::internal_error("Native result projection failed; preserve original request and inspect native effects; do not infer rollback or retry", None))
    }
}

fn invalid(message: &str) -> ErrorData {
    ErrorData::invalid_params(message.to_owned(), None)
}
