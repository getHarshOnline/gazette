//! Publication address rules applied only after editorial containment is valid.
use gazette_core::publication::{
    Key,
    content::{ContentKind, ContentNode, Diagnostic, Error, Limits, PageFamily, RoutingContext},
    routing::RoutePath,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Optional root prefixes; children follow their immediate editorial parent's route.
/// Enabling these rules requires a slug for each unrouted non-home node and a
/// selected prefix for each root kind. Missing rules never guess an address.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PublicationRouting {
    /// Standalone articles, commonly `/blog/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub articles: Option<RoutePath>,
    /// Root editorial collections, commonly `/collections/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collections: Option<RoutePath>,
    /// Project pages and root documentation collections, commonly `/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub projects: Option<RoutePath>,
    /// Other standalone pages, commonly `/`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<RoutePath>,
}

impl PublicationRouting {
    pub(super) fn prefixes(&self) -> [(&'static str, Option<&RoutePath>); 4] {
        [
            ("articles", self.articles.as_ref()),
            ("collections", self.collections.as_ref()),
            ("projects", self.projects.as_ref()),
            ("pages", self.pages.as_ref()),
        ]
    }

    fn root<'a>(
        &'a self,
        node: &ContentNode,
        context: RoutingContext<'_>,
    ) -> Option<&'a RoutePath> {
        match &node.structure {
            ContentKind::Collection { .. } if context.is_documentation(&node.id) => {
                self.projects.as_ref()
            }
            ContentKind::Collection { .. } => self.collections.as_ref(),
            ContentKind::Page {
                family: PageFamily::Article,
            } => self.articles.as_ref(),
            ContentKind::Page {
                family: PageFamily::Project,
            } => self.projects.as_ref(),
            ContentKind::Page { .. } => self.pages.as_ref(),
            ContentKind::Journey { .. } => None,
        }
    }
}

pub(super) fn derive(
    workspace: &super::Workspace,
    context: RoutingContext<'_>,
    limits: Limits,
) -> Result<BTreeMap<Key, RoutePath>, Error> {
    let mut paths = BTreeMap::<Key, RoutePath>::new();
    let mut done = BTreeSet::new();
    let mut errors = Vec::new();
    let mut bytes = 0usize;
    // Each node enters one trail. The graph has already rejected cycles, multiple
    // homes and cross-site containment; no folder walk or depth cutoff is needed.
    for start in context.nodes() {
        let mut trail = Vec::new();
        let mut cursor = Some(start);
        while let Some(node) = cursor {
            if done.contains(&node.id) {
                break;
            }
            trail.push(node);
            cursor = context.editorial_parent(&node.id);
        }
        for node in trail.into_iter().rev() {
            done.insert(node.id.clone());
            if node.route.is_some() {
                continue;
            }
            let Some(publication) = workspace.publication_for(&node.site) else {
                continue;
            };
            let Some(rules) = &publication.routing else {
                continue;
            };
            let result = if publication.home.as_ref() == Some(&node.id) {
                charge(&mut bytes, 12, limits)?;
                Ok("/".parse().expect("root route"))
            } else if let Some(slug) = &node.slug {
                let base = if let Some(parent) = context.editorial_parent(&node.id) {
                    parent
                        .route
                        .as_ref()
                        .map(|route| &route.path)
                        .or_else(|| paths.get(&parent.id))
                        .ok_or(("content_route_parent", Some(parent.id.clone())))
                } else {
                    rules
                        .root(node, context)
                        .ok_or(("content_route_rule", None))
                };
                match base {
                    Ok(base) => {
                        // Bound both encoded URL and decoded output before allocation.
                        let size = base
                            .as_str()
                            .len()
                            .checked_add(slug.as_str().len())
                            .and_then(|n| n.checked_add(12))
                            .and_then(|n| n.checked_mul(2))
                            .ok_or(Error::Limit("derived route text bytes"))?;
                        charge(&mut bytes, size, limits)?;
                        base.join_slug(slug).map_err(|_| {
                            (
                                "content_route_parent_directory",
                                context
                                    .editorial_parent(&node.id)
                                    .map(|parent| parent.id.clone()),
                            )
                        })
                    }
                    Err(error) => Err(error),
                }
            } else {
                Err(("content_route_slug", None))
            };
            match result {
                Ok(path) => {
                    paths.insert(node.id.clone(), path);
                }
                Err((code, target)) => errors.push(Diagnostic {
                    code,
                    source: node.id.clone(),
                    target,
                }),
            }
        }
    }
    if errors.is_empty() {
        Ok(paths)
    } else {
        errors.sort();
        errors.dedup();
        Err(Error::Invalid(errors))
    }
}

fn charge(bytes: &mut usize, amount: usize, limits: Limits) -> Result<(), Error> {
    *bytes = bytes
        .checked_add(amount)
        .filter(|n| *n <= limits.text_bytes)
        .ok_or(Error::Limit("derived route text bytes"))?;
    Ok(())
}
