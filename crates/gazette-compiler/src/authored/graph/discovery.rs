//! Borrowed discovery serialization over the shared bounded graph traversal.

use std::collections::BTreeMap;

use gazette_core::publication::{
    Key,
    content::{ContentNode, ProjectLink},
};
use serde::{
    Serialize, Serializer,
    ser::{Error, SerializeMap},
};

use super::{Discovery, SiteGraph, link_visible};

/// Emitted graph listing, not the complete canonical structure or private inspection model.
/// Native content and assertions remain borrowed; suppressed assertions are omitted whole.
#[derive(Debug)]
pub struct DiscoveryGraph<'a> {
    pub(super) graph: SiteGraph<'a>,
    pub(super) discovery: &'a Discovery<'a>,
}

impl Serialize for DiscoveryGraph<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let graph = &self.graph;
        let mut map = serializer.serialize_map(None)?;
        if !graph.listings.is_empty() {
            map.serialize_entry("listings", &graph.listings)?;
        }
        map.serialize_entry("format", "gazette-discovery-site-graph/1")?;
        map.serialize_entry("site", graph.site)?;
        map.serialize_entry("publisher", graph.publisher)?;
        if let Some(principal) = graph.rights_holder {
            map.serialize_entry("rights_holder", principal)?;
        }
        if let Some(principal) = graph.default_author {
            map.serialize_entry("default_author", principal)?;
        }
        map.serialize_entry("roles", graph.roles)?;
        map.serialize_entry(
            "nodes",
            &Nodes {
                nodes: &graph.nodes,
                discovery: self.discovery,
            },
        )?;
        map.serialize_entry("references", &graph.references)?;
        map.serialize_entry("routes", &graph.routes)?;
        map.serialize_entry("memberships", &graph.memberships)?;
        map.serialize_entry("related", &graph.related)?;
        if !graph.browsing.is_empty() {
            map.serialize_entry("browsing", &graph.browsing)?;
        }
        map.serialize_entry("containers", &graph.containers)?;
        map.serialize_entry("journeys", &graph.journeys)?;
        map.serialize_entry(
            "project_links",
            &Links {
                links: &graph.project_links,
                discovery: self.discovery,
            },
        )?;
        map.serialize_entry("declarations", &graph.declarations)?;
        map.serialize_entry("projects", &graph.projects)?;
        map.serialize_entry("principals", &graph.principals)?;
        map.serialize_entry("provenance", &graph.provenance)?;
        map.serialize_entry("classifications", &graph.classifications)?;
        map.end()
    }
}

struct Nodes<'a, 'data> {
    nodes: &'a BTreeMap<&'data Key, &'data ContentNode>,
    discovery: &'a Discovery<'data>,
}

impl Serialize for Nodes<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.nodes.len()))?;
        for (key, node) in self.nodes {
            let node = self
                .discovery
                .node(node)
                .ok_or_else(|| S::Error::custom("discovery node binding changed"))?;
            map.serialize_entry(key, &node)?;
        }
        map.end()
    }
}

struct Links<'a, 'data> {
    links: &'a BTreeMap<&'data Key, &'data BTreeMap<Key, ProjectLink>>,
    discovery: &'a Discovery<'data>,
}

impl Serialize for Links<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.links.len()))?;
        for (key, links) in self.links {
            map.serialize_entry(
                key,
                &Assertions {
                    links,
                    discovery: self.discovery,
                },
            )?;
        }
        map.end()
    }
}

struct Assertions<'a, 'data> {
    links: &'a BTreeMap<Key, ProjectLink>,
    discovery: &'a Discovery<'data>,
}

impl Serialize for Assertions<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (key, link) in self.links {
            if link_visible(link, Some(self.discovery)) {
                map.serialize_entry(key, link)?;
            }
        }
        map.end()
    }
}
