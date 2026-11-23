//! Explicit local reads; source policy and whole-set parsing remain SDK-owned.

use std::path::PathBuf;

use gazette_cli::sdk::{configuration, source};
use rmcp::model::{CallToolResult, ErrorData, JsonObject, Tool, ToolAnnotations};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::tools::{encode, failure};
use gazette_cli::command::{
    Kernel,
    inspection::{SourceCommand, SourceRequest, SourceSelection, TomlCommand, TomlRequest},
};

#[derive(Clone, Copy)]
pub(super) enum Route {
    Source,
    Toml,
}

impl Route {
    pub(super) const ALL: [Self; 2] = [Self::Source, Self::Toml];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Source => "inspect_source",
            Self::Toml => "inspect_toml",
        }
    }

    pub(super) fn descriptor(self) -> Tool {
        let (description, mut schema) = match self {
            Self::Source => (
                "Use inspect_source to see exactly what a local source tree contains before you rely on it. One call reads and hashes only the explicitly selected local tree. Select an unpinned observation, exact checked-out Git revision, or parent gitlink verification. Hidden entries count; links, special files, mismatches and exceeded ceilings fail without a partial inventory. No fetch, checkout, filters, writes, publication admission or release. Native read access must be permitted by the caller/host; tool annotations grant no permission. Results may disclose selected path names and hashes. If the selection fails, correct it and call again. Whether the sources are admitted to a publication stays with the publisher.",
                schemars::schema_for!(Inspect),
            ),
            Self::Toml => (
                "Use inspect_toml to check that every file in a configuration directory is valid TOML. One call captures an explicitly selected local configuration tree and inspects EVERY regular file as TOML through Gazette's SDK, including hidden files and regardless of suffix. Use a dedicated configuration directory, not a mixed repository. Any source or document failure rejects the complete set. No import discovery, semantic workspace admission, writes or release. Native read access must be permitted by the caller/host; results may disclose selected configuration values. It cannot say what a workspace means; use check_workspace or load_workspace for that.",
                schemars::schema_for!(Toml),
            ),
        };
        let output = match self {
            Self::Source => crate::tools::output_schema::<SourceOutput>(),
            Self::Toml => crate::tools::output_schema::<gazette_cli::sdk::toml::SetInspection>(),
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
        // Serde accepts positional arrays for ordinary structs. MCP discovery
        // promises named objects, including native nested limit contracts.
        objects(
            &value,
            &[
                "limits",
                "limits/source",
                "limits/syntax",
                "limits/syntax/document",
                "selection",
                "selection/limits",
                "selection/limits/content",
            ],
        )?;
        let input = match self {
            Self::Source => serde_json::from_value(value).map(Input::Source),
            Self::Toml => serde_json::from_value(value).map(Input::Toml),
        }
        .map_err(|_| {
            ErrorData::invalid_params("Invalid local-read arguments; consult inputSchema", None)
        })?;
        let (root, permitted) = match &input {
            Input::Source(input) => (&input.root, input.allow_native_reads),
            Input::Toml(input) => (&input.root, input.allow_native_reads),
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

fn objects(value: &Value, paths: &[&str]) -> Result<(), ErrorData> {
    for path in paths {
        if value
            .pointer(&format!("/{path}"))
            .is_some_and(|item| !item.is_object())
        {
            return Err(ErrorData::invalid_params(
                format!("{path} must be an object"),
                None,
            ));
        }
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Inspect {
    /// Explicit native-read opt-in. Must be true; does not override host permissions.
    allow_native_reads: bool,
    /// Explicit absolute local root. No current-directory or workspace inference.
    root: PathBuf,
    /// Observation and exact revision evidence are distinct; there is no fallback.
    selection: Selection,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Selection {
    /// Local tree observation, not an atomic snapshot or Git verification.
    Observation {
        /// Literal confined relative directory; omitted selects the root.
        #[serde(default)]
        #[schemars(with = "Option<String>")]
        subdirectory: Option<source::Subdirectory>,
        /// Native source ceilings; omitted fields use SDK defaults.
        #[serde(default)]
        limits: source::Limits,
    },
    /// Verify selected bytes against the exact checked-out commit.
    Revision {
        /// Literal confined relative directory; empty and dot components are rejected.
        #[schemars(with = "String")]
        subdirectory: source::Subdirectory,
        /// Exact 40-character lowercase SHA-1 commit, never a moving ref.
        #[schemars(with = "String", regex(pattern = "^[0-9a-f]{40}$"))]
        commit: source::CommitId,
        #[serde(default)]
        limits: source::RevisionLimits,
    },
    /// Verify the child located by the exact parent's gitlink; never a sibling clone.
    Submodule {
        /// Exact 40-character lowercase parent SHA-1 commit.
        #[schemars(with = "String", regex(pattern = "^[0-9a-f]{40}$"))]
        parent_commit: source::CommitId,
        /// Literal parent-relative gitlink path.
        #[schemars(with = "String")]
        submodule_path: source::Subdirectory,
        /// Literal directory beneath the bound child repository.
        #[schemars(with = "String")]
        subdirectory: source::Subdirectory,
        #[serde(default)]
        limits: source::RevisionLimits,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Toml {
    /// Explicit native-read opt-in. Must be true; does not override host permissions.
    allow_native_reads: bool,
    /// Explicit absolute local configuration root; every selected file is parsed.
    root: PathBuf,
    /// Literal confined relative directory; omitted selects the root.
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    subdirectory: Option<source::Subdirectory>,
    /// Independent source capture and complete-set/per-document syntax ceilings.
    #[serde(default)]
    limits: configuration::Limits,
}

pub(super) enum Input {
    Source(Inspect),
    Toml(Toml),
}

#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
#[allow(dead_code)]
enum SourceOutput {
    Observation(source::Inventory),
    Revision(source::VerifiedSource),
    Submodule(source::VerifiedSubmodule),
}

impl Input {
    pub(super) fn execute(self, maximum: usize) -> Result<CallToolResult, ErrorData> {
        match self {
            Self::Source(input) => {
                let selection = match input.selection {
                    Selection::Observation {
                        subdirectory,
                        limits,
                    } => SourceSelection::Observation {
                        subdirectory,
                        limits,
                    },
                    Selection::Revision {
                        subdirectory,
                        commit,
                        limits,
                    } => SourceSelection::Revision {
                        subdirectory,
                        commit,
                        limits,
                    },
                    Selection::Submodule {
                        parent_commit,
                        submodule_path,
                        subdirectory,
                        limits,
                    } => SourceSelection::Submodule {
                        parent_commit,
                        submodule_path,
                        subdirectory,
                        limits,
                    },
                };
                project(
                    Kernel::new().execute(SourceCommand(SourceRequest {
                        allow_native_reads: input.allow_native_reads,
                        root: input.root,
                        selection,
                    })),
                    maximum,
                )
            }
            Self::Toml(input) => project(
                Kernel::new().execute(TomlCommand(TomlRequest {
                    allow_native_reads: input.allow_native_reads,
                    root: input.root,
                    subdirectory: input.subdirectory,
                    limits: input.limits,
                })),
                maximum,
            ),
        }
    }
}

fn project(
    value: Result<
        gazette_cli::command::inspection::Outcome,
        gazette_cli::command::inspection::Failure,
    >,
    maximum: usize,
) -> Result<CallToolResult, ErrorData> {
    match value {
        Ok(value) => encode(&value, false, maximum),
        Err(error) => failure(error.code(), error.message(), maximum),
    }
}

#[cfg(test)]
#[path = "../tests/internal/source.rs"]
mod tests;
