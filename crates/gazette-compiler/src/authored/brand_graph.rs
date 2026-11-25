//! Project brand ownership and portfolio relationships from the resolved publication.

use crate::workspace::{Brand, ResolvedContent};
use gazette_core::publication::{Key, Principal, Project, ProjectKind, Visibility};

use super::{PrincipalWebsiteView, public_identity};

#[derive(Debug)]
pub(super) struct BrandGraph<'a> {
    pub current: BrandNode<'a>,
    pub owner: Option<(&'a Principal, Option<PrincipalWebsiteView>)>,
    pub siblings: Vec<BrandNode<'a>>,
}

#[derive(Debug)]
pub(super) struct BrandNode<'a> {
    pub brand: &'a Brand,
    pub identity: String,
    pub home: Option<&'a str>,
    pub parent: Option<String>,
    pub children: Vec<String>,
    pub subject: Option<ProjectNode<'a>>,
    pub projects: Vec<ProjectNode<'a>>,
}

#[derive(Debug)]
pub(super) struct ProjectNode<'a> {
    pub project: &'a Project,
    pub identity: String,
}

fn home<'a>(content: &'a ResolvedContent<'_>, brand: &Brand) -> Option<&'a str> {
    let [publication] = brand.publications.as_slice() else {
        return None;
    };
    Some(&content.route(publication.home.as_ref()?)?.public_url)
}

fn identity(content: &ResolvedContent<'_>, brand: &Brand) -> Option<String> {
    brand
        .identity
        .clone()
        .or_else(|| home(content, brand).map(|url| format!("{url}#brand")))
}

fn project<'a>(content: &'a ResolvedContent<'_>, key: &Key) -> Option<ProjectNode<'a>> {
    let project = content.project(key)?;
    if project.visibility != Visibility::Public {
        return None;
    }
    let route = project
        .resources
        .as_ref()
        .and_then(|resources| resources.landing.as_ref())
        .and_then(|landing| content.route(landing));
    if route.is_none() && project.identity.is_none() {
        return None;
    }
    Some(ProjectNode {
        project,
        identity: public_identity::project(project, route),
    })
}

pub(super) fn graph<'a>(
    content: &'a ResolvedContent<'_>,
    site: &Key,
    page: &Key,
) -> Option<BrandGraph<'a>> {
    let workspace = &content.compilation().workspace;
    let (selected, publication) = workspace.brands().find_map(|brand| {
        brand
            .publications
            .iter()
            .find(|publication| &publication.site == site)
            .map(|publication| (brand, publication))
    })?;
    if publication.home.as_ref() != Some(page) && publication.about.as_ref() != Some(page) {
        return None;
    }
    let node = |brand: &'a Brand| -> Option<BrandNode<'a>> {
        let selected_home = publication.home.as_ref().and_then(|id| content.route(id));
        let home = if brand.id == selected.id {
            selected_home.map(|route| route.public_url.as_str())
        } else {
            home(content, brand)
        };
        let parent = brand
            .parent
            .as_ref()
            .and_then(|id| workspace.brands().find(|value| &value.id == id))
            .and_then(|parent| identity(content, parent));
        Some(BrandNode {
            brand,
            identity: brand
                .identity
                .clone()
                .or_else(|| home.map(|url| format!("{url}#brand")))?,
            home,
            parent,
            children: workspace
                .brands()
                .filter(|child| child.parent.as_ref() == Some(&brand.id))
                .filter_map(|child| identity(content, child))
                .collect(),
            subject: brand.subject.as_ref().and_then(|id| project(content, id)),
            projects: brand
                .projects
                .iter()
                .filter_map(|id| project(content, id))
                .collect(),
        })
    };
    let owner = selected
        .owner
        .as_ref()
        .and_then(|id| workspace.principal(id))
        .map(|principal| (principal, content.principal_website(principal, site)));
    let siblings = selected.owner.as_ref().map_or_else(Vec::new, |owner| {
        workspace
            .brands()
            .filter(|brand| &brand.id != &selected.id && brand.owner.as_ref() == Some(owner))
            .filter_map(node)
            .collect()
    });
    Some(BrandGraph {
        current: node(selected)?,
        siblings,
        owner,
    })
}

pub(super) fn portfolio_brands(content: &ResolvedContent<'_>, project: &Project) -> Vec<String> {
    content
        .compilation()
        .workspace
        .brands()
        .filter(|brand| brand.projects.contains(&project.id))
        .filter_map(|brand| identity(content, brand))
        .collect()
}

pub(super) fn research_brand(content: &ResolvedContent<'_>, project: &Project) -> Option<String> {
    if project.kind != Some(ProjectKind::ResearchProject) {
        return None;
    }
    content
        .compilation()
        .workspace
        .brands()
        .find(|brand| brand.subject.as_ref() == Some(&project.id))
        .and_then(|brand| identity(content, brand))
}
