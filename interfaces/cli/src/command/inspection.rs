//! Source and syntax inspection commands shared by CLI and MCP.

use std::{fmt, path::PathBuf};

use gazette_sdk::{configuration, document, markdown, source, toml};
use serde::Serialize;
use serde_json::Value;

use super::{Command, Kernel};

/// Failure retained before terminal or protocol projection.
#[derive(Debug)]
pub struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn native(code: &'static str, error: &impl fmt::Display) -> Self {
        Self {
            code,
            message: error.to_string(),
        }
    }

    fn with_message(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
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
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Failure {}

/// One exact inspection result, retaining its native evidence shape.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Outcome {
    /// Native filesystem inventory.
    Observation(source::Inventory),
    /// Exact Git revision verification.
    Revision(source::VerifiedSource),
    /// Exact parent-bound submodule verification.
    Submodule(source::VerifiedSubmodule),
    /// Complete TOML-tree inspection.
    Toml(toml::SetInspection),
    /// Bounded Markdown or document inspection projection.
    Markdown(Value),
    /// Existing generated-tree verification projection.
    GeneratedTree(Value),
}

/// Explicit source selection; observation and Git verification never fall back.
pub enum SourceSelection {
    /// Observe current native bytes without asserting Git identity.
    Observation {
        /// Optional confined subtree.
        subdirectory: Option<source::Subdirectory>,
        /// Filesystem inventory ceilings.
        limits: source::Limits,
    },
    /// Verify one exact Git revision and subtree.
    Revision {
        /// Confined subtree within the repository.
        subdirectory: source::Subdirectory,
        /// Expected exact commit.
        commit: source::CommitId,
        /// Git revision verification ceilings.
        limits: source::RevisionLimits,
    },
    /// Verify a submodule through its exact parent gitlink.
    Submodule {
        /// Exact parent commit containing the gitlink.
        parent_commit: source::CommitId,
        /// Gitlink path in the parent repository.
        submodule_path: source::Subdirectory,
        /// Confined subtree in the submodule repository.
        subdirectory: source::Subdirectory,
        /// Git revision verification ceilings.
        limits: source::RevisionLimits,
    },
}

/// Exact native source inspection request.
pub struct SourceRequest {
    /// Explicit permission for native reads.
    pub allow_native_reads: bool,
    /// Exact absolute selected root.
    pub root: PathBuf,
    /// Required observation or Git-bound selection.
    pub selection: SourceSelection,
}

/// Execute one source observation or exact Git-bound verification.
pub struct SourceCommand(pub SourceRequest);

impl Command for SourceCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::with_message(
                "source_permission",
                "Explicit native-read permission is required; no source was opened",
            ));
        }
        match request.selection {
            SourceSelection::Observation {
                subdirectory,
                limits,
            } => source::inspect(&request.root, subdirectory.as_ref(), limits)
                .map(Outcome::Observation)
                .map_err(|error| Failure::native(error.code(), &error)),
            SourceSelection::Revision {
                subdirectory,
                commit,
                limits,
            } => source::verify(&request.root, &subdirectory, &commit, limits)
                .map(Outcome::Revision)
                .map_err(|error| Failure::native(error.code(), &error)),
            SourceSelection::Submodule {
                parent_commit,
                submodule_path,
                subdirectory,
                limits,
            } => source::verify_submodule(
                &request.root,
                &parent_commit,
                &submodule_path,
                &subdirectory,
                limits,
            )
            .map(Outcome::Submodule)
            .map_err(|error| Failure::native(error.code(), &error)),
        }
    }
}

/// Complete selected TOML tree request.
pub struct TomlRequest {
    /// Explicit permission for native reads.
    pub allow_native_reads: bool,
    /// Exact absolute selected root.
    pub root: PathBuf,
    /// Optional confined subtree.
    pub subdirectory: Option<source::Subdirectory>,
    /// Complete-set configuration ceilings.
    pub limits: configuration::Limits,
}

/// Execute one complete-set TOML inspection.
pub struct TomlCommand(pub TomlRequest);

impl Command for TomlCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::with_message(
                "configuration_permission",
                "Explicit native-read permission is required; no source was opened",
            ));
        }
        configuration::inspect_toml(&request.root, request.subdirectory.as_ref(), request.limits)
            .map(Outcome::Toml)
            .map_err(|error| Failure::native(error.code(), &error))
    }
}

/// Explicit Markdown framing mode.
#[derive(Clone, Copy)]
pub enum FrontMatter {
    /// Gazette TOML front matter.
    Toml,
}

/// All native syntax limits used by both CLI and MCP.
#[derive(Clone, Copy)]
pub struct MarkdownLimits {
    /// Markdown parser and render ceilings.
    pub markdown: markdown::Limits,
    /// TOML metadata parser ceilings.
    pub metadata: toml::Limits,
}

/// Supplied bytes or one document from an independently pinned complete tree.
pub enum MarkdownInput {
    /// Exact bytes supplied by the transport.
    Supplied(Vec<u8>),
    /// Already selected by a CLI transport that owns stdin/tree argument exclusivity.
    Loaded(markdown::input::Loaded),
    /// One document selected from an independently pinned tree.
    Tree {
        /// Exact tree and document binding.
        tree: markdown::input::Tree,
        /// Explicit permission for the bound native read.
        allow_native_reads: bool,
    },
}

/// Exact Markdown/document inspection request.
pub struct MarkdownRequest {
    /// Exact retained input selection.
    pub input: MarkdownInput,
    /// Optional Gazette front-matter framing.
    pub front_matter: Option<FrontMatter>,
    /// Whether to include bounded HTML rendering.
    pub html: bool,
    /// Complete syntax and rendering ceilings.
    pub limits: MarkdownLimits,
}

/// Execute one retained-input syntax inspection.
pub struct MarkdownCommand(pub MarkdownRequest);

#[derive(Serialize)]
#[serde(untagged)]
enum MarkdownProjection {
    Markdown(markdown::Report),
    Document(document::Report),
}

impl Command for MarkdownCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        let loaded = match request.input {
            MarkdownInput::Supplied(bytes) => markdown::input::Loaded::Supplied(bytes),
            MarkdownInput::Loaded(loaded) => loaded,
            MarkdownInput::Tree {
                tree,
                allow_native_reads,
            } => tree
                .load(allow_native_reads)
                .map_err(|error| Failure::with_message(error.code, error.message))?,
        };
        let text = loaded
            .text()
            .map_err(|error| Failure::with_message(error.code, error.message))?;
        let input = loaded.evidence();
        let projection = if request.front_matter.is_some() {
            document::inspect(
                text,
                document::Limits {
                    source_bytes: request.limits.markdown.source_bytes,
                    markdown: request.limits.markdown,
                    metadata: request.limits.metadata,
                },
                request.html,
            )
            .map(MarkdownProjection::Document)
            .map_err(|error| Failure::native(error.code(), &error))?
        } else {
            markdown::inspect(text, request.limits.markdown, request.html)
                .map(MarkdownProjection::Markdown)
                .map_err(|error| Failure::native(error.code(), &error))?
        };
        serde_json::to_value(markdown::input::Report { input, projection })
            .map(Outcome::Markdown)
            .map_err(|error| Failure::native("command_projection", &error))
    }
}

/// Exact generated-tree verification request.
pub struct GeneratedTreeRequest {
    /// Explicit permission for native reads.
    pub allow_native_reads: bool,
    /// Exact absolute generated-tree root.
    pub root: PathBuf,
    /// Expected generator, source, and output binding.
    pub expected_binding: source::GeneratedTreeBinding,
    /// Expected exact manifest digest.
    pub manifest_sha256: source::Sha256Digest,
    /// Generated-tree inventory ceilings.
    pub limits: source::Limits,
}

/// Verify one existing generated tree without repair.
pub struct GeneratedTreeCommand(pub GeneratedTreeRequest);

impl Command for GeneratedTreeCommand {
    type Outcome = Outcome;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let request = self.0;
        if !request.allow_native_reads {
            return Err(Failure::with_message(
                "generated_tree_permission",
                "Explicit native-read permission is required; no source was opened",
            ));
        }
        let verified = source::verify_generated_tree(
            &request.root,
            &request.expected_binding,
            request.manifest_sha256,
            request.limits,
        )
        .map_err(|error| Failure::native(error.code(), &error))?;
        serde_json::to_value(verified.inspect())
            .map(Outcome::GeneratedTree)
            .map_err(|error| Failure::native("command_projection", &error))
    }
}
