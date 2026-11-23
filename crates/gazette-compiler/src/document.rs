//! Explicit TOML framing composed with the same Markdown-body compiler.

use std::fmt;

use gazette_core::source::{Sha256Digest, Span};
use serde::Serialize;

use crate::{
    markdown::{self, ParsedMarkdown},
    toml::{self, ParsedToml},
};

/// Complete document and independent metadata/body ceilings.
#[derive(Clone, Copy, Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum complete UTF-8 document bytes, including delimiters.
    #[schemars(range(max = usize::MAX))]
    pub source_bytes: usize,
    /// TOML metadata byte/node/depth ceilings.
    pub metadata: toml::Limits,
    /// Markdown body and projection ceilings.
    pub markdown: markdown::Limits,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            source_bytes: 1024 * 1024,
            metadata: toml::Limits::default(),
            markdown: markdown::Limits::default(),
        }
    }
}

/// Framing, syntax or projection failure without successful partial output.
#[derive(Debug)]
pub enum Error {
    /// Complete document byte ceiling was exceeded.
    SourceLimit,
    /// Required exact opening/closing delimiter line is missing.
    Framing(&'static str),
    /// Metadata failure; its local diagnostic coordinates begin at `offset`.
    Metadata {
        /// Original TOML syntax/schema/limit failure.
        error: toml::Error,
        /// Metadata's start byte in the complete document.
        offset: usize,
    },
    /// Markdown failure; its local diagnostic coordinates begin at `offset`.
    Markdown {
        /// Original Markdown failure.
        error: markdown::Error,
        /// Body's start byte in the complete document.
        offset: usize,
    },
}

impl Error {
    /// Stable failure class, preserving the failing parser's diagnostic class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::SourceLimit => "document_limit",
            Self::Framing(_) => "document_framing",
            Self::Metadata { error, .. } => error.code(),
            Self::Markdown { error, .. } => error.code(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceLimit => f.write_str("complete document byte ceiling exceeded"),
            Self::Framing(message) => f.write_str(message),
            Self::Metadata { error, offset } => write!(
                f,
                "{error}; TOML-local ranges start at document byte {offset}"
            ),
            Self::Markdown { error, offset } => write!(
                f,
                "{error}; Markdown-local ranges start at document byte {offset}"
            ),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata { error, .. } => Some(error),
            Self::Markdown { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Complete document syntax evidence with distinct full/metadata/body identities.
#[derive(Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct Inspection {
    /// Exact whole-source bytes, including delimiters and line endings.
    pub source_sha256: Sha256Digest,
    /// Metadata byte range, excluding delimiter lines.
    pub metadata_span: Span,
    /// Body byte range in the complete input. Markdown ranges are relative to this.
    pub body_span: Span,
    /// Opaque TOML structural evidence, not claim or lifecycle admission.
    pub metadata: toml::Inspection,
    /// The same Markdown inspection as the body-only API, with body-local ranges.
    pub markdown: markdown::Inspection,
}

/// A complete explicitly selected TOML-frontmatter document, not an admitted page.
///
/// Metadata has no influence on rendering until a caller validates and consumes
/// its typed contract. In particular, no title, author, identity or lifecycle
/// default is invented by this syntax layer.
pub struct ParsedDocument<'s> {
    source: &'s str,
    source_sha256: Sha256Digest,
    metadata_span: Span,
    body_span: Span,
    metadata: ParsedToml<'s>,
    body: ParsedMarkdown<'s>,
}

/// Source-local TOML frontmatter framing before strict metadata/body parsing.
pub(crate) enum Frame {
    Complete { metadata: Span, body: Span },
    Incomplete { metadata: Span },
}

/// Locate exact frontmatter coordinates without interpreting its TOML values.
///
/// An incomplete closing fence is retained for editor assistance but remains an
/// error to [`ParsedDocument`]. Opening/fence spelling and source ceilings are
/// identical for both consumers.
pub(crate) fn frame(source: &str, limits: Limits) -> Result<Frame, Error> {
    if source.len() > limits.source_bytes {
        return Err(Error::SourceLimit);
    }
    let mut lines = source.split_inclusive('\n');
    let opening = lines
        .next()
        .ok_or(Error::Framing("missing +++ opening delimiter"))?;
    if line_value(opening) != "+++" || !opening.ends_with('\n') {
        return Err(Error::Framing(
            "TOML front matter requires +++ on the first complete line",
        ));
    }
    let start = opening.len();
    let mut end = start;
    for line in lines {
        if line_value(line) == "+++" {
            return Ok(Frame::Complete {
                metadata: Span { start, end },
                body: Span {
                    start: end + line.len(),
                    end: source.len(),
                },
            });
        }
        end += line.len();
        if end - start > limits.metadata.source_bytes {
            return Err(Error::Metadata {
                error: toml::Error::Limit("source bytes"),
                offset: start,
            });
        }
    }
    Ok(Frame::Incomplete {
        metadata: Span {
            start,
            end: source.len(),
        },
    })
}

impl fmt::Debug for ParsedDocument<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedDocument")
            .field("source_sha256", &self.source_sha256)
            .field("metadata_span", &self.metadata_span)
            .field("body_span", &self.body_span)
            .finish()
    }
}

impl<'s> ParsedDocument<'s> {
    /// Parse exact `+++` delimiter lines, validate TOML, then parse only the body.
    ///
    /// The opener must be the first line; LF and CRLF are supported. A closing
    /// line may end at EOF. Delimiters permit no spaces/indentation. A standalone
    /// `+++` line is reserved by this framing even inside a multiline value;
    /// malformed TOML fails rather than searching for a later convenient fence.
    /// Inline delimiter-like text is ordinary TOML data. No bytes are trimmed.
    ///
    /// # Errors
    ///
    /// Rejects missing/malformed framing, invalid or over-limit metadata and body.
    /// No YAML detection, frontmatter inference or metadata-to-body fallback occurs.
    pub fn parse(
        source: &'s str,
        limits: Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<Self, Error> {
        Self::parse_with_profile(
            source,
            gazette_core::markdown::Profile::default(),
            limits,
            markdown,
        )
    }

    /// Parse explicit TOML framing and its body under one selected Markdown profile.
    /// Metadata is never interpreted as a syntax selection or execution permission.
    ///
    /// # Errors
    /// Preserves framing, metadata and source-local body failures without fallback.
    pub fn parse_with_profile(
        source: &'s str,
        profile: gazette_core::markdown::Profile,
        limits: Limits,
        markdown: &dyn gazette_ports::markdown::Markdown,
    ) -> Result<Self, Error> {
        let Frame::Complete {
            metadata: metadata_span,
            body: body_span,
        } = frame(source, limits)?
        else {
            return Err(Error::Framing(
                "missing +++ closing delimiter; no document produced",
            ));
        };
        let metadata = ParsedToml::parse(
            metadata_span.slice(source).ok_or(Error::Framing(
                "invalid frontmatter coordinates; no document produced",
            ))?,
            limits.metadata,
        )
        .map_err(|error| Error::Metadata {
            error,
            offset: metadata_span.start,
        })?;
        let body = ParsedMarkdown::parse_with_profile(
            &source[body_span.start..],
            profile,
            limits.markdown,
            markdown,
        )
        .map_err(|error| Error::Markdown {
            error,
            offset: body_span.start,
        })?;
        Ok(Self {
            source,
            source_sha256: Sha256Digest::of(source.as_bytes()),
            metadata_span,
            body_span,
            metadata,
            body,
        })
    }

    /// Retained validated TOML for the caller's explicit schema, not inferred meaning.
    pub fn metadata(&self) -> &ParsedToml<'s> {
        &self.metadata
    }

    /// Exact complete source, retained without reopening a file or reserialization.
    pub fn source(&self) -> &'s str {
        self.source
    }

    /// Exact whole-document identity, independent of inspection/output budgets.
    pub fn source_sha256(&self) -> Sha256Digest {
        self.source_sha256
    }

    /// Metadata coordinates in the complete document, excluding delimiter lines.
    pub fn metadata_span(&self) -> Span {
        self.metadata_span
    }

    /// Body coordinates in the complete document; body AST ranges remain body-local.
    pub fn body_span(&self) -> Span {
        self.body_span
    }

    /// Retained body syntax; its coordinates start at the document's body span.
    pub fn body(&self) -> &ParsedMarkdown<'s> {
        &self.body
    }

    /// Attach the exact body's retained math result without reparsing or changing metadata.
    ///
    /// # Errors
    /// Rejects stale source/profile, changed occurrences or exceeded binding ceilings.
    pub fn with_math(
        mut self,
        identity: gazette_core::markdown::Identity,
        bindings: Vec<gazette_core::markdown::MathBinding>,
    ) -> Result<Self, Error> {
        self.body = self
            .body
            .with_math(identity, bindings)
            .map_err(|error| Error::Markdown {
                error,
                offset: self.body_span.start,
            })?;
        Ok(self)
    }

    /// Inspect metadata and body with explicit source-coordinate boundaries.
    ///
    /// # Errors
    ///
    /// Propagates body projection exhaustion without a partial document report.
    pub fn inspect(&self) -> Result<Inspection, Error> {
        Ok(Inspection {
            source_sha256: self.source_sha256,
            metadata_span: self.metadata_span,
            body_span: self.body_span,
            metadata: self.metadata.inspect(),
            markdown: self.body.inspect().map_err(|error| Error::Markdown {
                error,
                offset: self.body_span.start,
            })?,
        })
    }

    /// Render only the body through the shared Markdown fragment operation.
    ///
    /// # Errors
    ///
    /// Preserves the body's HTML policy and limits; metadata never grants trust.
    pub fn render_html(&self) -> Result<String, Error> {
        self.body.render_html().map_err(|error| Error::Markdown {
            error,
            offset: self.body_span.start,
        })
    }
}

fn line_value(line: &str) -> &str {
    match line.strip_suffix('\n') {
        Some(line) => line.strip_suffix('\r').unwrap_or(line),
        None => line,
    }
}
