//! Catalog lineage admission for emitted views, independent from hosted-doc discovery.

use std::fmt;

use gazette_core::publication::{Key, Project, Visibility};

use super::Discovery;
use crate::authored::PageData;

/// An emitted project origin would expose a nonpublic catalog endpoint.
#[derive(Debug)]
pub struct ProjectOriginError {
    /// Project whose retained origin cannot be emitted completely.
    pub project: Key,
    /// Exact upstream reference; no alternative or stripped-origin fallback.
    pub upstream: Key,
}

impl ProjectOriginError {
    /// Stable class shared by emitted page and graph callers.
    pub fn code(&self) -> &'static str {
        "publication_project_origin"
    }
}

impl fmt::Display for ProjectOriginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "project {} origin references nonpublic upstream {}; no discovery projection produced; review the explicit catalog visibility or publication selection",
            self.project, self.upstream,
        )
    }
}

impl std::error::Error for ProjectOriginError {}

impl Discovery<'_> {
    /// An explicit public catalog reference does not require a hosted documentation
    /// placement. Conversely, hidden documentation does not hide that public identity.
    /// The upstream record is never recursively exported or stripped from its origin.
    pub(crate) fn validate_project_origin(
        &self,
        project: &Project,
    ) -> Result<(), ProjectOriginError> {
        if let Some(upstream) = project.origin.as_ref().and_then(|origin| origin.upstream())
            && !self
                .owner
                .project(upstream)
                .is_some_and(|value| value.visibility == Visibility::Public)
        {
            return Err(ProjectOriginError {
                project: project.id.clone(),
                upstream: upstream.clone(),
            });
        }
        Ok(())
    }

    pub(crate) fn validate_page_projects(
        &self,
        page: &PageData<'_>,
        listing: bool,
    ) -> Result<(), ProjectOriginError> {
        for project in page
            .projects
            .iter()
            .filter(|project| !listing || self.includes_project(project))
        {
            self.validate_project_origin(project.project)?;
        }
        for declaration in &page.project_declarations {
            self.validate_project_origin(declaration.project)?;
        }
        Ok(())
    }
}
