//! Complete owner atoms and exact capability references, not provider activation.

use gazette_core::publication::{Discussion, Key};

use super::{Diagnostic, Workspace, resolve::report};

pub(super) fn validate(workspace: &Workspace, errors: &mut Vec<Diagnostic>) {
    for site in workspace.sites() {
        if let Some(atom) = &site.discussion {
            if !atom.is_valid()
                || workspace
                    .publication_for(&site.id)
                    .and_then(|publication| publication.repository.as_ref())
                    .is_none()
            {
                report(
                    errors,
                    "workspace_discussion",
                    "sites.discussion",
                    Some(&site.id),
                );
            }
            capability(workspace, &site.id, &site.id, atom, errors);
        }
    }
    validate_projects(workspace, workspace.projects(), errors);
}

pub(super) fn validate_projects<'a>(
    workspace: &Workspace,
    projects: impl Iterator<Item = &'a gazette_core::publication::Project>,
    errors: &mut Vec<Diagnostic>,
) {
    for project in projects {
        if let Some(atom) = &project.discussion {
            if !atom.is_valid() || project.repository.is_none() {
                report(
                    errors,
                    "workspace_discussion",
                    "projects.discussion",
                    Some(&project.id),
                );
            }
            for placement in workspace
                .placements()
                .iter()
                .filter(|placement| placement.render && placement.project == project.id)
            {
                capability(workspace, &placement.site, &project.id, atom, errors);
            }
        }
    }
}

fn capability(
    workspace: &Workspace,
    site: &Key,
    owner: &Key,
    atom: &Discussion,
    errors: &mut Vec<Diagnostic>,
) {
    if workspace
        .composition_for(site)
        .is_none_or(|composition| !composition.capabilities().contains_key(&atom.capability))
    {
        report(
            errors,
            "workspace_discussion_capability",
            "discussion.capability",
            Some(owner),
        );
    }
}
