//! Join retained content to the sole workspace source map, without inferring meaning.

use std::fmt;

use gazette_core::publication::Key;

use super::ResolvedAuthored;
use crate::workspace::ContentSource;

/// A resolved object cannot be associated with its configured source publication.
#[derive(Debug)]
pub enum SourceBindingError {
    /// The object or its source path has no configured content binding.
    Missing(Key),
    /// Explicit content home disagrees with its source binding; neither wins silently.
    Home {
        /// Exact content identity.
        content: Key,
        /// Home declared by the content.
        declared: Key,
        /// Site selected by the source's owning declaration.
        configured: Key,
    },
    /// Rendered native documentation requires an existing same-site routed landing.
    DocumentationMount {
        /// Exact declared landing, not a manufactured project identifier.
        landing: Key,
        /// Failed graph constraint.
        reason: &'static str,
    },
}

impl SourceBindingError {
    /// Stable class, shared by direct interfaces and application callers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Missing(_) => "publication_source_binding",
            Self::Home { .. } => "publication_home",
            Self::DocumentationMount { .. } => "publication_documentation_mount_landing",
        }
    }
}

impl fmt::Display for SourceBindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DocumentationMount { landing, reason } => {
                write!(f, "documentation mount landing {landing}: {reason}")
            }
            Self::Missing(content) => {
                write!(f, "content {content} has no configured source binding")
            }
            Self::Home {
                content,
                declared,
                configured,
            } => write!(
                f,
                "content {content} declares home {declared}, but its configured source belongs to {configured}; no source-bound publication produced"
            ),
        }
    }
}

impl std::error::Error for SourceBindingError {}

impl<'a> ResolvedAuthored<'a, '_> {
    /// Resolve source provenance independently of about, mentions and membership.
    ///
    /// The returned binding references the original workspace. Its source key and
    /// capture-relative entry join exact acquisition evidence; no source is reopened.
    /// This relationship grants neither authorship nor public-admission authority.
    ///
    /// # Errors
    /// Rejects absent content/source bindings or a conflicting declared home.
    pub fn source_binding(&self, id: &Key) -> Result<ContentSource<'a>, SourceBindingError> {
        let node = self
            .content
            .graph()
            .node(id)
            .ok_or_else(|| SourceBindingError::Missing(id.clone()))?;
        let workspace = &self.content.compilation().workspace;
        let source = workspace
            .legal_sources()
            .find(|source| {
                source
                    .legal_binding()
                    .is_some_and(|binding| &binding.id == id)
            })
            .filter(|source| {
                self.source_path(id)
                    .is_some_and(|path| source.entry(path).is_some())
            })
            .or_else(|| {
                self.source_path(id)
                    .and_then(|path| workspace.content_source(path))
            })
            .ok_or_else(|| SourceBindingError::Missing(id.clone()))?;
        if source.site != &node.site {
            return Err(SourceBindingError::Home {
                content: id.clone(),
                declared: node.site.clone(),
                configured: source.site.clone(),
            });
        }
        Ok(source)
    }

    /// Require complete source coverage before an application renders the publication.
    /// Standalone syntax/graph inspection may still resolve unbound local documents.
    ///
    /// # Errors
    /// Returns the first binding failure in stable content-identity order.
    pub fn validate_source_bindings(&self) -> Result<(), SourceBindingError> {
        for node in self.content.graph().nodes() {
            self.source_binding(&node.id)?;
        }
        for mount in self
            .content
            .compilation()
            .workspace
            .documentation_mounts()
            .iter()
            .filter(|mount| mount.declaration.render)
        {
            let landing = &mount.declaration.landing;
            let reason = match self.content.graph().node(landing) {
                None => Some("configured page is absent from the selected corpus"),
                Some(node) if node.site != mount.declaration.site => {
                    Some("configured page belongs to another publication")
                }
                Some(_) if self.content.route(landing).is_none() => {
                    Some("configured page has no compiled route")
                }
                Some(_) => None,
            };
            if let Some(reason) = reason {
                return Err(SourceBindingError::DocumentationMount {
                    landing: landing.clone(),
                    reason,
                });
            }
        }
        Ok(())
    }
}
