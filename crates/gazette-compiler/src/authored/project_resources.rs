//! One explicit project-resource projection shared by pages and hosted components.
use super::{ProjectionError, ResolvedAuthored, links::ResolvedReference};
use crate::{markdown::LinkDestination, workspace::ListingSelection};
use gazette_core::{
    markdown::{
        ProjectResourceAction, ProjectResourceDestination, ProjectResourceKind,
        ProjectResourcePresentation, ProjectResourcesView,
    },
    publication::{
        Key, Project,
        content::{ContentKind, ContentNode, PageFamily, ProjectDeclarationKind, ProjectRelation},
    },
};

/// Shallow navigation selected by the project owner, without changing the page's subject evidence.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ProjectLanding<'a> {
    /// Exact content identity named by the project-owned resource.
    pub id: &'a Key,
    /// Literal landing title; callers may retain the separately authored project name.
    pub title: &'a str,
    /// Already resolved serving URL, including the selected preview address.
    pub url: &'a str,
    /// Existing destination facts relative to the page displaying this project context.
    pub destination: LinkDestination,
}

/// Shallow facts from the validated provider page selected by a project's code-documentation URL.
#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
pub struct ProjectDocumentation {
    /// Exact public reading address, independent from the resource action's preview URL.
    pub public_url: String,
    /// Provider-admitted title; absence never falls back to the route or project name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

pub(super) fn component_destination(value: LinkDestination) -> ProjectResourceDestination {
    ProjectResourceDestination {
        kind: value.kind,
        content: value.content,
        site: value.site,
        relationships: value.relationships,
        presentation: value
            .presentation
            .map(|presentation| ProjectResourcePresentation {
                theme: presentation.theme.as_key().clone(),
                palette: presentation.palette,
            }),
    }
}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn project_works<'a>(
        &'a self,
        project: &Project,
        discovery: Option<&super::Discovery<'_>>,
    ) -> Result<Vec<super::ContentView<'a>>, ProjectionError> {
        let Some(resources) = &project.resources else {
            return Ok(Vec::new());
        };
        let graph = self.content().graph();
        let mut seen = std::collections::BTreeSet::new();
        let mut works = Vec::new();
        for id in [
            &resources.landing,
            &resources.learn,
            &resources.mentions,
            &resources.blogs,
            &resources.thoughts,
        ]
        .into_iter()
        .flatten()
        {
            if discovery.is_some_and(|view| !view.contains(id))
                || !graph
                    .project_relations(id)
                    .and_then(|relations| relations.get(&project.id))
                    .is_some_and(|relation| relation.relation() == ProjectRelation::About)
            {
                continue;
            }
            let node = graph.node(id).ok_or_else(|| ProjectionError::Binding {
                id: id.clone(),
                reason: "project reading destination is absent",
            })?;
            let route = self
                .content()
                .route(id)
                .ok_or_else(|| ProjectionError::Binding {
                    id: id.clone(),
                    reason: "project reading destination has no route",
                })?;
            if seen.insert(super::public_identity::work(node, Some(route), false)) {
                works.push(super::ContentView::new(
                    node,
                    None,
                    Some(route),
                    graph.documentation_navigation_owner(id).is_some(),
                    self.content().is_listing(node),
                ));
            }
        }
        Ok(works)
    }

    pub(super) fn project_landing_target<'a>(
        &'a self,
        project: &Key,
        id: &Key,
    ) -> Result<(&'a ContentNode, &'a crate::workspace::ResolvedRoute), &'static str> {
        let graph = self.content().graph();
        let target = graph
            .node(id)
            .ok_or("project landing is absent from the captured graph")?;
        if !self
            .content()
            .compilation()
            .workspace
            .is_project_landing(target)
        {
            return Err(
                "project landing must be a project page, root collection or explicitly configured index-family Home",
            );
        }
        if !graph
            .project_relations(id)
            .and_then(|relations| relations.get(project))
            .is_some_and(|relationship| {
                relationship.evidence().iter().any(|evidence| {
                    evidence.source == target.id && evidence.kind == ProjectDeclarationKind::About
                })
            })
        {
            return Err("project landing must directly declare About the selected project");
        }
        if target
            .route
            .as_ref()
            .is_some_and(|route| route.canonical.is_some())
        {
            return Err("project landing must have its own canonical route");
        }
        let route = self
            .content()
            .route(id)
            .ok_or("project landing has no compiled route")?;
        Ok((target, route))
    }

    pub(super) fn project_landing_for<'a>(
        &'a self,
        project: &Project,
        source: &ContentNode,
    ) -> Result<Option<ProjectLanding<'a>>, ProjectionError> {
        let Some(id) = project
            .resources
            .as_ref()
            .and_then(|resources| resources.landing.as_ref())
        else {
            return Ok(None);
        };
        let (target, route) = self
            .project_landing_target(&project.id, id)
            .map_err(|reason| ProjectionError::Binding {
                id: id.clone(),
                reason,
            })?;
        let reference = ResolvedReference {
            asset_path: None,
            destination: route.url.clone(),
            asset: None,
            content: Some(target.id.clone()),
            resource: false,
        };
        Ok(Some(ProjectLanding {
            id: &target.id,
            title: &target.title,
            url: &route.url,
            destination: self.link_destination(source, &route.url, &route.url, Some(&reference)),
        }))
    }

    /// Resolve resources for one About project on its project page, root collection or Home.
    ///
    /// # Errors
    /// Refuses configured resource destinations that cannot bind to the captured graph.
    pub fn project_resources_for(
        &self,
        id: &Key,
    ) -> Result<Option<ProjectResourcesView>, ProjectionError> {
        let graph = self.content().graph();
        let source = graph
            .node(id)
            .ok_or_else(|| ProjectionError::UnknownContent(id.clone()))?;
        let project_page = matches!(
            source.structure,
            ContentKind::Page {
                family: PageFamily::Project
            }
        );
        if !self
            .content()
            .compilation()
            .workspace
            .is_project_landing(source)
        {
            return Ok(None);
        }
        let mut about = graph
            .project_relations(id)
            .into_iter()
            .flat_map(|links| links.iter())
            .filter(|(_, relationship)| relationship.relation() == ProjectRelation::About);
        let Some((subject, _)) = about.next() else {
            return Ok(None);
        };
        if about.next().is_some() {
            return Ok(None);
        }
        let project = self
            .content()
            .project(subject)
            .ok_or_else(|| ProjectionError::Binding {
                id: subject.clone(),
                reason: "project resources require the configured project",
            })?;
        if !project_page {
            if project
                .resources
                .as_ref()
                .and_then(|resources| resources.landing.as_ref())
                != Some(id)
            {
                return Ok(None);
            }
            self.project_landing_target(&project.id, id)
                .map_err(|reason| ProjectionError::Binding {
                    id: id.clone(),
                    reason,
                })?;
        }
        match self.resources_for_project(project, source)? {
            Some(resources) => Ok(Some(resources)),
            None => {
                let view = ProjectResourcesView {
                    project: project.id.clone(),
                    name: project.name.clone(),
                    actions: Vec::new(),
                };
                self.bound_project_resources(&view)?;
                Ok(Some(view))
            }
        }
    }

    pub(super) fn resources_for_project(
        &self,
        project: &Project,
        source: &ContentNode,
    ) -> Result<Option<ProjectResourcesView>, ProjectionError> {
        let empty = gazette_core::publication::ProjectResources::default();
        let resources = match &project.resources {
            Some(resources) => resources,
            None if project.repository.is_some() || project.available_through.is_some() => &empty,
            None => return Ok(None),
        };
        let fail = |id: &Key, reason| ProjectionError::Binding {
            id: id.clone(),
            reason,
        };
        let graph = self.content().graph();
        for blogs_id in [&resources.mentions, &resources.blogs, &resources.thoughts]
            .into_iter()
            .flatten()
        {
            let blogs = graph
                .node(blogs_id)
                .ok_or_else(|| fail(blogs_id, "project article destination is absent"))?;
            let declaration = self
                .content()
                .compilation()
                .workspace
                .publication_for(&blogs.site)
                .and_then(|publication| {
                    publication
                        .listings
                        .iter()
                        .find(|listing| &listing.content == blogs_id)
                })
                .ok_or_else(|| {
                    fail(
                        blogs_id,
                        "project article destination requires its native listing",
                    )
                })?;
            if !matches!(&declaration.select,ListingSelection::ProjectArticles {project:subject,..} if subject==&project.id)
            {
                return Err(fail(
                    blogs_id,
                    "project article listing must select the same project",
                ));
            }
            self.listing_subject(&declaration.select, blogs)
                .map_err(ProjectionError::Listing)?;
        }
        enum Target<'a> {
            Content(&'a Key),
            Url(&'a str),
        }
        let mut actions = Vec::new();
        let code = project.source.as_ref();
        for (kind, target) in [
            (
                ProjectResourceKind::CodeDocumentation,
                resources.code_documentation.as_deref().map(Target::Url),
            ),
            (
                ProjectResourceKind::Learn,
                resources.learn.as_ref().map(Target::Content),
            ),
            (
                ProjectResourceKind::Github,
                project
                    .repository
                    .as_deref()
                    .or(resources.github.as_deref())
                    .map(Target::Url),
            ),
            (
                ProjectResourceKind::License,
                code.map(|source| Target::Url(&source.license_url)),
            ),
            (
                ProjectResourceKind::Notice,
                code.and_then(|source| source.notice_url.as_deref())
                    .map(Target::Url),
            ),
            (
                ProjectResourceKind::Mentions,
                resources.mentions.as_ref().map(Target::Content),
            ),
            (
                ProjectResourceKind::Blogs,
                resources.blogs.as_ref().map(Target::Content),
            ),
            (
                ProjectResourceKind::Thoughts,
                resources.thoughts.as_ref().map(Target::Content),
            ),
            (
                ProjectResourceKind::AvailableThrough,
                project
                    .available_through
                    .as_ref()
                    .map(|through| Target::Content(&through.content)),
            ),
        ] {
            let Some(target) = target else { continue };
            let (url, destination) = match target {
                Target::Url(url) => {
                    let projected = self
                        .preview_public_link(url)
                        .map_err(|_| {
                            fail(
                                &project.id,
                                "project resource requires a safe URL and an explicit provider in local mode",
                            )
                        })?
                        .unwrap_or_else(|| url.to_owned());
                    let destination = self.link_destination(source, url, &projected, None);
                    (projected, destination)
                }
                Target::Content(id) => {
                    let node = graph.node(id).ok_or_else(|| {
                        fail(id, "project resource content destination is absent")
                    })?;
                    if kind == ProjectResourceKind::Learn {
                        // A help landing must not advertise itself as its own next step.
                        if id == &source.id || resources.landing.as_ref() == Some(id) {
                            continue;
                        }
                        let documentation =
                            matches!(
                                node.structure,
                                ContentKind::Page {
                                    family: PageFamily::Documentation
                                }
                            ) || matches!(
                                node.structure,
                                ContentKind::Page {
                                    family: PageFamily::Article
                                } | ContentKind::Collection { .. }
                            ) && graph.documentation_navigation_owner(id).is_some();
                        if !documentation {
                            return Err(fail(
                                id,
                                "Learn must select a documentation page or a documentation collection's page",
                            ));
                        }
                    }
                    let route = self.content().route(id).ok_or_else(|| {
                        fail(id, "project resource content destination has no route")
                    })?;
                    let resolved = ResolvedReference {
                        asset_path: None,
                        destination: route.url.clone(),
                        asset: None,
                        content: Some(id.clone()),
                        resource: false,
                    };
                    (
                        route.url.clone(),
                        self.link_destination(source, &route.url, &route.url, Some(&resolved)),
                    )
                }
            };
            actions.push(ProjectResourceAction {
                kind,
                url,
                destination: component_destination(destination),
            });
        }
        let view = ProjectResourcesView {
            project: project.id.clone(),
            name: project.name.clone(),
            actions,
        };
        self.bound_project_resources(&view)?;
        Ok(Some(view))
    }

    fn bound_project_resources(&self, view: &ProjectResourcesView) -> Result<(), ProjectionError> {
        let mut bytes = gazette_core::render::LimitedWriter::new(
            std::io::sink(),
            self.projection_limits.text_bytes,
        );
        serde_json::to_writer(&mut bytes, &view)
            .map_err(|_| ProjectionError::Limit("project resources bytes"))?;
        if bytes.exceeded() || view.actions.len() > self.projection_limits.edges {
            return Err(ProjectionError::Limit("project resources bytes"));
        }
        Ok(())
    }
}
