//! Renderer-independent bounds and failures for trusted presentation programs.

use std::fmt;

use serde::Serialize;

use crate::source::{self, EntryPath};

mod output;
pub use output::LimitedWriter;

/// Known generated representation, independent of its configured filename.
/// Transport adapters project this meaning into their native content-type syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaType {
    /// Rendered page, alias or graph view.
    Html,
    /// Structured model, search index or JSON feed.
    Json,
    /// Linked-data publication projection.
    JsonLd,
    /// Browser application manifest.
    WebManifest,
    /// XML discovery document.
    Xml,
    /// RSS syndication document.
    Rss,
    /// Atom syndication document.
    Atom,
    /// UTF-8 plain-text discovery document.
    Text,
    /// UTF-8 Markdown publication representation, not unparsed plain text.
    Markdown,
    /// Compiled browser module.
    #[serde(rename = "javascript")]
    JavaScript,
    /// Compiled stylesheet.
    Css,
    /// Transformed PNG image.
    Png,
    /// Transformed JPEG image.
    Jpeg,
    /// Lossless WebP image.
    Webp,
    /// Validated inert SVG image.
    Svg,
    /// Validated WOFF2 webfont.
    Woff2,
}

/// Explicit compilation and emission ceilings, not a hostile-code sandbox.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Selected template tree bounds.
    pub templates: source::Limits,
    /// Maximum serialized presentation context size.
    #[schemars(range(max = usize::MAX))]
    pub context_bytes: usize,
    /// Maximum successful emitted artifact size.
    #[schemars(range(max = usize::MAX))]
    pub output_bytes: usize,
    /// Maximum admitted template execution nesting, including calls and nested
    /// AST containers/expressions. The captured Tera profile admits only 1..=64;
    /// this is independent of source path depth and is not a CPU/parser sandbox.
    #[schemars(range(min = 1, max = 64))]
    pub execution_depth: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            templates: source::Limits {
                entries: 512,
                depth: 32,
                file_bytes: 256 << 10,
                total_bytes: 4 << 20,
            },
            context_bytes: 4 << 20,
            output_bytes: 8 << 20,
            execution_depth: 64,
        }
    }
}

/// A failed compilation or render never yields a partial successful artifact.
#[derive(Debug)]
pub enum Error {
    /// A selected retained input cannot be consumed.
    Input {
        /// Logical template path.
        path: EntryPath,
        /// Violated input condition.
        reason: &'static str,
    },
    /// A declared resource ceiling was exceeded.
    Limit(&'static str),
    /// Presentation context must be an object.
    ContextShape,
    /// Native template diagnostics, without an inward renderer dependency.
    Template(Box<dyn std::error::Error + Send + Sync>),
    /// Native context serialization diagnostics.
    Context(Box<dyn std::error::Error + Send + Sync>),
}

impl Error {
    /// Stable diagnostic class; not a publication acceptance decision.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Input { .. } => "render_input",
            Self::Limit(_) => "render_limit",
            Self::ContextShape => "render_context_shape",
            Self::Template(_) => "render_template",
            Self::Context(_) => "render_context",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Input { path, reason } => write!(f, "template {path:?}: {reason}"),
            Self::Limit(resource) => write!(f, "render limit exceeded: {resource}"),
            Self::ContextShape => f.write_str("presentation context must be an object"),
            Self::Template(error) => write!(f, "template: {error}"),
            Self::Context(error) => write!(f, "context: {error}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Template(error) | Self::Context(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}
