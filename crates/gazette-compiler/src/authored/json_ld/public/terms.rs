//! Project references and scoped classifications retain their declared meanings.

use super::{Described, IdentityUrl, PrincipalNode, Reference, Sequence};
use crate::authored::{ClassificationView, PageData, ProjectView};
use gazette_core::{publication::ProjectKind, source::Sha256Digest};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

pub(super) struct Project<'a, 'data>(pub(super) &'a ProjectView<'data>, pub(super) &'a Described);
impl Serialize for Project<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", &self.0.identity)?;
        map.serialize_entry("identifier", &self.0.project.id)?;
        map.serialize_entry(
            "@type",
            crate::authored::public_identity::project_type(self.0.project.kind),
        )?;
        if let Some(name) = &self.0.project.name {
            map.serialize_entry("name", name)?;
        }
        if let Some(description) = self.0.description {
            map.serialize_entry("description", description)?;
        }
        if self.0.project.kind == Some(ProjectKind::SoftwareSourceCode)
            && let Some(repository) = &self.0.project.repository
        {
            map.serialize_entry("codeRepository", repository)?;
        }
        let mut parents = self
            .0
            .programme
            .iter()
            .chain(&self.0.portfolio_brands)
            .map(|id| IdentityUrl(id));
        if let Some(first) = parents.next() {
            if parents.clone().next().is_some() {
                map.serialize_entry(
                    "http://purl.org/dc/terms/isPartOf",
                    &Sequence(std::iter::once(first).chain(parents)),
                )?;
            } else {
                map.serialize_entry("http://purl.org/dc/terms/isPartOf", &first)?;
            }
        }
        if let Some(brand) = &self.0.research_brand {
            map.serialize_entry("brand", &IdentityUrl(brand))?;
        }
        if !self.0.programme_members.is_empty() {
            map.serialize_entry(
                "http://purl.org/dc/terms/hasPart",
                &Sequence(
                    self.0
                        .programme_members
                        .iter()
                        .map(|member| IdentityUrl(member)),
                ),
            )?;
        }
        if self.0.project.visibility == gazette_core::publication::Visibility::Public
            && let Some(repository) = &self.0.project.repository
            && (self.0.project.kind == Some(ProjectKind::SoftwareApplication)
                || self.0.project.source.is_some())
            && self.0.project.kind != Some(ProjectKind::SoftwareSourceCode)
        {
            map.serialize_entry(
                "http://purl.org/dc/terms/relation",
                &SoftwareSource {
                    project: self.0,
                    repository,
                    described: self.1,
                },
            )?;
        }
        let external = self
            .0
            .project
            .resources
            .as_ref()
            .and_then(|resources| resources.code_documentation.as_deref())
            .filter(|url| {
                !self.0.works.iter().any(|work| {
                    work.resolved_route
                        .is_some_and(|route| route.public_url == *url)
                })
            });
        if !self.0.works.is_empty() || external.is_some() || !self.0.project.papers.is_empty() {
            map.serialize_entry("subjectOf", &Works(self.0, external))?;
        }
        map.end()
    }
}

struct SoftwareSource<'a, 'data> {
    project: &'a ProjectView<'data>,
    repository: &'a str,
    described: &'a Described,
}

impl Serialize for SoftwareSource<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@id", &self.repository)?;
        map.serialize_entry("@type", "SoftwareSourceCode")?;
        map.serialize_entry("codeRepository", &self.repository)?;
        let relationship = if self.project.project.kind == Some(ProjectKind::SoftwareApplication) {
            "targetProduct"
        } else {
            "about"
        };
        map.serialize_entry(relationship, &IdentityUrl(&self.project.identity))?;
        if let Some(source) = &self.project.project.source {
            map.serialize_entry("license", &IdentityUrl(&source.license_url))?;
            map.serialize_entry("citation", &IdentityUrl(&source.citation_url))?;
        }
        for (property, credit) in [
            ("author", &self.project.source_author),
            ("copyrightHolder", &self.project.source_rights_holder),
        ] {
            if let Some((principal, website)) = credit {
                map.serialize_entry(
                    property,
                    &PrincipalNode(principal, website.as_ref()).within(Some(self.described)),
                )?;
            }
        }
        map.end()
    }
}

struct Works<'a, 'data>(&'a ProjectView<'data>, Option<&'a str>);

#[derive(Serialize)]
struct DocumentationWork<'a> {
    #[serde(rename = "@id")]
    identity: &'a str,
    #[serde(rename = "@type")]
    kind: &'static str,
    url: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<IdentityUrl<'a>>,
}

impl Serialize for Works<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for work in &self.0.works {
            sequence.serialize_element(&super::Content(
                work.node,
                work.resolved_route,
                work.listing,
            ))?;
        }
        for paper in &self.0.project.papers {
            sequence.serialize_element(&DocumentationWork {
                identity: &paper.url,
                kind: "ScholarlyArticle",
                url: &paper.url,
                name: Some(&paper.name),
                about: Some(IdentityUrl(&self.0.identity)),
            })?;
        }
        if let Some(url) = self.1 {
            if let Some(page) = self
                .0
                .code_documentation
                .as_ref()
                .filter(|page| page.public_url == url)
            {
                sequence.serialize_element(&DocumentationWork {
                    identity: url,
                    kind: "WebPage",
                    url,
                    name: page.title.as_deref(),
                    about: None,
                })?;
            } else {
                // The declaration alone establishes help, not the external work's kind or title.
                sequence.serialize_element(&super::super::IdentityUrl(url))?;
            }
        }
        sequence.end()
    }
}

pub(super) struct Keywords<'a, 'data>(pub(super) &'a PageData<'data>);
impl Serialize for Keywords<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(
            self.0.content.keywords.len() + self.0.classifications.len(),
        ))?;
        for keyword in &self.0.content.keywords {
            sequence.serialize_element(keyword)?;
        }
        for classification in &self.0.classifications {
            sequence.serialize_element(&Classification(classification))?;
        }
        sequence.end()
    }
}

struct Classification<'a, 'data>(&'a ClassificationView<'data>);
impl Serialize for Classification<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Taxonomy<'a> {
            #[serde(flatten)]
            identity: Reference<'a>,
            #[serde(rename = "@type")]
            kind: &'static str,
            name: &'a str,
        }
        let value = self.0;
        let mut map = serializer.serialize_map(Some(5))?;
        map.serialize_entry(
            "@id",
            &format!(
                "_:term_{}_{}",
                Sha256Digest::of(value.taxonomy.id.as_str().as_bytes()),
                Sha256Digest::of(value.term.id.as_str().as_bytes())
            ),
        )?;
        map.serialize_entry("@type", "DefinedTerm")?;
        map.serialize_entry("identifier", &value.term.id)?;
        map.serialize_entry("name", &value.term.label)?;
        map.serialize_entry(
            "inDefinedTermSet",
            &Taxonomy {
                identity: Reference("taxonomy", value.taxonomy.id),
                kind: "DefinedTermSet",
                name: value.taxonomy.label,
            },
        )?;
        map.end()
    }
}
