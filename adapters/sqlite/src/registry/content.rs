//! Existing content values become normalized rows; compiler graph decisions are not repeated.

use super::rows::{Cell as C, Rows};
use gazette_core::{
    publication::content::{ContentKind, PageFamily},
    store::{Error, RegistryInput},
};

pub(super) fn append(input: &RegistryInput<'_>, rows: &mut Rows) -> Result<(), Error> {
    let model = input.model;
    if input.documents.len() != model.content.nodes().len()
        || model.provenance.len() != model.content.nodes().len()
    {
        return Err(Error::RegistryVerification);
    }
    for node in model.content.nodes() {
        let (kind, family, extension, parent, collection) = match &node.structure {
            ContentKind::Page { family } => {
                let (name, extension) = match family {
                    PageFamily::Article => ("article", None),
                    PageFamily::Documentation => ("documentation", None),
                    PageFamily::Project => ("project", None),
                    PageFamily::Index => ("index", None),
                    PageFamily::Utility => ("utility", None),
                    PageFamily::Extension { schema, .. } => ("extension", Some(schema)),
                };
                ("page", Some(name), extension, None, None)
            }
            ContentKind::Collection { parent, .. } => {
                ("collection", None, None, parent.as_ref(), None)
            }
            ContentKind::Journey { collection, .. } => {
                ("journey", None, None, None, collection.as_ref())
            }
        };
        let digest = *input
            .documents
            .get(&node.id)
            .ok_or(Error::RegistryVerification)?;
        let extension_data = match &node.structure {
            ContentKind::Page {
                family: PageFamily::Extension { data, .. },
            } => rows.json(data)?,
            _ => C::Null,
        };
        let collection_purpose = match &node.structure {
            ContentKind::Collection {
                purpose: Some(purpose),
                ..
            } => C::enumeration(purpose)?,
            _ => C::Null,
        };
        let collection_navigation = match &node.structure {
            ContentKind::Collection {
                navigation: Some(items),
                ..
            } => rows.json(items)?,
            _ => C::Null,
        };
        let collection_member_order = match &node.structure {
            ContentKind::Collection { member_order, .. } => rows.json(member_order)?,
            _ => C::Null,
        };
        rows.add(
            "gazette_registry_content",
            vec![
                C::text(&node.id),
                C::text(&node.site),
                C::text(&node.title),
                C::optional(node.description.as_ref()),
                C::optional(node.license.as_ref()),
                C::optional(node.published.as_ref()),
                C::optional(node.modified.as_ref()),
                C::enumeration(&node.lifecycle)?,
                C::optional(node.language.as_ref()),
                C::text(kind),
                C::optional(family),
                C::optional(extension),
                extension_data,
                C::optional(parent),
                C::optional(collection),
                C::digest(digest),
                C::optional(node.route.as_ref().map(|route| route.path.as_str())),
                collection_purpose,
                collection_navigation,
                collection_member_order,
            ],
        )?;
        if let Some(route) = &node.route {
            for (position, alias) in route.aliases.iter().enumerate() {
                rows.add(
                    "gazette_registry_declared_aliases",
                    vec![
                        C::text(&node.id),
                        C::ordinal(position)?,
                        C::text(alias.as_str()),
                    ],
                )?;
            }
        }
        for (position, attribution) in node.attributions.iter().enumerate() {
            rows.add(
                "gazette_registry_attributions",
                vec![
                    C::text(&node.id),
                    C::ordinal(position)?,
                    C::text(&attribution.principal),
                    C::enumeration(&attribution.role)?,
                ],
            )?;
        }
        for assignment in &node.taxonomies {
            rows.add(
                "gazette_registry_classifications",
                vec![
                    C::text(&node.id),
                    C::text(&assignment.taxonomy),
                    C::text(&assignment.term),
                ],
            )?;
        }
        if let ContentKind::Journey { members, .. } = &node.structure {
            for (position, member) in members.iter().enumerate() {
                rows.add(
                    "gazette_registry_journeys",
                    vec![
                        C::text(&node.id),
                        C::ordinal(position)?,
                        C::text(&member.content),
                        C::flag(member.primary),
                    ],
                )?;
            }
        }
        if let Some(links) = model.content.project_relations(&node.id) {
            for (project, link) in links {
                rows.add(
                    "gazette_registry_project_links",
                    vec![
                        C::text(&node.id),
                        C::text(project),
                        C::enumeration(&link.relation())?,
                    ],
                )?;
                for evidence in link.evidence() {
                    rows.add(
                        "gazette_registry_project_evidence",
                        vec![
                            C::text(&node.id),
                            C::text(project),
                            C::text(&evidence.source),
                            C::enumeration(&evidence.kind)?,
                        ],
                    )?;
                }
            }
        }
        let source = model
            .provenance
            .get(&node.id)
            .ok_or(Error::RegistryVerification)?;
        rows.add(
            "gazette_registry_provenance",
            vec![
                C::text(&node.id),
                C::text(source.source),
                C::text(source.entry),
                C::optional(source.project),
            ],
        )?;
    }
    for edge in model.content.memberships() {
        rows.add(
            "gazette_registry_memberships",
            vec![C::text(&edge.content), C::text(&edge.collection)],
        )?;
    }
    for edge in model.content.related() {
        rows.add(
            "gazette_registry_related",
            vec![C::text(&edge.source), C::text(&edge.target)],
        )?;
    }
    for declaration in model.content.declared_projects() {
        rows.add(
            "gazette_registry_project_declarations",
            vec![
                C::text(&declaration.content),
                C::text(&declaration.project),
                C::enumeration(&declaration.kind)?,
            ],
        )?;
    }
    for (id, route) in &model.routes {
        let node = model.content.node(id).ok_or(Error::RegistryVerification)?;
        if route.canonical
            != node
                .route
                .as_ref()
                .and_then(|route| route.canonical.as_ref())
        {
            return Err(Error::RegistryVerification);
        }
        if let Some(original) = route.canonical {
            let target = model
                .routes
                .get(original)
                .ok_or(Error::RegistryVerification)?;
            if original == *id || target.canonical.is_some() || target.url != route.canonical_url {
                return Err(Error::RegistryVerification);
            }
        } else if route.url != route.canonical_url {
            return Err(Error::RegistryVerification);
        }
        rows.add(
            "gazette_registry_routes",
            vec![
                C::text(id),
                C::text(route.url),
                C::optional(route.canonical),
                C::text(route.canonical_url),
                C::text(route.output),
            ],
        )?;
        for alias in &route.aliases {
            rows.add(
                "gazette_registry_aliases",
                vec![C::text(id), C::text(alias.url), C::text(alias.output)],
            )?;
        }
        for mirror in &route.mirrors {
            if mirror.site == &node.site {
                return Err(Error::RegistryVerification);
            }
            rows.add(
                "gazette_registry_mirrors",
                vec![
                    C::text(mirror.site),
                    C::text(mirror.output),
                    C::text(id),
                    C::text(mirror.url),
                ],
            )?;
        }
    }
    Ok(())
}
