//! Select transport before parsing; pinned-tree mode never falls back to stdin.

use gazette_sdk::markdown::input::{Failure, Loaded, Tree};
use gazette_sdk::source::{EntryPath, Limits, TreeIdentity};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(clap::Args)]
pub(super) struct Input {
    /// Capture this complete source/staging tree instead of reading stdin.
    #[arg(long, requires_all = ["tree_identity", "document"])]
    tree: Option<PathBuf>,
    /// Independent {format,sha256} JSON from prior check/staging evidence.
    #[arg(long, requires = "tree", value_parser = parse_identity)]
    tree_identity: Option<TreeIdentity>,
    /// Literal root-relative regular file within the pinned tree; no suffix inference.
    #[arg(long, requires = "tree")]
    document: Option<EntryPath>,
    /// Maximum captured tree entries, including directories and hidden files.
    #[arg(long, requires = "tree", default_value_t = Limits::default().entries)]
    max_tree_entries: usize,
    /// Maximum captured tree depth, separate from Markdown parser depth.
    #[arg(long, requires = "tree", default_value_t = Limits::default().depth)]
    max_tree_depth: usize,
    /// Maximum bytes in any captured tree file, including non-document assets.
    #[arg(long, requires = "tree", default_value_t = Limits::default().file_bytes)]
    max_tree_file_bytes: u64,
    /// Maximum combined bytes in the complete captured tree.
    #[arg(long, requires = "tree", default_value_t = Limits::default().total_bytes)]
    max_tree_bytes: u64,
}

impl Input {
    pub(super) fn load(self, source_bytes: usize) -> Result<Loaded, Failure> {
        match (self.tree, self.tree_identity, self.document) {
            (Some(root), Some(identity), Some(document)) => Tree {
                root,
                identity,
                document,
                limits: Limits {
                    entries: self.max_tree_entries,
                    depth: self.max_tree_depth,
                    file_bytes: self.max_tree_file_bytes,
                    total_bytes: self.max_tree_bytes,
                },
            }
            .load(true),
            (None, None, None) => {
                let mut bytes = Vec::new();
                io::stdin()
                    .lock()
                    .take((source_bytes as u64).saturating_add(1))
                    .read_to_end(&mut bytes)
                    .map_err(|error| Failure {
                        code: "markdown_input",
                        message: error.to_string(),
                    })?;
                Ok(Loaded::Supplied(bytes))
            }
            _ => Err(Failure {
                code: "markdown_input",
                message:
                    "tree input requires root, independent identity and document; no stdin fallback"
                        .into(),
            }),
        }
    }
}

fn parse_identity(value: &str) -> Result<TreeIdentity, String> {
    if value.len() > 1024 {
        return Err("tree identity exceeds 1024 bytes".into());
    }
    serde_json::from_str(value).map_err(|error| format!("invalid tree identity: {error}"))
}
