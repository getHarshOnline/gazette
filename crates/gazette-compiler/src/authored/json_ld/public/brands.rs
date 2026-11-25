//! Public brand nodes keep company identity, programme and portfolio connections distinct.

use super::{IdentityUrl, PrincipalNode, Sequence};
use crate::authored::brand_graph::{BrandGraph, BrandNode, ProjectNode};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

pub(super) fn append<S: SerializeSeq>(
    sequence: &mut S,
    graph: &BrandGraph<'_>,
) -> Result<(), S::Error> {
    sequence.serialize_element(&Brand(&graph.current))?;
    for sibling in &graph.siblings {
        sequence.serialize_element(&Brand(sibling))?;
    }
    if let Some(principal) = owner(graph) {
        sequence.serialize_element(&Owner(graph, principal))?;
    }
    Ok(())
}

pub(super) struct Brand<'a, 'data>(pub &'a BrandNode<'data>);

impl Serialize for Brand<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let node = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", &node.identity)?;
        map.serialize_entry("@type", "Brand")?;
        map.serialize_entry("identifier", &node.brand.id)?;
        map.serialize_entry("name", &node.brand.name)?;
        if let Some(description) = &node.brand.description {
            map.serialize_entry("description", description)?;
        }
        if let Some(home) = node.home {
            map.serialize_entry("url", home)?;
            map.serialize_entry("subjectOf", &IdentityUrl(&format!("{home}#website")))?;
        }
        if let Some(parent) = &node.parent {
            map.serialize_entry("http://purl.org/dc/terms/isPartOf", &IdentityUrl(parent))?;
        }
        if let Some(subject) = &node.subject {
            map.serialize_entry("http://purl.org/dc/terms/relation", &Project(subject))?;
        }
        if !node.children.is_empty() || !node.projects.is_empty() {
            map.serialize_entry("http://purl.org/dc/terms/hasPart", &Parts(node))?;
        }
        map.end()
    }
}

struct Parts<'a, 'data>(&'a BrandNode<'data>);

impl Serialize for Parts<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence =
            serializer.serialize_seq(Some(self.0.children.len() + self.0.projects.len()))?;
        for child in &self.0.children {
            sequence.serialize_element(&IdentityUrl(child))?;
        }
        for project in &self.0.projects {
            sequence.serialize_element(&Project(project))?;
        }
        sequence.end()
    }
}

struct Project<'a, 'data>(&'a ProjectNode<'data>);

impl Serialize for Project<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        super::terms::ProjectIdentity::for_project(&self.0.identity, self.0.project)
            .serialize(serializer)
    }
}

pub(super) fn owner<'a>(graph: &'a BrandGraph<'_>) -> Option<PrincipalNode<'a>> {
    graph
        .owner
        .as_ref()
        .map(|(principal, website)| PrincipalNode(principal, website.as_ref()))
}

struct Owner<'a, 'data>(&'a BrandGraph<'data>, PrincipalNode<'a>);

impl Serialize for Owner<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        self.1.write_fields(&mut map)?;
        map.serialize_entry(
            "brand",
            &Sequence(
                std::iter::once(&self.0.current)
                    .chain(&self.0.siblings)
                    .map(|brand| IdentityUrl(&brand.identity)),
            ),
        )?;
        map.end()
    }
}
