//! Resolve optional request limits against one loaded workspace.

use crate::{corpus, document, notebook, workspace};
use serde::{Deserialize, Serialize};

/// Corpus request limits. Only the aggregate byte ceiling has a workspace default.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CorpusLimits {
    /// Maximum retained source files, including assets.
    pub files: usize,
    /// Explicit invocation ceiling; omission uses the workspace, then the native default.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "source_bytes"
    )]
    #[schemars(with = "usize")]
    pub source_bytes: Option<usize>,
    /// Maximum aggregate logical-path bytes.
    pub path_bytes: usize,
    /// Maximum retained Markdown nodes.
    pub markdown_nodes: usize,
    /// Maximum retained frontmatter nodes.
    pub metadata_nodes: usize,
    /// Maximum retained notebook values.
    pub notebook_nodes: usize,
    /// Independent per-document parser limits.
    pub document: document::Limits,
    /// Independent per-notebook parser limits.
    pub notebook: notebook::Limits,
}

impl Default for CorpusLimits {
    fn default() -> Self {
        let mut limits: Self = corpus::Limits::default().into();
        limits.source_bytes = None;
        limits
    }
}

impl From<corpus::Limits> for CorpusLimits {
    fn from(limits: corpus::Limits) -> Self {
        Self {
            files: limits.files,
            source_bytes: Some(limits.source_bytes),
            path_bytes: limits.path_bytes,
            markdown_nodes: limits.markdown_nodes,
            metadata_nodes: limits.metadata_nodes,
            notebook_nodes: limits.notebook_nodes,
            document: limits.document,
            notebook: limits.notebook,
        }
    }
}

impl CorpusLimits {
    /// Resolve the byte default without changing any independently supplied parser bound.
    pub fn resolve(self, workspace: &workspace::Workspace) -> corpus::Limits {
        corpus::Limits {
            files: self.files,
            source_bytes: self
                .source_bytes
                .or_else(|| workspace.build().and_then(|build| build.corpus_bytes))
                .unwrap_or_else(|| corpus::Limits::default().source_bytes),
            path_bytes: self.path_bytes,
            markdown_nodes: self.markdown_nodes,
            metadata_nodes: self.metadata_nodes,
            notebook_nodes: self.notebook_nodes,
            document: self.document,
            notebook: self.notebook,
        }
    }
}

/// Local request limits before the workspace's optional corpus ceiling is resolved.
pub type LocalRenderLimits = super::PublicationSetRenderLimits<CorpusLimits>;

pub(super) fn resolve(
    limits: LocalRenderLimits,
    workspace: &workspace::Workspace,
) -> super::PublicationSetRenderLimits {
    super::PublicationSetRenderLimits {
        publication: super::PublicationRenderLimits {
            corpus: limits.publication.corpus.resolve(workspace),
            graph: limits.publication.graph,
            output: limits.publication.output,
        },
        bytes: limits.bytes,
        entries: limits.entries,
    }
}

fn source_bytes<'de, D: serde::Deserializer<'de>>(decoder: D) -> Result<Option<usize>, D::Error> {
    usize::deserialize(decoder).map(Some)
}
