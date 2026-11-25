//! Bind the existing content graph to the exact compiled workspace, without policy admission.

use gazette_core::publication::content::{
    ContentKind, Declaration, Diagnostic, Error, Graph, Limits, PageFamily, SchemaType, Subject,
};
use gazette_core::publication::routing::PublicBaseUrl;
use gazette_core::publication::{ContentLifecycle, PrincipalKind, PrincipalWebsite};
use std::collections::BTreeMap;

use super::{Compilation, Key, ResolvedRoute, Site, ThemeSelection};

/// Complete structural content bound to one immutable configuration compilation.
///
/// This borrows the existing site catalog and source identity instead of copying
/// either into content records. It is not an admitted publication, Theme envelope,
/// persistence projection or permission to emit draft/archived content publicly.
#[derive(Debug)]
pub struct ResolvedContent<'a> {
    compilation: &'a Compilation,
    graph: Graph,
    projects: BTreeMap<Key, gazette_core::publication::Project>,
    profiles: BTreeMap<Key, Key>,
    routes: BTreeMap<Key, ResolvedRoute>,
    preview_urls: BTreeMap<Key, PublicBaseUrl>,
}

impl Compilation {
    /// Resolve content structure, home sites and every project/principal reference.
    ///
    /// All lifecycles are checked. Cross-site edges preserve the declared homes;
    /// deployability does not govern structural validity. No source is read and no
    /// site subset is silently removed. Authored-metadata lowering and rights,
    /// privacy, visibility and publication-policy admission are separate phases.
    ///
    /// # Errors
    /// Propagates graph limits and structural failures. After structural resolution,
    /// aggregates unknown home sites, projects and principals in stable diagnostic order,
    /// including unused container context. No partially bound content is returned;
    /// other identity namespaces and project availability never substitute for lookup.
    pub fn resolve_content(
        &self,
        declaration: Declaration,
        limits: Limits,
    ) -> Result<ResolvedContent<'_>, Error> {
        self.resolve_content_with_projects(declaration, BTreeMap::new(), limits)
    }

    pub(crate) fn resolve_content_with_projects(
        &self,
        mut declaration: Declaration,
        authored_projects: BTreeMap<Key, gazette_core::publication::Project>,
        limits: Limits,
    ) -> Result<ResolvedContent<'_>, Error> {
        self.workspace.require_resolved_facts().map_err(|error| {
            Error::Invalid(vec![Diagnostic {
                code: error.code(),
                source: error.target.principal().clone(),
                target: Some(error.binding),
            }])
        })?;
        for node in &mut declaration.nodes {
            if node.language.is_none() {
                node.language = self
                    .workspace
                    .publication_for(&node.site)
                    .and_then(|publication| publication.language.clone());
            }
            if node.lifecycle == ContentLifecycle::Published
                && matches!(
                    node.structure,
                    ContentKind::Page {
                        family: PageFamily::Article
                    }
                )
                && node.attributions.is_empty()
                && let Some(author) = self
                    .workspace
                    .site(&node.site)
                    .and_then(|site| site.default_author.as_ref())
            {
                node.attributions
                    .push(gazette_core::publication::content::Attribution {
                        principal: author.clone(),
                        role: gazette_core::publication::content::AttributionRole::Author,
                    });
            }
        }
        let graph = if self.workspace.sites().any(|site| {
            self.workspace
                .publication_for(&site.id)
                .is_some_and(|publication| publication.routing.is_some())
        }) {
            Graph::resolve_with_routes(declaration, limits, |context| {
                super::derived_routes::derive(&self.workspace, context, limits)
            })?
        } else {
            Graph::resolve(declaration, limits)?
        };
        let mut errors: Vec<_> = graph
            .nodes()
            .filter(|node| self.workspace.site(&node.site).is_none())
            .map(|node| Diagnostic {
                code: "content_site_reference",
                source: node.id.clone(),
                target: Some(node.site.clone()),
            })
            .collect();
        let projects = super::project_catalog::resolve(
            &self.workspace,
            &graph,
            authored_projects,
            &mut errors,
        );
        if projects.len() > limits.nodes {
            return Err(Error::Limit("project catalog nodes"));
        }
        let mut project_bytes =
            gazette_core::render::LimitedWriter::new(std::io::sink(), limits.text_bytes);
        serde_json::to_writer(&mut project_bytes, &projects)
            .map_err(|_| Error::Limit("project catalog bytes"))?;
        errors.extend(
            graph
                .declared_projects()
                .iter()
                .filter(|declaration| !projects.contains_key(&declaration.project))
                .map(|declaration| Diagnostic {
                    code: "content_project_reference",
                    source: declaration.content.clone(),
                    target: Some(declaration.project.clone()),
                }),
        );
        // Template selection belongs to the composing site. Content identity,
        // never a route, title or source filename, binds a retained Theme entry.
        for site in self.workspace.sites() {
            if let Some(bindings) = self
                .workspace
                .composition_for(&site.id)
                .and_then(|c| c.page_templates())
            {
                for id in bindings.value().keys() {
                    let code = match graph.node(id) {
                        None => Some("content_page_template_reference"),
                        Some(node) if node.site != site.id => Some("content_page_template_site"),
                        Some(_) => None,
                    };
                    if let Some(code) = code {
                        errors.push(Diagnostic {
                            code,
                            source: site.id.clone(),
                            target: Some(id.clone()),
                        });
                    }
                }
            }
        }
        errors.extend(graph.nodes().flat_map(|node| {
            node.attributions
                .iter()
                .filter(|attribution| self.workspace.principal(&attribution.principal).is_none())
                .map(|attribution| Diagnostic {
                    code: "content_principal_reference",
                    source: node.id.clone(),
                    target: Some(attribution.principal.clone()),
                })
        }));
        for node in graph.nodes() {
            if let Some(subject) = &node.subject
                && self.workspace.principal(subject.principal()).is_none()
            {
                errors.push(Diagnostic {
                    code: "content_subject_principal_reference",
                    source: node.id.clone(),
                    target: Some(subject.principal().clone()),
                });
            }
            if let gazette_core::publication::content::ContentKind::Page {
                family:
                    gazette_core::publication::content::PageFamily::Extension { schema, data, .. },
            } = &node.structure
                && let Err(code) = self.workspace.extensions.validate(schema, data)
            {
                errors.push(Diagnostic {
                    code,
                    source: node.id.clone(),
                    target: Some(schema.clone()),
                });
            }
            for membership in &node.taxonomies {
                let Some(taxonomy) = self.workspace.taxonomy(&membership.taxonomy) else {
                    errors.push(Diagnostic {
                        code: "content_taxonomy_reference",
                        source: node.id.clone(),
                        target: Some(membership.taxonomy.clone()),
                    });
                    continue;
                };
                if taxonomy.term(&membership.term).is_none() {
                    errors.push(Diagnostic {
                        code: "content_taxonomy_term_reference",
                        source: node.id.clone(),
                        target: Some(membership.term.clone()),
                    });
                }
                if taxonomy.language.is_some() && taxonomy.language != node.language {
                    errors.push(Diagnostic {
                        code: "content_taxonomy_language",
                        source: node.id.clone(),
                        target: Some(membership.taxonomy.clone()),
                    });
                }
            }
        }
        let mut profiles = BTreeMap::new();
        for node in graph.nodes().filter(|node| {
            node.schema_type == Some(SchemaType::ProfilePage)
                && node.lifecycle == ContentLifecycle::Published
        }) {
            let Some(Subject::Principal { principal }) = &node.subject else {
                continue;
            };
            let owns_site = self.workspace.principal(principal).is_some_and(|person| {
                person.kind == PrincipalKind::Person
                    && matches!(&person.website, Some(PrincipalWebsite::Site { site }) if site == &node.site)
            });
            if !owns_site
                || profiles
                    .insert(principal.clone(), node.id.clone())
                    .is_some()
            {
                errors.push(Diagnostic {
                    code: "content_principal_profile",
                    source: node.id.clone(),
                    target: Some(principal.clone()),
                });
            }
        }
        if !errors.is_empty() {
            errors.sort();
            errors.dedup();
            return Err(Error::Invalid(errors));
        }
        let mut resolved = ResolvedContent {
            compilation: self,
            graph,
            projects,
            profiles,
            routes: BTreeMap::new(),
            preview_urls: BTreeMap::new(),
        };
        resolved.routes = super::routes::compile(&resolved, limits)?;
        super::projects::validate_identities(&resolved)?;
        Ok(resolved)
    }
}

impl<'a> ResolvedContent<'a> {
    pub(crate) fn is_listing(
        &self,
        node: &gazette_core::publication::content::ContentNode,
    ) -> bool {
        self.compilation
            .workspace
            .publication_for(&node.site)
            .is_some_and(|publication| {
                publication
                    .listings
                    .iter()
                    .any(|listing| listing.content == node.id)
            })
    }
    /// Complete joined project facts shared by every publication consumer.
    pub fn projects(&self) -> impl ExactSizeIterator<Item = &gazette_core::publication::Project> {
        self.projects.values()
    }

    /// Exact resolved project, including landing-owned facts from this retained corpus.
    pub fn project(&self, id: &Key) -> Option<&gazette_core::publication::Project> {
        self.projects.get(id)
    }

    /// Explicit developer-session bindings; none means original publication URLs.
    /// Consumers must retain this marker rather than certify preview output as release input.
    pub fn preview_urls(&self) -> Option<&BTreeMap<Key, PublicBaseUrl>> {
        (!self.preview_urls.is_empty()).then_some(&self.preview_urls)
    }

    /// Effective serving base; unselected sites keep their immutable configured base.
    /// A preview override changes neither source configuration nor publication identity.
    pub fn base_url(&self, site: &Key) -> Option<&PublicBaseUrl> {
        self.preview_urls
            .get(site)
            .or_else(|| self.compilation.workspace.public_base_url(site))
    }

    pub(crate) fn with_preview_urls(
        mut self,
        urls: BTreeMap<Key, PublicBaseUrl>,
        limits: Limits,
    ) -> Result<Self, Error> {
        if urls.is_empty() || urls.len() > limits.nodes.min(128) {
            return Err(Error::Limit("preview URL sites"));
        }
        let mut bytes = 0usize;
        for (site, base) in &urls {
            bytes = bytes
                .checked_add(site.as_str().len())
                .and_then(|value| value.checked_add(base.as_str().len()))
                .filter(|&value| value <= limits.text_bytes)
                .ok_or(Error::Limit("preview URL bytes"))?;
        }
        let mut errors = Vec::new();
        for (site, base) in &urls {
            let code = if self.compilation.workspace.site(site).is_none() {
                Some("content_preview_site")
            } else if let Some(original) = self.compilation.workspace.public_base_url(site) {
                let valid = url::Url::parse(base.as_str())
                    .ok()
                    .zip(url::Url::parse(original.as_str()).ok())
                    .is_some_and(|(preview, original)| {
                        preview.scheme() == "http"
                            && preview
                                .port_or_known_default()
                                .is_some_and(|port| port != 0)
                            && preview.path() == original.path()
                            && match preview.host() {
                                Some(url::Host::Ipv4(address)) => address.is_loopback(),
                                Some(url::Host::Ipv6(address)) => address.is_loopback(),
                                _ => false,
                            }
                    });
                (!valid).then_some("content_preview_url")
            } else {
                Some("content_preview_base")
            };
            if let Some(code) = code {
                errors.push(Diagnostic {
                    code,
                    source: site.clone(),
                    target: None,
                });
            }
        }
        if !errors.is_empty() {
            return Err(Error::Invalid(errors));
        }
        self.preview_urls = urls;
        self.routes = super::routes::compile(&self, limits)?;
        Ok(self)
    }

    pub(crate) fn into_selected(
        mut self,
        decisions: &BTreeMap<Key, gazette_core::publication::content::Disposition>,
        limits: Limits,
    ) -> Result<Self, Error> {
        self.graph = self.graph.into_selected(decisions, limits)?;
        self.routes.retain(|id, _| self.graph.node(id).is_some());
        Ok(self)
    }

    /// Shared compiled address for an explicitly routed object. None means unknown
    /// or unrouted, never an inferred filename/title route or publication approval.
    pub fn route(&self, content: &Key) -> Option<&ResolvedRoute> {
        self.routes.get(content)
    }

    pub(crate) fn profile_route(&self, principal: &Key) -> Option<(&Key, &ResolvedRoute)> {
        let profile = self.profiles.get(principal)?;
        Some((profile, self.route(profile)?))
    }

    /// Exact compilation, including its source digest; no reconstructed configuration.
    pub fn compilation(&self) -> &'a Compilation {
        self.compilation
    }

    /// Shared immutable structure; renderers and stores need no independent resolver.
    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// Resolve a content object's home site; only an unknown content key returns none.
    ///
    /// The returned site is borrowed from the same catalog validated at construction.
    /// Its publisher role is configuration, not content authorship or authorization.
    pub fn site_for(&self, content: &Key) -> Option<&'a Site> {
        self.compilation
            .workspace
            .site(&self.graph.node(content)?.site)
    }

    /// Effective destination presentation for an exact configured site.
    ///
    /// Absence means no Theme was selected. This does not expose selection origin,
    /// provider/plugin state or Theme capture/readiness as destination metadata.
    pub fn presentation_for(&self, site: &Key) -> Option<&'a ThemeSelection> {
        self.compilation
            .workspace
            .composition_for(site)?
            .theme()
            .map(|selection| selection.value())
    }
}
