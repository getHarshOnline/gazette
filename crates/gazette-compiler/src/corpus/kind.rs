//! The parser and editor discovery use the same filename rules.

use gazette_core::source::EntryPath;

/// Parser choice for a logical source path; its bytes may still be invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// Markdown, with optional TOML frontmatter.
    Markdown,
    /// A `.landing.yaml` or `.landing.yml` document.
    Landing,
    /// A Jupyter notebook.
    Notebook,
    /// Opaque companion data, which discovery need not read.
    Asset,
}

impl SourceKind {
    /// Choose the existing corpus parser without opening the file.
    pub fn for_path(path: &EntryPath) -> Self {
        let name = path.as_str().to_ascii_lowercase();
        if name.ends_with(".landing.yaml") || name.ends_with(".landing.yml") {
            return Self::Landing;
        }
        match name.rsplit_once('.').map(|(_, suffix)| suffix) {
            Some("md" | "markdown") => Self::Markdown,
            Some("ipynb") => Self::Notebook,
            _ => Self::Asset,
        }
    }
}
