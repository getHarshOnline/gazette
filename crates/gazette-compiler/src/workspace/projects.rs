//! Exact project lineage; no repository observation, role inheritance or content hierarchy.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::publication::Key;

use super::{Diagnostic, resolve::report};

pub(super) fn validate(
    projects: &BTreeMap<Key, super::ProjectDeclaration>,
    errors: &mut Vec<Diagnostic>,
) {
    validate_catalog(
        projects
            .values()
            .filter_map(super::ProjectDeclaration::inline),
        projects.keys(),
        errors,
    );
}

pub(super) fn validate_catalog<'a>(
    facts: impl Iterator<Item = &'a gazette_core::publication::Project>,
    ids: impl Iterator<Item = &'a Key>,
    errors: &mut Vec<Diagnostic>,
) {
    let known: BTreeSet<_> = ids.collect();
    let projects: BTreeMap<_, _> = facts.map(|project| (&project.id, project)).collect();
    // Duplicate declarations have no selected body from which to derive an edge.
    if errors
        .iter()
        .any(|error| error.code == "workspace_duplicate" && error.field == "projects")
    {
        return;
    }
    for project in projects.values() {
        if let Some(programme) = &project.programme
            && (!known.contains(programme)
                || programme == &project.id
                || projects.get(programme).is_some_and(|parent| {
                    parent.kind != Some(gazette_core::publication::ProjectKind::ResearchProject)
                        || parent.programme.is_some()
                }))
        {
            report(
                errors,
                "workspace_project_programme",
                "projects.programme",
                Some(&project.id),
            );
        }
        if let Some(upstream) = project.origin.as_ref().and_then(|origin| origin.upstream())
            && !known.contains(upstream)
        {
            report(
                errors,
                "workspace_reference",
                &field(&project.id),
                Some(upstream),
            );
        }
    }
    // Each project has at most one upstream. Mark completed paths once so shared
    // ancestors do not cause repeated walks; source limits bound all stored keys.
    let mut complete: BTreeSet<&Key> = BTreeSet::new();
    for &id in projects.keys() {
        let mut path: Vec<&Key> = Vec::new();
        let mut positions: BTreeMap<&Key, usize> = BTreeMap::new();
        let mut cursor = Some(id);
        while let Some(id) = cursor {
            if complete.contains(id) {
                break;
            }
            if let Some(&position) = positions.get(id) {
                for id in &path[position..] {
                    report(
                        errors,
                        "workspace_project_lineage_cycle",
                        &field(id),
                        Some(id),
                    );
                }
                break;
            }
            positions.insert(id, path.len());
            path.push(id);
            cursor = projects
                .get(id)
                .and_then(|project| project.origin.as_ref())
                .and_then(|origin| origin.upstream());
        }
        complete.extend(path);
    }
}

fn field(id: &Key) -> String {
    format!("projects[{:?}].origin.upstream", id.as_str())
}

/// Validate the same public facts whether they came from configuration or authored content.
pub(super) fn validate_facts<'a>(
    projects: impl Iterator<Item = &'a gazette_core::publication::Project>,
    errors: &mut Vec<Diagnostic>,
) {
    for project in projects {
        let mut paper_urls = BTreeSet::new();
        if project.papers.iter().any(|paper| {
            paper.name.trim().is_empty()
                || !valid_identity(&paper.url)
                || !paper_urls.insert(&paper.url)
        }) {
            report(
                errors,
                "workspace_project_paper",
                "projects.papers",
                Some(&project.id),
            );
        }
        if let Some(source) = &project.source
            && (project.repository.is_none()
                || !valid_identity(&source.license_url)
                || source
                    .notice_url
                    .as_ref()
                    .is_some_and(|url| !valid_identity(url))
                || !valid_identity(&source.citation_url))
        {
            report(
                errors,
                "workspace_project_source",
                "projects.source",
                Some(&project.id),
            );
        }
        if project
            .identity
            .as_ref()
            .is_some_and(|value| !valid_identity(value))
        {
            report(
                errors,
                "workspace_project_identity",
                "projects.identity",
                Some(&project.id),
            );
        }
        if project
            .name
            .as_ref()
            .is_some_and(|name| name.trim().is_empty() || name.chars().any(char::is_control))
        {
            report(
                errors,
                "workspace_project_name",
                "projects.name",
                Some(&project.id),
            );
        }
        if project
            .resources
            .as_ref()
            .and_then(|resources| resources.github.as_ref())
            .is_some_and(|url| project.repository.is_some() || !super::repositories::valid(url))
        {
            report(
                errors,
                "workspace_project_resources",
                "projects.resources.github",
                Some(&project.id),
            );
        }
        if project
            .resources
            .as_ref()
            .and_then(|resources| resources.code_documentation.as_ref())
            .is_some_and(|url| {
                !url.get(..8)
                    .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
                    || !super::repositories::valid(url)
            })
        {
            report(
                errors,
                "workspace_project_resources",
                "projects.resources.code_documentation",
                Some(&project.id),
            );
        }
        if project
            .repository
            .as_ref()
            .is_some_and(|value| !super::repositories::valid(value))
        {
            report(
                errors,
                "workspace_project_repository",
                "projects.repository",
                Some(&project.id),
            );
        }
    }
}

pub(super) fn valid_identity(value: &str) -> bool {
    if !super::repositories::valid(value) {
        return false;
    }
    let Ok(url) = url::Url::parse(value) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(host)) => {
            !(host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local"))
        }
        Some(url::Host::Ipv4(ip)) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast())
        }
        Some(url::Host::Ipv6(ip)) => {
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|ip| ip.is_private() || ip.is_loopback() || ip.is_link_local()))
        }
        None => false,
    }
}

pub(super) fn validate_identities(
    content: &super::ResolvedContent<'_>,
) -> Result<(), gazette_core::publication::content::Error> {
    use gazette_core::publication::content::{Diagnostic, Error};
    let mut identities = BTreeMap::new();
    for node in content.graph().nodes() {
        if let Some(route) = content.route(&node.id) {
            for page in [false, true] {
                identities.insert(
                    identity_key(&crate::authored::public_identity::work(
                        node,
                        Some(route),
                        page,
                    )),
                    node.id.clone(),
                );
            }
        }
    }
    let mut errors = Vec::new();
    for brand in content.compilation().workspace.brands() {
        let mut register = |identity: String| {
            if let Some(other) = identities.insert(identity_key(&identity), brand.id.clone()) {
                errors.push(Diagnostic {
                    code: "publication_identity_collision",
                    source: brand.id.clone(),
                    target: Some(other),
                });
            }
        };
        if let Some(identity) = &brand.identity {
            register(identity.clone());
        }
        for publication in &brand.publications {
            if let Some(route) = publication
                .home
                .as_ref()
                .filter(|home| {
                    content
                        .graph()
                        .node(home)
                        .is_some_and(|node| node.site == publication.site)
                })
                .and_then(|home| content.route(home))
            {
                register(format!("{}#website", route.public_url));
                if brand.identity.is_none() {
                    register(format!("{}#brand", route.public_url));
                }
            }
        }
    }
    for project in content.projects() {
        let route = project
            .resources
            .as_ref()
            .and_then(|resources| resources.landing.as_ref())
            .and_then(|id| content.route(id));
        let identity = crate::authored::public_identity::project(project, route);
        if let Some(other) = identities.insert(identity_key(&identity), project.id.clone()) {
            errors.push(Diagnostic {
                code: "project_identity_collision",
                source: project.id.clone(),
                target: Some(other),
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Invalid(errors))
    }
}

fn identity_key(value: &str) -> String {
    url::Url::parse(value).map_or_else(
        |_| value.to_owned(),
        |mut url| {
            if url.fragment() == Some("") {
                url.set_fragment(None);
            }
            url.to_string()
        },
    )
}
