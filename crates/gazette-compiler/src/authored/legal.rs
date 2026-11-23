//! Reduced owner-held legal metadata lowered without altering its retained body.

use std::fmt;

use gazette_core::{
    publication::{
        ContentLifecycle, Key, LanguageTag,
        content::{ContentKind, ContentNode, PageFamily},
        routing::Route,
    },
    source::{Sha256Digest, Span},
};
use serde::{Deserialize, Serialize};

use crate::{
    document::ParsedDocument,
    toml::{self, PathSegment},
    workspace::{LegalBinding, LegalFamily},
};

/// A reusable owner's source declaration, not a page assigned to a corporate site.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegalSourceMetadata {
    /// Owner-held source identity, distinct from each explicitly composed page identity.
    pub id: Key,
    /// Authored title, never inferred from the filename or output route.
    pub title: String,
    /// Authored plain description; absence remains absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Source lifecycle; composition does not approve or promote a draft.
    pub lifecycle: ContentLifecycle,
    /// Explicit prose language; absence supplies no default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<LanguageTag>,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Declaration {
    content: LegalSourceMetadata,
}

/// Exact frontmatter schema consumed by legal-source lowering, including `[content]`.
#[must_use]
pub fn legal_source_schema() -> schemars::Schema {
    schemars::schema_for!(Declaration)
}

/// One ordinary Utility node plus its original source identity and retained syntax.
/// This is compiler substrate, not a complete notice, provenance binding or admission.
pub struct LoweredLegalSource<'a, 's> {
    /// Explicit output identity/site/route with the source's scalar authored metadata.
    pub content: ContentNode,
    /// Matched source identity, retained independently from the output node identity.
    pub source_content: Key,
    /// The exact original document and body AST; no prose is rewritten or reparsed.
    pub document: &'a ParsedDocument<'s>,
}

impl fmt::Debug for LoweredLegalSource<'_, '_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LoweredLegalSource")
            .field("content", &self.content)
            .field("source_content", &self.source_content)
            .field("source_sha256", &self.document.source_sha256())
            .finish_non_exhaustive()
    }
}

/// A retained legal source does not satisfy its explicit reduced profile or binding.
#[derive(Debug)]
pub enum LegalSourceError {
    /// Native typed TOML rejection, retaining complete-document identity and coordinates.
    Metadata {
        /// Original parser/schema error with its TOML-local range.
        error: toml::Error,
        /// Digest of the whole retained document, including framing and body.
        source_sha256: Sha256Digest,
        /// Original error range lifted into the complete document when available.
        span: Option<Span>,
    },
    /// The selected body's identity differs from the explicitly expected source identity.
    Identity {
        /// Source identity required by the selected workspace binding.
        expected: Key,
        /// Source identity declared by the retained owner's document.
        actual: Key,
        /// Digest of the whole retained document, not a synthesized output document.
        source_sha256: Sha256Digest,
        /// Exact original source-identity field range, when the parser exposes it.
        span: Option<Span>,
    },
}

impl LegalSourceError {
    /// Stable diagnostic class; original metadata causes remain separately inspectable.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Metadata { .. } => "legal_source_metadata",
            Self::Identity { .. } => "legal_source_identity",
        }
    }
}

impl fmt::Display for LegalSourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Metadata { error, span, .. } => {
                write!(
                    formatter,
                    "legal source metadata: {error}; document range {span:?}"
                )
            }
            Self::Identity {
                expected, actual, ..
            } => write!(
                formatter,
                "legal source identity {actual} does not match the selected identity {expected}"
            ),
        }
    }
}

impl std::error::Error for LegalSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Metadata { error, .. } => Some(error),
            Self::Identity { .. } => None,
        }
    }
}

/// Lower one explicitly bound retained source into an ordinary Utility page.
///
/// The caller supplies the publication site and validated workspace binding.
/// This operation never acquires input, changes the body, resolves roles, creates
/// a corporate site, or admits an output to discovery or publication.
///
/// # Errors
/// Refuses metadata outside the reduced profile and an unmatched source identity.
pub fn lower_legal_source<'a, 's>(
    document: &'a ParsedDocument<'s>,
    site: &Key,
    binding: &LegalBinding,
) -> Result<LoweredLegalSource<'a, 's>, LegalSourceError> {
    let source = read(document)?;
    compose(document, site, binding, &source)
}

pub(super) fn read(document: &ParsedDocument<'_>) -> Result<LegalSourceMetadata, LegalSourceError> {
    let Declaration { content: source } = document
        .metadata()
        .deserialize::<Declaration>()
        .map_err(|error| {
            let span = match &error {
                toml::Error::Invalid { span, .. } => span
                    .and_then(|span| span.in_parent(document.source(), document.metadata_span())),
                _ => None,
            };
            LegalSourceError::Metadata {
                error,
                source_sha256: document.source_sha256(),
                span,
            }
        })?;
    Ok(source)
}

pub(super) fn compose<'a, 's>(
    document: &'a ParsedDocument<'s>,
    site: &Key,
    binding: &LegalBinding,
    source: &LegalSourceMetadata,
) -> Result<LoweredLegalSource<'a, 's>, LegalSourceError> {
    if source.id != binding.source_content {
        let span = document
            .metadata()
            .span_at(&[
                PathSegment::Key("content".into()),
                PathSegment::Key("id".into()),
            ])
            .and_then(|span| span.in_parent(document.source(), document.metadata_span()));
        return Err(LegalSourceError::Identity {
            expected: binding.source_content.clone(),
            actual: source.id.clone(),
            source_sha256: document.source_sha256(),
            span,
        });
    }
    Ok(LoweredLegalSource {
        source_content: source.id.clone(),
        document,
        content: ContentNode {
            id: binding.id.clone(),
            site: site.clone(),
            title: source.title.clone(),
            description: source.description.clone(),
            lifecycle: source.lifecycle,
            language: source.language.clone(),
            keywords: vec![],
            schema_type: None,
            subject: None,
            license: None,
            citation: None,
            published: None,
            modified: None,
            taxonomies: vec![],
            attributions: vec![],
            slug: None,
            route: Some(Route {
                path: binding.route.clone(),
                canonical: None,
                aliases: binding.aliases.clone(),
                mirrors: vec![],
            }),
            structure: ContentKind::Page {
                family: match binding.family {
                    LegalFamily::Utility => PageFamily::Utility,
                },
            },
        },
    })
}
