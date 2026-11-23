//! Lower existing catalog values without resolving or inferring their meaning.

use super::rows::{Cell as C, Rows};
use gazette_core::{
    publication::{Discussion, Key, PrincipalWebsite, registry::Projection},
    store::Error,
};

pub(super) fn append(model: &Projection<'_>, rows: &mut Rows) -> Result<(), Error> {
    for (id, extension) in &model.extensions {
        if *id != &extension.id {
            return Err(Error::RegistryVerification);
        }
        let schema = rows.json(&extension.schema)?;
        rows.add("gazette_registry_extensions", vec![C::text(id), schema])?;
    }
    for (id, principal) in &model.principals {
        if *id != &principal.id {
            return Err(Error::RegistryVerification);
        }
        rows.add(
            "gazette_registry_principals",
            vec![
                C::text(id),
                C::enumeration(&principal.kind)?,
                C::optional(principal.name.as_ref()),
            ],
        )?;
        if let Some(website) = &principal.website {
            let (kind, site, url) = match website {
                PrincipalWebsite::Site { site } => ("site", Some(site), None),
                PrincipalWebsite::External { url } => ("external", None, Some(url)),
            };
            rows.add(
                "gazette_registry_principal_websites",
                vec![
                    C::text(id),
                    C::text(kind),
                    C::optional(site),
                    C::optional(url),
                ],
            )?;
        }
        for (ordinal, url) in principal.same_as.iter().enumerate() {
            rows.add(
                "gazette_registry_principal_same_as",
                vec![C::text(id), C::ordinal(ordinal)?, C::text(url)],
            )?;
        }
    }
    for (id, project) in &model.projects {
        if *id != &project.id {
            return Err(Error::RegistryVerification);
        }
        rows.add(
            "gazette_registry_projects",
            vec![
                C::text(id),
                C::enumeration(&project.lifecycle)?,
                C::enumeration(&project.visibility)?,
                C::enumeration(&project.availability)?,
                C::optional(project.name.as_ref()),
                C::optional(project.repository.as_ref()),
                C::optional(project.license.as_ref()),
                project
                    .kind
                    .as_ref()
                    .map(C::enumeration)
                    .transpose()?
                    .unwrap_or(C::Null),
                C::optional(project.identity.as_ref()),
            ],
        )?;
        if let Some(resources) = &project.resources {
            rows.add(
                "gazette_registry_project_resources",
                vec![
                    C::text(id),
                    C::optional(resources.learn.as_ref()),
                    C::optional(resources.github.as_ref()),
                    C::optional(resources.blogs.as_ref()),
                    C::optional(resources.thoughts.as_ref()),
                    C::optional(resources.mentions.as_ref()),
                    C::optional(resources.code_documentation.as_ref()),
                ],
            )?;
        }
        if let Some(origin) = &project.origin {
            rows.add(
                "gazette_registry_project_origins",
                vec![
                    C::text(id),
                    C::text(origin.kind()),
                    C::optional(origin.upstream()),
                ],
            )?;
        }
        if let Some(discussion) = &project.discussion {
            append_discussion(
                rows,
                "gazette_registry_project_discussions",
                id,
                project.repository.as_deref(),
                discussion,
            )?;
        }
    }
    for (id, brand) in &model.brands {
        rows.add(
            "gazette_registry_brands",
            vec![C::text(id), C::text(brand.name), C::optional(brand.parent)],
        )?;
    }
    for (id, site) in &model.sites {
        rows.add(
            "gazette_registry_sites",
            vec![
                C::text(id),
                C::text(site.publisher),
                C::optional(site.rights_holder),
                C::optional(site.default_author),
                C::flag(site.deployable),
                C::optional(site.brand),
                C::optional(site.public_base_url.map(|url| url.as_str())),
                C::optional(site.repository),
            ],
        )?;
        if let Some(discussion) = site.discussion {
            append_discussion(
                rows,
                "gazette_registry_site_discussions",
                id,
                site.repository,
                discussion,
            )?;
        }
        for binding in site.roles {
            rows.add(
                "gazette_registry_site_roles",
                vec![
                    C::text(id),
                    C::text(&binding.principal),
                    C::enumeration(&binding.role)?,
                ],
            )?;
        }
    }
    for placement in &model.placements {
        rows.add(
            "gazette_registry_placements",
            vec![
                C::text(placement.site),
                C::text(placement.project),
                C::enumeration(&placement.role)?,
                C::flag(placement.render),
                C::flag(placement.discoverable),
            ],
        )?;
    }
    for (id, taxonomy) in &model.taxonomies {
        if *id != &taxonomy.id {
            return Err(Error::RegistryVerification);
        }
        rows.add(
            "gazette_registry_taxonomies",
            vec![
                C::text(id),
                C::text(&taxonomy.label),
                C::optional(taxonomy.language.as_ref()),
            ],
        )?;
        for term in &taxonomy.terms {
            rows.add(
                "gazette_registry_terms",
                vec![C::text(id), C::text(&term.id), C::text(&term.label)],
            )?;
        }
    }
    Ok(())
}

fn append_discussion(
    rows: &mut Rows,
    table: &str,
    owner: &Key,
    repository: Option<&str>,
    discussion: &Discussion,
) -> Result<(), Error> {
    if !discussion.is_valid() || repository.is_none_or(str::is_empty) {
        return Err(Error::RegistryVerification);
    }
    rows.add(
        table,
        vec![
            C::text(owner),
            C::text(&discussion.capability),
            C::text(&discussion.repository_id),
            C::text(&discussion.category_id),
            rows.json(&discussion.options)?,
        ],
    )
}
