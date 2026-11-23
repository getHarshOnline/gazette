//! Source-bound product article selection, independent from editorial containment.
use super::{
    ListingError, ResolvedAuthored,
    links::ResolvedReference,
    listings::{ListingSubject, ListingSubjectLanding},
};
use crate::workspace::{ListingProjectResidence, ListingProjectScope, ListingSelection};
use gazette_core::{
    markdown::{LinkDestinationKind, LinkKind},
    publication::{
        Key,
        content::{ContentKind, ContentNode, PageFamily, ProjectDeclarationKind},
    },
};

fn failure(id: &Key, reason: &'static str) -> ListingError {
    ListingError::Binding {
        content: id.clone(),
        reason,
    }
}
impl ResolvedAuthored<'_, '_> {
    pub(super) fn listing_subject(
        &self,
        selection: &ListingSelection,
        owner: &ContentNode,
    ) -> Result<Option<ListingSubject>, ListingError> {
        let ListingSelection::ProjectArticles {
            project,
            explore,
            residence,
            ..
        } = selection
        else {
            return Ok(None);
        };
        let graph = self.content().graph();
        let subject = self.content().project(project).ok_or_else(|| {
            failure(
                &owner.id,
                "project article subject is absent from the configured catalog",
            )
        })?;
        let landing = selection
            .project_landing(self.content())
            .map_err(|reason| failure(&owner.id, reason))?
            .ok_or_else(|| failure(&owner.id, "project article landing is absent"))?;
        let (target, route) = self
            .project_landing_target(project, landing)
            .map_err(|reason| failure(&owner.id, reason))?;
        let reference = ResolvedReference {
            asset_path: None,
            destination: route.url.clone(),
            asset: None,
            content: Some(landing.clone()),
            resource: false,
        };
        let explore = explore
            .as_ref()
            .map(|id| {
                let node = graph.node(id).ok_or_else(|| {
                    failure(
                        &owner.id,
                        "exploration index is absent from the captured graph",
                    )
                })?;
                if node.site != owner.site
                    || node
                        .route
                        .as_ref()
                        .is_some_and(|route| route.canonical.is_some())
                    || !matches!(
                        node.structure,
                        ContentKind::Page {
                            family: PageFamily::Index
                        }
                    )
                {
                    return Err(failure(
                        &owner.id,
                        "exploration requires a canonical own-publication index",
                    ));
                }
                let declaration = self
                    .content()
                    .compilation()
                    .workspace
                    .publication_for(&owner.site)
                    .and_then(|publication| {
                        publication
                            .listings
                            .iter()
                            .find(|listing| &listing.content == id)
                    })
                    .ok_or_else(|| {
                        failure(
                            &owner.id,
                            "exploration index requires its configured native listing",
                        )
                    })?;
                if matches!(declaration.select, ListingSelection::ProjectArticles { .. }) {
                    return Err(failure(
                        &owner.id,
                        "exploration requires an unfiltered publication listing",
                    ));
                }
                let route = self
                    .content()
                    .route(id)
                    .ok_or_else(|| failure(&owner.id, "exploration index has no compiled route"))?;
                let reference = ResolvedReference {
                    asset_path: None,
                    destination: route.url.clone(),
                    asset: None,
                    content: Some(id.clone()),
                    resource: false,
                };
                Ok(ListingSubjectLanding {
                    id: id.clone(),
                    title: node.title.clone(),
                    url: route.url.clone(),
                    destination: self.link_destination(
                        owner,
                        &route.url,
                        &route.url,
                        Some(&reference),
                    ),
                })
            })
            .transpose()?;
        Ok(Some(ListingSubject {
            identity: super::public_identity::project(subject, Some(route)),
            kind: subject.kind,
            residence: *residence,
            explore,
            project: project.clone(),
            name: subject.name.clone(),
            landing: ListingSubjectLanding {
                id: landing.clone(),
                title: target.title.clone(),
                url: route.url.clone(),
                destination: self.link_destination(owner, &route.url, &route.url, Some(&reference)),
            },
        }))
    }

    pub(super) fn listing_candidate(
        &self,
        selection: &ListingSelection,
        owner: &ContentNode,
        node: &ContentNode,
        remaining: &mut usize,
    ) -> Result<bool, ListingError> {
        let graph = self.content().graph();
        if node
            .route
            .as_ref()
            .is_some_and(|route| route.canonical.is_some())
        {
            return Ok(false);
        }
        let article = !graph.is_documentation(&node.id)
            && matches!(
                node.structure,
                ContentKind::Page {
                    family: PageFamily::Article
                }
            );
        match selection {
            ListingSelection::AllPages => Ok(node.site == owner.site
                && node.id != owner.id
                && self.content().route(&node.id).is_some()),
            ListingSelection::StandaloneArticles => Ok(node.site == owner.site
                && article
                && graph.editorial_parent(&node.id).is_none()),
            ListingSelection::RootCollections => Ok(node.site == owner.site
                && matches!(node.structure, ContentKind::Collection { .. })
                && graph.editorial_parent(&node.id).is_none()
                && !graph.is_documentation(&node.id)),
            ListingSelection::ProjectArticles {
                project,
                scope,
                residence,
                ..
            } => {
                if !article || (*scope == ListingProjectScope::Site && node.site != owner.site) {
                    return Ok(false);
                }
                let collected = graph.editorial_parent(&node.id).is_some();
                if (*residence == ListingProjectResidence::Standalone && collected)
                    || (*residence == ListingProjectResidence::Collected && !collected)
                {
                    return Ok(false);
                }
                if graph
                    .project_relations(&node.id)
                    .and_then(|relations| relations.get(project))
                    .is_some_and(|relationship| {
                        relationship.evidence().iter().any(|evidence| {
                            evidence.source == node.id
                                && matches!(
                                    evidence.kind,
                                    ProjectDeclarationKind::About
                                        | ProjectDeclarationKind::Mentions
                                )
                        })
                    })
                {
                    return Ok(true);
                }
                let landing = selection
                    .project_landing(self.content())
                    .map_err(|reason| failure(&owner.id, reason))?
                    .ok_or_else(|| failure(&owner.id, "project article landing is absent"))?;
                self.article_references(&node.id, landing, remaining)
            }
        }
    }

    fn article_references(
        &self,
        id: &Key,
        landing: &Key,
        remaining: &mut usize,
    ) -> Result<bool, ListingError> {
        let fail = |error| ListingError::Reference(Box::new(error));
        let source = self
            .content()
            .graph()
            .node(id)
            .ok_or_else(|| failure(id, "article reference owner is absent"))?;
        let assets = self
            .assets_for(&source.site)
            .map_err(|error| fail(super::ProjectionError::Asset(error)))?;
        let path = self
            .source_path(id)
            .ok_or_else(|| failure(id, "article reference path is absent"))?;
        let main = self
            .file(id)
            .ok_or_else(|| failure(id, "article reference source is absent"))?;
        let notes = self
            .page_package(id)
            .and_then(|package| package.notes())
            .and_then(|notes| self.corpus.files().get_key_value(notes.path()));
        for (path, file, is_notes) in
            std::iter::once((path, main, false)).chain(notes.map(|(path, file)| (path, file, true)))
        {
            let Some(body) = file.body() else { continue };
            for link in body
                .source_links()
                .iter()
                .filter(|link| link.emitted && link.kind == LinkKind::Link)
            {
                *remaining = remaining
                    .checked_sub(1)
                    .ok_or(ListingError::Limit("article reference occurrences"))?;
                let reference = if is_notes {
                    self.resolve_notes_reference(
                        id,
                        assets,
                        link.form,
                        link.kind,
                        &link.destination,
                        link.location.span,
                    )
                } else {
                    self.resolve_reference(
                        id,
                        assets,
                        link.form,
                        link.kind,
                        &link.destination,
                        link.location.span,
                    )
                }
                .map_err(|error| fail(super::ProjectionError::body(path, file, error)))?;
                let mapped = reference
                    .as_ref()
                    .map_or(link.destination.as_str(), |reference| {
                        reference.destination.as_str()
                    });
                let effective = if mapped.starts_with('/') {
                    self.content()
                        .route(id)
                        .and_then(|route| url::Url::parse(&route.url).ok())
                        .and_then(|base| base.join(mapped).ok())
                        .map(String::from)
                        .unwrap_or_else(|| mapped.to_owned())
                } else {
                    mapped.to_owned()
                };
                let destination = self.link_destination(
                    source,
                    &link.destination,
                    &effective,
                    reference.as_ref(),
                );
                if destination.kind == LinkDestinationKind::Resource {
                    continue;
                }
                if destination.content.as_ref().is_some_and(|target| {
                    target == landing
                        || self
                            .content()
                            .graph()
                            .node(target)
                            .and_then(|node| node.route.as_ref())
                            .and_then(|route| route.canonical.as_ref())
                            == Some(landing)
                }) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
