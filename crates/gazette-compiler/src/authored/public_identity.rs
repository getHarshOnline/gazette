//! Public entity and work addresses, separate from native inspection keys and serving URLs.

use crate::workspace::ResolvedRoute;
use gazette_core::{
    publication::{
        Key, Project, ProjectKind,
        content::{ContentKind, ContentNode, PageFamily, SchemaType},
    },
    source::Sha256Digest,
};

pub(super) fn blank(space: &str, id: &Key) -> String {
    format!("_:{space}_{}", Sha256Digest::of(id.as_str().as_bytes()))
}

pub(super) fn article(node: &ContentNode) -> bool {
    matches!(
        node.structure,
        ContentKind::Page {
            family: PageFamily::Article
        }
    )
}

pub(crate) fn work(node: &ContentNode, route: Option<&ResolvedRoute>, page: bool) -> String {
    match route {
        Some(route) if article(node) && !page => format!("{}#article", route.public_url),
        Some(route) => route.public_url.clone(),
        None => blank(
            if article(node) && page {
                "page"
            } else {
                "content"
            },
            &node.id,
        ),
    }
}

pub(super) fn work_type(node: &ContentNode, page: bool, listing: bool) -> &'static str {
    match &node.structure {
        ContentKind::Page {
            family: PageFamily::Article,
        } if !page => match node.schema_type.unwrap_or(SchemaType::Article) {
            SchemaType::Article => "Article",
            SchemaType::BlogPosting => "BlogPosting",
            SchemaType::TechArticle => "TechArticle",
            SchemaType::AboutPage => "Article",
        },
        ContentKind::Collection { .. } | ContentKind::Journey { .. } => "CollectionPage",
        ContentKind::Page {
            family: PageFamily::Index,
        } if listing => "CollectionPage",
        ContentKind::Page {
            family: PageFamily::Utility,
        } if node.schema_type == Some(SchemaType::AboutPage) => "AboutPage",
        ContentKind::Page { .. } => "WebPage",
    }
}

pub(crate) fn project(project: &Project, landing: Option<&ResolvedRoute>) -> String {
    project.identity.clone().unwrap_or_else(|| {
        landing.map_or_else(
            || blank("project", &project.id),
            |route| format!("{}#entity", route.public_url),
        )
    })
}

pub(super) fn project_type(kind: Option<ProjectKind>) -> &'static str {
    match kind {
        None => "Thing",
        Some(ProjectKind::SoftwareApplication) => "SoftwareApplication",
        Some(ProjectKind::SoftwareSourceCode) => "SoftwareSourceCode",
        Some(ProjectKind::CreativeWork) => "CreativeWork",
        Some(ProjectKind::Collection) => "Collection",
        Some(ProjectKind::Project) => "Project",
        Some(ProjectKind::ResearchProject) => "ResearchProject",
    }
}
