//! Source-relative documents and static references share one confined source coordinate.

use std::borrow::Cow;

use super::{
    AssetPlan, ResolvedAuthored, SitePresentation, StaticAsset, assets::supported_static,
    page_packages::svg_image,
};
use crate::{
    markdown::{
        EcosystemReference, Error, IdentityReference, LinkDestination, LinkForm, LinkKind,
        source_path,
    },
    workspace::ContentSource,
};
use gazette_core::{
    markdown::{LinkDestinationKind, LinkRelationship},
    publication::{
        Key,
        content::{ContentKind, ContentNode},
    },
    source::{EntryPath, Span},
};

pub(super) struct ResolvedReference<'a> {
    pub destination: String,
    pub asset: Option<&'a StaticAsset<'a>>,
    pub content: Option<Key>,
    pub resource: bool,
    pub asset_path: Option<EntryPath>,
}

impl ResolvedAuthored<'_, '_> {
    pub(super) fn resolve_notes_reference<'a>(
        &self,
        id: &Key,
        assets: &'a AssetPlan<'a>,
        form: LinkForm,
        kind: LinkKind,
        destination: &str,
        span: Span,
    ) -> Result<Option<ResolvedReference<'a>>, Error> {
        if form == LinkForm::Markdown && destination.starts_with('#') {
            if kind == LinkKind::Image {
                return Err(Error::LinkResolution {
                    span,
                    reason: "image reference has no retained asset path",
                });
            }
            let body = self
                .page_package(id)
                .and_then(|package| package.notes())
                .and_then(|companion| self.corpus.files().get(companion.path()))
                .and_then(crate::corpus::File::body)
                .ok_or(Error::LinkResolution {
                    span,
                    reason: "notes target has no retained Markdown body",
                })?;
            body.fragment_anchor(destination, span)?;
            return Ok(None);
        }
        // Notes are a sibling of index.md, so the exact source-relative asset
        // coordinate is shared. Other link languages retain their existing owner.
        self.resolve_reference(id, assets, form, kind, destination, span)
    }
    pub(super) fn resolve_reference<'a>(
        &self,
        id: &Key,
        assets: &'a AssetPlan<'a>,
        form: LinkForm,
        kind: LinkKind,
        destination: &str,
        span: Span,
    ) -> Result<Option<ResolvedReference<'a>>, Error> {
        let failure = |reason| Error::LinkResolution { span, reason };
        if form == LinkForm::WikiLink {
            if kind != LinkKind::Link {
                return Err(failure("symbolic publication references cannot be images"));
            }
            return self.resolve_symbolic_reference(destination, span).map(Some);
        }
        let end = destination.find(['?', '#']).unwrap_or(destination.len());
        let path = &destination[..end];
        let suffix = &destination[end..];
        // A fragment on the current document is shared by authored and native
        // content; it does not introduce a publication-relative source grammar.
        if path.is_empty() {
            if kind == LinkKind::Image {
                return Err(failure("image reference has no retained asset path"));
            }
            self.validate_fragment(id, suffix, span)?;
            return Ok(None);
        }
        // Absolute/root-relative destinations have no configured source coordinate.
        // Retaining them does not certify their existence or fetch their contents.
        if source_path::is_external(path) {
            if kind == LinkKind::Link
                && let Some(destination) = self
                    .preview_public_link(destination)
                    .map_err(|_| failure("link mode requires safe paths, an explicit local documentation provider, and no public loopback hyperlinks"))?
            {
                return Ok(Some(ResolvedReference {
                    asset_path: None,
                    destination,
                    asset: None,
                    content: None,
                    resource: false,
                }));
            }
            return Ok(None);
        }
        let Some(origin) = self.source_path(id) else {
            return Err(failure("source document association is missing"));
        };
        let decoded = source_path::decode(path, span)?;
        let Some(filename) = decoded.last() else {
            return Ok(None);
        };
        let static_reference =
            kind == LinkKind::Image || supported_static(filename) || svg_image(filename);
        let document = filename.rsplit_once('.').is_some_and(|(_, extension)| {
            extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
        });
        let source = self.source_binding(id);
        if document
            && !source
                .as_ref()
                .is_ok_and(|source| source.is_documentation())
        {
            return Err(failure(
                "authored content must link to a publication address, not a relative Markdown file",
            ));
        }
        let Ok(source) = source else {
            if static_reference {
                return Err(failure("static reference has no configured source binding"));
            }
            return Ok(None);
        };
        if let Ok(target) = source_target(source, origin, decoded.clone(), span)
            && source.is_retained_only(&target)
        {
            return Err(failure(
                "relative link targets an explicitly retained-only input",
            ));
        }
        let current = decoded.iter().all(|part| matches!(part.as_ref(), "" | "."));
        if source.is_documentation() && kind == LinkKind::Link && current {
            self.validate_fragment(id, suffix, span)?;
            // `./#id` on a file-shaped public route would navigate to its parent,
            // despite validating this body's ID. A suffix-only URL stays on the
            // validated current page for both file and directory routes.
            if suffix
                .split_once('#')
                .is_some_and(|(_, fragment)| !fragment.is_empty())
            {
                return Ok(Some(ResolvedReference {
                    asset_path: None,
                    destination: suffix.to_owned(),
                    asset: None,
                    content: Some(id.clone()),
                    resource: false,
                }));
            }
            return Ok(None);
        }
        // Authored document links retain their separate language; only native
        // documentation adopts the source-relative Markdown-to-route grammar.
        if !(static_reference || source.is_documentation() && document) {
            return Ok(None);
        }
        let target = source_target(source, origin, decoded, span)?;
        if static_reference {
            return self
                .mapped_resource(id, source, assets, &target, suffix, span)
                .map(Some);
        }
        let target_id = self
            .paths
            .iter()
            .find_map(|(id, path)| (path == &target).then_some(id))
            .ok_or_else(|| failure("native document target is absent from the resolved corpus"))?;
        self.source_binding(target_id)
            .map_err(|_| failure("native target source/home binding is invalid"))?;
        self.validate_fragment(target_id, suffix, span)?;
        let route = self
            .content
            .route(target_id)
            .ok_or_else(|| failure("native document target has no resolved public route"))?;
        Ok(Some(ResolvedReference {
            asset_path: None,
            destination: format!("{}{suffix}", route.canonical_url),
            asset: None,
            content: Some(target_id.clone()),
            resource: false,
        }))
    }

    pub(super) fn resolve_notebook_reference<'a>(
        &self,
        owner: (&Key, &EntryPath),
        assets: &'a AssetPlan<'a>,
        form: LinkForm,
        kind: LinkKind,
        destination: &str,
        span: Span,
    ) -> Result<Option<ResolvedReference<'a>>, Error> {
        let (id, origin) = owner;
        if form == LinkForm::WikiLink {
            return self.resolve_reference(id, assets, form, kind, destination, span);
        }
        let end = destination.find(['?', '#']).unwrap_or(destination.len());
        let path = &destination[..end];
        if path.is_empty() {
            return Ok(None);
        }
        if source_path::is_external(path) {
            return self.resolve_reference(id, assets, form, kind, destination, span);
        }
        let decoded = source_path::decode(path, span)?;
        if kind != LinkKind::Image
            && !decoded
                .last()
                .is_some_and(|name| supported_static(name) || svg_image(name))
        {
            return Err(Error::LinkResolution {
                span,
                reason: "local notebook references require an explicit retained asset mapping",
            });
        }
        // A notebook has its own directory, but cannot borrow another capture's
        // assets. Resolve from the owning page's exact configured source binding.
        let source = self.source_binding(id).map_err(|_| Error::LinkResolution {
            span,
            reason: "notebook owner source/home binding is invalid",
        })?;
        let target = source_target(source, origin, decoded, span)?;
        self.mapped_resource(id, source, assets, &target, &destination[end..], span)
            .map(Some)
    }

    fn mapped_resource<'a>(
        &self,
        id: &Key,
        source: ContentSource<'_>,
        assets: &'a AssetPlan<'a>,
        target: &EntryPath,
        suffix: &str,
        span: Span,
    ) -> Result<ResolvedReference<'a>, Error> {
        let mut images = self.resources(id).iter().filter(|resource| {
            (resource.rendition
                || matches!(
                    resource.build,
                    gazette_core::resource::Recipe::SvgImage { .. }
                ))
                && source
                    .logical_path(resource.build.entry())
                    .is_ok_and(|path| &path == target)
        });
        let Some(image) = images.next() else {
            return mapped_asset(assets, target, suffix, span);
        };
        if images.next().is_some() {
            return Err(Error::LinkResolution {
                span,
                reason: "retained image has multiple compiled publication destinations",
            });
        }
        let base = self
            .content
            .base_url(source.site)
            .ok_or(Error::LinkResolution {
                span,
                reason: "compiled image has no resolved site base URL",
            })?;
        Ok(ResolvedReference {
            asset_path: Some(target.clone()),
            destination: format!("{}{suffix}", base.resolve(&image.output)),
            asset: None,
            content: None,
            resource: true,
        })
    }

    fn validate_fragment(&self, target: &Key, suffix: &str, span: Span) -> Result<(), Error> {
        if suffix
            .split_once('#')
            .is_none_or(|(_, part)| part.is_empty())
        {
            return Ok(());
        }
        self.reference_definition(target, suffix, span).map(|_| ())
    }

    pub(super) fn resolve_symbolic_reference<'a>(
        &self,
        destination: &str,
        span: Span,
    ) -> Result<ResolvedReference<'a>, Error> {
        let failure = |reason| Error::LinkResolution { span, reason };
        let (target, fragment) = if destination.starts_with("id:") {
            let selector = IdentityReference::parse(destination, span)?;
            let target = self
                .content
                .graph()
                .node(&selector.content)
                .ok_or_else(|| {
                    failure("symbolic content identity is absent from the resolved corpus")
                })?;
            (target, selector.fragment)
        } else {
            let selector = EcosystemReference::parse(destination, span)?;
            let workspace = &self.content.compilation().workspace;
            let mut sites = workspace.sites().filter(|site| {
                workspace
                    .public_base_url(&site.id)
                    .and_then(|base| url::Url::parse(base.as_str()).ok())
                    .is_some_and(|base| base.host_str() == Some(selector.domain.as_str()))
            });
            let site = sites
                .next()
                .ok_or_else(|| failure("symbolic publication domain is not configured"))?;
            if sites.next().is_some() {
                return Err(failure("symbolic publication domain is ambiguous"));
            }
            // Site-relative declarations, never capture paths or preview hosts, own
            // the target. Explicit aliases normalize to the exact primary serving URL.
            let target = self
                .content
                .graph()
                .nodes()
                .find(|node| {
                    node.site == site.id
                        && node.route.as_ref().is_some_and(|route| {
                            route.path == selector.route || route.aliases.contains(&selector.route)
                        })
                })
                .ok_or_else(|| {
                    failure("symbolic publication route is absent from the resolved corpus")
                })?;
            (target, selector.fragment)
        };
        self.source_binding(&target.id)
            .map_err(|_| failure("symbolic target source/home binding is invalid"))?;
        self.validate_fragment(&target.id, fragment, span)?;
        let route = self
            .content
            .route(&target.id)
            .ok_or_else(|| failure("symbolic publication target has no resolved serving route"))?;
        Ok(ResolvedReference {
            asset_path: None,
            destination: format!("{}{}", route.url, fragment),
            asset: None,
            content: Some(target.id.clone()),
            resource: false,
        })
    }

    pub(super) fn link_destination(
        &self,
        source: &ContentNode,
        original: &str,
        effective: &str,
        resolved: Option<&ResolvedReference<'_>>,
    ) -> LinkDestination {
        if resolved.is_some_and(|value| value.resource) {
            return LinkDestination {
                kind: LinkDestinationKind::Resource,
                content: None,
                site: Some(source.site.clone()),
                relationships: Vec::new(),
                presentation: self
                    .content
                    .presentation_for(&source.site)
                    .map(SitePresentation::from),
            };
        }
        let exact = resolved
            .and_then(|value| value.content.as_ref())
            .and_then(|id| self.content.graph().node(id).map(|node| (node, &node.site)))
            .or_else(|| self.destination_content(effective));
        let (content, site) = match exact {
            Some((node, site)) => (Some(node), Some(site.clone())),
            None => (None, self.destination_site(effective)),
        };
        let site = site.or_else(|| {
            (!source_path::is_external(original) || original.starts_with('/'))
                .then(|| source.site.clone())
        });
        let kind = match &site {
            Some(site) if site == &source.site => LinkDestinationKind::InternalNative,
            Some(_) => LinkDestinationKind::CrossSite,
            None => LinkDestinationKind::External,
        };
        let content_id = content.map(|node| node.id.clone());
        let relationships = content
            .map(|target| self.link_relationships(source, target))
            .unwrap_or_default();
        let presentation = site
            .as_ref()
            .and_then(|site| self.content.presentation_for(site))
            .map(SitePresentation::from);
        LinkDestination {
            kind,
            content: content_id,
            site,
            relationships,
            presentation,
        }
    }

    fn destination_content<'a>(&'a self, destination: &str) -> Option<(&'a ContentNode, &'a Key)> {
        let destination = route_address(destination)?;
        for node in self.content.graph().nodes() {
            let Some(route) = self.content.route(&node.id) else {
                continue;
            };
            // Public authored addresses retain exact native identity under a local
            // preview base; this is a configured route join, not hostname inference.
            let declared = node.route.as_ref();
            let public = self
                .content
                .compilation()
                .workspace
                .public_base_url(&node.site);
            if declared.zip(public).is_some_and(|(declared, base)| {
                base.resolve(&declared.path) == destination
                    || declared
                        .aliases
                        .iter()
                        .any(|alias| base.resolve(alias) == destination)
            }) {
                let target = declared
                    .and_then(|route| route.canonical.as_ref())
                    .and_then(|id| self.content.graph().node(id))
                    .unwrap_or(node);
                return Some((target, &target.site));
            }
            if let Some(mirror) = declared.and_then(|declared| {
                declared.mirrors.iter().find(|mirror| {
                    self.content
                        .compilation()
                        .workspace
                        .public_base_url(&mirror.site)
                        .is_some_and(|base| base.resolve(&mirror.path) == destination)
                })
            }) {
                return Some((node, &mirror.site));
            }
            if route.url == destination || route.canonical_url == destination {
                return Some((node, &node.site));
            }
            if route.aliases.iter().any(|alias| alias.url == destination) {
                return Some((node, &node.site));
            }
            if let Some(mirror) = route
                .mirrors
                .iter()
                .find(|mirror| mirror.url == destination)
            {
                return Some((node, &mirror.site));
            }
        }
        None
    }

    fn destination_site(&self, destination: &str) -> Option<Key> {
        let target = url::Url::parse(destination).ok()?;
        self.content
            .compilation()
            .workspace
            .sites()
            .find(|site| {
                self.content
                    .base_url(&site.id)
                    .into_iter()
                    .chain(
                        self.content
                            .compilation()
                            .workspace
                            .public_base_url(&site.id),
                    )
                    .any(|base| {
                        url::Url::parse(base.as_str())
                            .is_ok_and(|base| base.origin() == target.origin())
                    })
            })
            .map(|site| site.id.clone())
    }

    fn link_relationships(
        &self,
        source: &ContentNode,
        target: &ContentNode,
    ) -> Vec<LinkRelationship> {
        let graph = self.content.graph();
        let mut relationships = Vec::new();
        if graph
            .related()
            .iter()
            .any(|edge| edge.source == source.id && edge.target == target.id)
        {
            relationships.push(LinkRelationship::Related);
        }
        if graph
            .memberships()
            .iter()
            .any(|edge| edge.content == source.id && edge.collection == target.id)
        {
            relationships.push(LinkRelationship::Collection);
        }
        if graph
            .memberships()
            .iter()
            .any(|edge| edge.collection == source.id && edge.content == target.id)
        {
            relationships.push(LinkRelationship::CollectionMember);
        }
        if let ContentKind::Journey { members, .. } = &target.structure
            && members.iter().any(|member| member.content == source.id)
        {
            relationships.push(LinkRelationship::Journey);
        }
        if let ContentKind::Journey { members, .. } = &source.structure
            && members.iter().any(|member| member.content == target.id)
        {
            relationships.push(LinkRelationship::JourneyMember);
        }
        relationships
    }
}

fn mapped_asset<'a>(
    assets: &'a AssetPlan<'a>,
    target: &EntryPath,
    suffix: &str,
    span: Span,
) -> Result<ResolvedReference<'a>, Error> {
    let asset = assets.get(target).ok_or(Error::LinkResolution {
        span,
        reason: "static target is absent or uncompiled in its configured source",
    })?;
    Ok(ResolvedReference {
        asset_path: Some(target.clone()),
        destination: format!("{}{suffix}", asset.url),
        asset: Some(asset),
        content: None,
        resource: true,
    })
}

fn route_address(destination: &str) -> Option<String> {
    let mut url = url::Url::parse(destination).ok()?;
    url.set_query(None);
    url.set_fragment(None);
    Some(String::from(url))
}

pub(super) fn source_target(
    source: ContentSource<'_>,
    origin: &EntryPath,
    decoded: Vec<Cow<'_, str>>,
    span: Span,
) -> Result<EntryPath, Error> {
    let failure = |reason| Error::LinkResolution { span, reason };
    let entry = source
        .entry(origin)
        .ok_or_else(|| failure("source entry is missing"))?;
    let target = source_path::reduce(entry, decoded, span)?;
    let target = target
        .parse()
        .map_err(|_| failure("invalid source target path"))?;
    source
        .logical_path(&target)
        .map_err(|_| failure("invalid source target path"))
}
