//! Explicit directory browsing never contributes an editorial parent.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{ContentKind, ContentNode, Diagnostic, Key, PageFamily};

/// A page or root collection's authored browsing association, not membership or arrival history.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct BrowsingRelation {
    /// Exact declaring page, which owns this relationship.
    pub source: Key,
    /// Existing same-site routed index; its body remains its own authored content.
    pub index: Key,
}

pub(super) fn validate(
    declarations: Vec<BrowsingRelation>,
    nodes: &BTreeMap<Key, ContentNode>,
    routes_complete: bool,
    errors: &mut Vec<Diagnostic>,
) -> BTreeSet<BrowsingRelation> {
    let mut result = BTreeSet::new();
    for relation in declarations {
        let code = match (nodes.get(&relation.source), nodes.get(&relation.index)) {
            (Some(source), Some(index)) => {
                if !matches!(
                    source.structure,
                    ContentKind::Page { .. } | ContentKind::Collection { parent: None, .. }
                ) {
                    Some("content_browsing_source")
                } else if relation.source == relation.index {
                    Some("content_browsing_self")
                } else if !matches!(
                    index.structure,
                    ContentKind::Page {
                        family: PageFamily::Index
                    }
                ) {
                    Some("content_browsing_index")
                } else if source.site != index.site {
                    Some("content_browsing_site")
                } else if routes_complete && !has_own_route(index) {
                    Some("content_browsing_route")
                } else {
                    None
                }
            }
            _ => Some("content_browsing_reference"),
        };
        if let Some(code) = code {
            errors.push(Diagnostic {
                code,
                source: relation.source.clone(),
                target: Some(relation.index.clone()),
            });
        }
        if !result.insert(relation.clone()) {
            errors.push(Diagnostic {
                code: "content_duplicate_browsing",
                source: relation.source,
                target: Some(relation.index),
            });
        }
    }
    result
}

// Endpoint kinds, site and uniqueness are checked before route completion. Only
// this address requirement waits for the derived paths; browsing adds no parent.
pub(super) fn validate_routes(
    relations: &BTreeSet<BrowsingRelation>,
    nodes: &BTreeMap<Key, ContentNode>,
    errors: &mut Vec<Diagnostic>,
) {
    for relation in relations {
        if nodes
            .get(&relation.index)
            .is_none_or(|index| !has_own_route(index))
        {
            errors.push(Diagnostic {
                code: "content_browsing_route",
                source: relation.source.clone(),
                target: Some(relation.index.clone()),
            });
        }
    }
}

fn has_own_route(index: &ContentNode) -> bool {
    index
        .route
        .as_ref()
        .is_some_and(|route| route.canonical.is_none())
}
