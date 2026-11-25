//! Join captured landing facts once, before any publication projection.

use super::{ProjectDeclaration, Workspace};
use gazette_core::publication::{
    Key, Project,
    content::{ContentKind, ContentNode, Diagnostic, Graph, PageFamily, ProjectDeclarationKind},
};
use std::collections::BTreeMap;

impl Workspace {
    /// A programme may use its explicitly selected publication Home as its introduction.
    /// An index's address or About edge alone does not make it a project landing.
    pub(crate) fn is_project_landing(&self, node: &ContentNode) -> bool {
        match node.structure {
            ContentKind::Page {
                family: PageFamily::Project,
            }
            | ContentKind::Collection { parent: None, .. } => true,
            ContentKind::Page {
                family: PageFamily::Index,
            } => {
                self.publication_for(&node.site)
                    .and_then(|publication| publication.home.as_ref())
                    == Some(&node.id)
            }
            _ => false,
        }
    }
}

pub(super) fn resolve(
    workspace: &Workspace,
    graph: &Graph,
    mut authored: BTreeMap<Key, Project>,
    errors: &mut Vec<Diagnostic>,
) -> BTreeMap<Key, Project> {
    let mut projects = BTreeMap::new();
    let primary_homes: BTreeMap<_, _> = workspace
        .placements()
        .iter()
        .filter(|placement| placement.role.is_primary())
        .map(|placement| (&placement.project, &placement.site))
        .collect();
    for declaration in workspace.project_declarations() {
        let mut project = match declaration {
            ProjectDeclaration::Inline(project) => (**project).clone(),
            ProjectDeclaration::Landing(source) => {
                let Some(node) = graph.node(&source.landing) else {
                    failure(
                        errors,
                        "project_landing_missing",
                        &source.landing,
                        &source.id,
                    );
                    continue;
                };
                let Some(mut project) = authored.remove(&source.landing) else {
                    failure(
                        errors,
                        "project_landing_facts_missing",
                        &source.landing,
                        &source.id,
                    );
                    continue;
                };
                let valid_kind = workspace.is_project_landing(node);
                let about = graph
                    .project_relations(&source.landing)
                    .and_then(|relations| relations.get(&source.id))
                    .is_some_and(|relation| {
                        relation.evidence().iter().any(|evidence| {
                            evidence.source == source.landing
                                && evidence.kind == ProjectDeclarationKind::About
                        })
                    });
                let wrong_home = primary_homes
                    .get(&source.id)
                    .is_some_and(|site| **site != node.site);
                if project.id != source.id
                    || !valid_kind
                    || !about
                    || wrong_home
                    || node
                        .route
                        .as_ref()
                        .is_none_or(|route| route.canonical.is_some())
                {
                    failure(errors, "project_landing_owner", &source.landing, &source.id);
                    continue;
                }
                if project
                    .resources
                    .as_ref()
                    .is_some_and(|resources| resources.code_documentation.is_some())
                {
                    failure(
                        errors,
                        "project_documentation_owner",
                        &source.landing,
                        &source.id,
                    );
                    continue;
                }
                let resources = project.resources.get_or_insert_with(Default::default);
                if resources
                    .landing
                    .as_ref()
                    .is_some_and(|landing| landing != &source.landing)
                {
                    failure(
                        errors,
                        "project_landing_conflict",
                        &source.landing,
                        &source.id,
                    );
                    continue;
                }
                resources.landing = Some(source.landing.clone());
                project
            }
        };
        match super::documentation::project_url(workspace, &project.id) {
            Ok(Some(url)) => {
                let resources = project.resources.get_or_insert_with(Default::default);
                if resources
                    .code_documentation
                    .as_ref()
                    .is_some_and(|declared| declared != &url)
                {
                    failure(
                        errors,
                        "project_documentation_conflict",
                        &project.id,
                        &project.id,
                    );
                }
                resources.code_documentation = Some(url);
            }
            Ok(None) => {}
            Err(code) => failure(errors, code, &project.id, &project.id),
        }
        if let Some(through) = &project.available_through
            && (project.availability != gazette_core::publication::Availability::Gated
                || workspace.site(&through.site).is_none()
                || graph
                    .node(&through.content)
                    .is_none_or(|node| node.site != through.site || node.route.is_none()))
        {
            failure(
                errors,
                "project_availability_destination",
                &project.id,
                &through.content,
            );
        }
        if let Some(source) = &project.source
            && workspace.principal(&source.author).is_none_or(|principal| {
                principal.kind != gazette_core::publication::PrincipalKind::Person
            })
        {
            failure(errors, "project_source_author", &project.id, &source.author);
        }
        if let Some(source) = &project.source
            && workspace.principal(&source.rights_holder).is_none()
        {
            failure(
                errors,
                "project_source_rights_holder",
                &project.id,
                &source.rights_holder,
            );
        }
        projects.insert(project.id.clone(), project);
    }
    for (landing, project) in authored {
        failure(errors, "project_facts_unselected", &landing, &project.id);
    }
    let mut configuration_errors = Vec::new();
    super::projects::validate_facts(projects.values(), &mut configuration_errors);
    super::projects::validate_catalog(
        projects.values(),
        projects.keys(),
        &mut configuration_errors,
    );
    super::discussion::validate_projects(workspace, projects.values(), &mut configuration_errors);
    super::processing::validate_with_projects(
        workspace,
        Some(&projects),
        &mut configuration_errors,
    );
    for error in configuration_errors {
        if let Some(id) = error.key {
            let source = match workspace.project_declaration(&id) {
                Some(ProjectDeclaration::Landing(source)) => source.landing.clone(),
                _ => id.clone(),
            };
            errors.push(Diagnostic {
                code: error.code,
                source,
                target: Some(id),
            });
        } else if let Some(site) = workspace.sites().next() {
            // A compiled workspace always has a site; preserve unkeyed provider errors too.
            errors.push(Diagnostic {
                code: error.code,
                source: site.id.clone(),
                target: None,
            });
        }
    }
    projects
}

fn failure(errors: &mut Vec<Diagnostic>, code: &'static str, source: &Key, target: &Key) {
    errors.push(Diagnostic {
        code,
        source: source.clone(),
        target: Some(target.clone()),
    });
}
