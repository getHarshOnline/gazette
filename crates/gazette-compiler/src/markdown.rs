//! Gazette Markdown admission and resolution over a private syntax adapter.

use std::sync::OnceLock;
use std::{borrow::Cow, fmt};

use gazette_core::markdown::{self as syntax, Profile};
pub use gazette_core::markdown::{Limits, LinkForm, LinkKind};
use gazette_core::source::{Sha256Digest, Span};
use gazette_ports::markdown::{Document, Markdown};

mod container_contents;
mod faq;
pub use faq::{Faq, FaqQuestion, validate_faq_html};
mod figures;
pub use figures::validate_figure_html;
mod anchors;
mod completion;
mod inspect;
mod references;
mod render;
pub(crate) mod source_path;
mod summary;
mod symbolic;
pub use anchors::Heading;
pub use inspect::{Inspection, Link};
pub use references::HeadingReference;
pub(crate) use references::decode_reference;
pub(crate) use render::EmbeddedImage;
pub(crate) use render::Projection;
pub use render::{
    ComponentPresentation, ComponentUse, ImageUse, ImageVariants, LinkDestination, LinkUse,
    Opening, RenderedMarkdown, ResponsiveImage,
};
pub(crate) use render::{ImagePresentation, ResolvedLink};
pub use summary::Summary;
pub use symbolic::{EcosystemReference, IdentityReference};

/// The owner of source anchor admission, independent of the parser's syntax profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorPolicy {
    /// Authored publication headings, footnotes and continuation markers.
    #[default]
    Publication,
    /// Repository manual headings and named HTML admitted by documentation projection.
    Manual,
}

/// No partial projection is returned on a bound or unsupported HTML operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A source-bound parser or formatter failure; no partial document is accepted.
    Adapter(syntax::Error),
    /// A caller supplied an unsafe or oversized embedded-fragment namespace.
    Namespace,
    /// A named resource exhausted its explicit ceiling.
    Limit(&'static str),
    /// Raw HTML requires a separately admitted rendering capability.
    RawHtml(Span),
    /// The HTML projection rejects a dangerous or unsupported URL form.
    UnsafeUrl(Span),
    /// An explicit summary boundary is ambiguous or inside unsupported syntax.
    Summary {
        /// Original Markdown marker range.
        span: Span,
        /// Stable bounded reason, never the author's unbounded text.
        reason: &'static str,
    },
    /// Heading or footnote anchors cannot be emitted safely and unambiguously.
    Anchor {
        /// Original Markdown declaration range.
        span: Span,
        /// Stable bounded reason, never unbounded source text.
        reason: &'static str,
    },
    /// A caller could not resolve a syntactic link under its configured contract.
    LinkResolution {
        /// Original Markdown link range, not a generated destination offset.
        span: Span,
        /// Bounded diagnostic classification; never the caller's unbounded source text.
        reason: &'static str,
    },
    /// The parser/renderer violated an internal structural or UTF-8 invariant.
    InvalidOutput,
}

impl Error {
    /// A source construct, when the native diagnostic supplies one.
    pub fn location(&self) -> Option<gazette_core::source::Location> {
        use gazette_core::source::{Location, Precision};
        let span = match self {
            Self::RawHtml(span)
            | Self::UnsafeUrl(span)
            | Self::Summary { span, .. }
            | Self::Anchor { span, .. }
            | Self::LinkResolution { span, .. } => *span,
            Self::Adapter(syntax::Error::Unsupported { location, .. }) => return Some(*location),
            _ => return None,
        };
        Some(Location {
            span,
            precision: Precision::Construct,
        })
    }
    /// Stable diagnostic class, independent of display wording.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Adapter(_) => "markdown_adapter",
            Self::Namespace => "markdown_namespace",
            Self::Limit(_) => "markdown_limit",
            Self::RawHtml(_) => "markdown_raw_html_unavailable",
            Self::UnsafeUrl(_) => "markdown_unsafe_url",
            Self::Summary { .. } => "markdown_summary",
            Self::Anchor { .. } => "markdown_anchor",
            Self::LinkResolution { .. } => "markdown_link_resolution",
            Self::InvalidOutput => "markdown_invalid_output",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Adapter(error) => error.fmt(f),
            Self::Namespace => f.write_str("Markdown fragment namespace must contain at most 256 ASCII letters, digits, hyphens or underscores"),
            Self::Limit(resource) => write!(f, "Markdown {resource} ceiling exceeded"),
            Self::RawHtml(span) => write!(
                f,
                "raw HTML at bytes {}..{} requires an admitted renderer; no HTML returned",
                span.start, span.end
            ),
            Self::UnsafeUrl(span) => write!(
                f,
                "unsafe or unsupported URL at bytes {}..{}; no HTML returned",
                span.start, span.end
            ),
            Self::LinkResolution { span, reason } => write!(
                f,
                "Markdown link at bytes {}..{} could not resolve: {reason}; no HTML returned",
                span.start, span.end
            ),
            Self::Summary { span, reason } => write!(
                f,
                "Markdown summary at bytes {}..{} is invalid: {reason}; no projection returned",
                span.start, span.end
            ),
            Self::Anchor { span, reason } => write!(
                f,
                "Markdown anchor at bytes {}..{} is invalid: {reason}; no projection returned",
                span.start, span.end
            ),
            Self::InvalidOutput => {
                f.write_str("Markdown parser or renderer produced invalid internal output")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<syntax::Error> for Error {
    fn from(error: syntax::Error) -> Self {
        match error {
            syntax::Error::Limit(resource) => Self::Limit(resource),
            other => Self::Adapter(other),
        }
    }
}

pub(crate) fn validate_namespace(namespace: &str) -> Result<(), Error> {
    if namespace.len() > 256
        || !namespace
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(Error::Namespace);
    }
    Ok(())
}

/// Exact source plus private adapter-retained syntax facts, not a publication model.
/// The compiler alone allocates anchors, admits syntax and resolves references.
pub struct ParsedMarkdown<'s> {
    source: Cow<'s, str>,
    document: Box<dyn Document>,
    limits: Limits,
    anchors: OnceLock<Result<anchors::Anchors, Error>>,
    manual_anchors: OnceLock<Result<anchors::Anchors, Error>>,
    summary: OnceLock<Result<Option<summary::Cut>, Error>>,
    math: Vec<syntax::MathBinding>,
}

impl fmt::Debug for ParsedMarkdown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParsedMarkdown")
            .field("node_count", &self.node_count())
            .field("source_sha256", &self.source_sha256())
            .finish()
    }
}

impl<'s> ParsedMarkdown<'s> {
    /// Original input, including exact line endings; never reconstructed from syntax.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Exact original byte identity.
    pub fn source_sha256(&self) -> Sha256Digest {
        self.document.syntax().identity.source_sha256
    }

    /// Parser-inspected syntax nodes, including the document root.
    pub fn node_count(&self) -> usize {
        self.document.syntax().node_count
    }

    /// Retained native component candidates in source order.
    ///
    /// These are syntax facts only. Name, selected-Theme membership and arguments
    /// remain owned by the presentation component contract.
    pub fn directives(&self) -> &[syntax::Directive] {
        &self.document.syntax().directives
    }

    /// Check source-only rendering constraints before caller link resolution or Theme use.
    ///
    /// This reuses the retained parser facts consumed by the real renderer. It does not
    /// resolve URLs, select components, admit raw HTML or establish publication validity.
    ///
    /// # Errors
    /// Reports raw HTML other than the explicit summary marker and parser constructs whose
    /// normalized output cannot preserve correct footnote backlinks.
    pub fn check_source_rendering(&self) -> Result<(), Error> {
        self.renderable_summary_cut()?;
        self.check_adapter_rendering()
    }

    /// Check source admission for its declared owner without granting HTML output permission.
    ///
    /// # Errors
    /// Retains the owner's invalid HTML, anchor and adapter failures at source locations.
    pub fn check_source_rendering_with_policy(&self, policy: AnchorPolicy) -> Result<(), Error> {
        match policy {
            AnchorPolicy::Publication => self.check_source_rendering(),
            AnchorPolicy::Manual => {
                self.anchor_index_with_policy(policy)?;
                self.check_adapter_rendering()
            }
        }
    }

    pub(in crate::markdown) fn renderable_summary_cut(
        &self,
    ) -> Result<Option<&summary::Cut>, Error> {
        let cut = self.summary_cut()?;
        for html in &self.document.syntax().html {
            if !cut.is_some_and(|value| value.marker == *html) {
                return Err(Error::RawHtml(html.location.span));
            }
        }
        Ok(cut)
    }

    pub(in crate::markdown) fn check_adapter_rendering(&self) -> Result<(), Error> {
        self.document.check_rendering().map_err(Error::from)
    }

    /// Parse through the explicitly supplied Markdown adapter with Gazette's syntax profile.
    /// No path, ambient adapter selection or remote observation is consulted.
    ///
    /// # Errors
    /// Refuses exhausted bounds, unsupported parser syntax or inconsistent source identity.
    pub fn parse(source: &'s str, limits: Limits, adapter: &dyn Markdown) -> Result<Self, Error> {
        Self::parse_with_profile(source, Profile::Publication, limits, adapter)
    }

    /// Parse under an explicitly selected syntax profile; no generator is invoked.
    ///
    /// # Errors
    /// Refuses parser limits or adapter identity inconsistent with the selected profile.
    pub fn parse_with_profile(
        source: &'s str,
        profile: Profile,
        limits: Limits,
        adapter: &dyn Markdown,
    ) -> Result<Self, Error> {
        Self::parse_source(Cow::Borrowed(source), profile, limits, adapter)
    }

    /// Parse a compiler-owned projection while retaining its exact normalized bytes.
    /// The enclosing source format remains responsible for original-source locations.
    ///
    /// # Errors
    /// Uses the same bounded parser and identity checks as borrowed Markdown.
    pub(crate) fn parse_owned(
        source: String,
        profile: Profile,
        limits: Limits,
        adapter: &dyn Markdown,
    ) -> Result<Self, Error> {
        Self::parse_source(Cow::Owned(source), profile, limits, adapter)
    }

    fn parse_source(
        source: Cow<'s, str>,
        profile: Profile,
        limits: Limits,
        adapter: &dyn Markdown,
    ) -> Result<Self, Error> {
        let document = adapter
            .parse(&source, profile, limits)
            .map_err(Error::from)?;
        if document.syntax().identity
            != (syntax::Identity {
                source_sha256: Sha256Digest::of(source.as_bytes()),
                profile,
            })
        {
            return Err(Error::InvalidOutput);
        }
        Ok(Self {
            source,
            document,
            limits,
            anchors: OnceLock::new(),
            manual_anchors: OnceLock::new(),
            summary: OnceLock::new(),
            math: Vec::new(),
        })
    }

    /// Exact source and syntax selection to which generated fragments must bind.
    pub fn identity(&self) -> syntax::Identity {
        self.document.syntax().identity
    }

    /// Native source formulas, including non-emitted evidence, without generation.
    pub fn math(&self) -> &[syntax::Math] {
        &self.document.syntax().math
    }

    /// Retain a complete ordered typesetting result for this exact source/profile.
    /// Fragment structure is checked by the formatting adapter before any output.
    ///
    /// # Errors
    /// Rejects wrong source/profile, incomplete or changed occurrences and output ceilings.
    pub fn with_math(
        mut self,
        identity: syntax::Identity,
        bindings: Vec<syntax::MathBinding>,
    ) -> Result<Self, Error> {
        let facts = self.math().iter().filter(|formula| formula.emitted);
        if identity != self.identity()
            || facts.clone().count() != bindings.len()
            || facts
                .zip(&bindings)
                .any(|(fact, binding)| fact != &binding.occurrence)
        {
            return Err(Error::Adapter(syntax::Error::InvalidPlan));
        }
        let mut budget = Budget(self.limits.output_bytes, "math binding bytes");
        for binding in &bindings {
            budget.charge(binding.mathml.len())?;
            budget.charge(binding.occurrence.tex.len())?;
        }
        self.math = bindings;
        Ok(self)
    }
}

struct Budget(usize, &'static str);

impl Budget {
    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.0 = self.0.checked_sub(bytes).ok_or(Error::Limit(self.1))?;
        Ok(())
    }

    fn copy(&mut self, text: &str) -> Result<String, Error> {
        self.charge(text.len())?;
        Ok(text.to_owned())
    }

    fn push(&mut self, output: &mut String, text: &str) -> Result<(), Error> {
        self.charge(text.len())?;
        output.push_str(text);
        Ok(())
    }
}

pub(crate) use render::allowed_url;
