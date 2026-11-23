//! Public mount labels point to the shared routed landing, never private source coordinates.

use gazette_core::publication::Key;
use serde::Serialize;

use super::{ContentView, ProjectionError, ResolvedAuthored, ResolvedDestination};

/// The documentation source's selected label and exact publication landing.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct DocumentationMountView<'a> {
    /// Explicit workspace-selected label; no repository-name inference.
    pub name: &'a str,
    /// Existing graph object, compiled route and home-site presentation.
    pub landing: ResolvedDestination<'a>,
}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn documentation_mount_view(
        &self,
        id: &Key,
    ) -> Result<Option<DocumentationMountView<'_>>, ProjectionError> {
        let Some(mount) = self
            .source_binding(id)
            .ok()
            .and_then(|source| source.documentation_mount)
        else {
            return Ok(None);
        };
        let landing = &mount.declaration.landing;
        let node = self
            .content
            .graph()
            .node(landing)
            .ok_or_else(|| ProjectionError::Binding {
                id: landing.clone(),
                reason: "documentation mount landing is absent",
            })?;
        let route = self
            .content
            .route(landing)
            .ok_or_else(|| ProjectionError::Binding {
                id: landing.clone(),
                reason: "documentation mount landing requires its compiled route",
            })?;
        if node.site != mount.declaration.site {
            return Err(ProjectionError::Binding {
                id: landing.clone(),
                reason: "documentation mount landing belongs to another site",
            });
        }
        Ok(Some(DocumentationMountView {
            name: &mount.declaration.name,
            landing: ResolvedDestination {
                content: ContentView::new(
                    node,
                    self.content.presentation_for(&node.site),
                    Some(route),
                    self.content.graph().is_documentation(&node.id),
                    self.content.is_listing(node),
                ),
                route,
            },
        }))
    }
}
