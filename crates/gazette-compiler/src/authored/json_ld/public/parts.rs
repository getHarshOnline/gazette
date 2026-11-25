//! Structural parts, FAQ answers and journey order from the existing page projection.

use super::{Content, Discovery, Sequence};
use crate::authored::{ContentView, PageProjection};
use gazette_core::{
    publication::{Key, content::JourneyMember},
    source::Sha256Digest,
};
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

pub(super) struct Parts<'a, 'data> {
    pub page: &'a PageProjection<'data>,
    pub discovery: Option<&'a Discovery<'data>>,
    pub include_faq: bool,
}
impl Parts<'_, '_> {
    /// No selected child and no FAQ answer: the page names no parts.
    pub(super) fn is_empty(&self) -> bool {
        !self.page.data.children.iter().any(|child| {
            self.discovery
                .is_none_or(|discovery| discovery.node(child.node).is_some())
        }) && !(self.include_faq
            && self
                .page
                .component_uses()
                .any(|component| component.faq.is_some()))
            && !self.page.component_uses().any(|component| {
                component
                    .catalogue
                    .as_ref()
                    .is_some_and(|items| !items.is_empty())
            })
    }
}

impl Serialize for Parts<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for child in &self.page.data.children {
            if self
                .discovery
                .is_none_or(|discovery| discovery.node(child.node).is_some())
            {
                sequence.serialize_element(&Content(
                    child.node,
                    child.resolved_route,
                    child.listing,
                ))?;
            }
        }
        for component in self.page.component_uses().filter(|_| self.include_faq) {
            if let Some(faq) = &component.faq {
                sequence.serialize_element(&Faq(faq, &self.page.data))?;
            }
        }
        for items in self
            .page
            .component_uses()
            .filter_map(|component| component.catalogue.as_ref())
        {
            if !items.is_empty() {
                sequence.serialize_element(&Catalogue(items))?;
            }
        }
        sequence.end()
    }
}

struct Catalogue<'a>(&'a [crate::markdown::CatalogueItem]);

impl Serialize for Catalogue<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Link<'a> {
            url: &'a str,
            #[serde(rename = "@id", skip_serializing_if = "Option::is_none")]
            identity: Option<&'a str>,
            #[serde(rename = "@type", skip_serializing_if = "Option::is_none")]
            kind: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            name: Option<&'a str>,
            #[serde(flatten)]
            software: Option<super::terms::SoftwareFields<'a>>,
        }
        #[derive(Serialize)]
        struct Item<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            position: usize,
            name: &'a str,
            item: Link<'a>,
        }

        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("@type", "ItemList")?;
        map.serialize_entry("numberOfItems", &self.0.len())?;
        map.serialize_entry(
            "itemListElement",
            &Sequence(self.0.iter().enumerate().map(|(index, entry)| {
                Item {
                    kind: "ListItem",
                    position: index + 1,
                    name: &entry.title,
                    item: Link {
                        url: &entry.url,
                        identity: entry.identity.as_deref(),
                        kind: entry.kind.and_then(super::public_identity::reference_type),
                        name: (entry.kind == Some("SoftwareApplication"))
                            .then_some(entry.title.as_str()),
                        software: entry
                            .software
                            .as_ref()
                            .map(super::terms::SoftwareFields::from),
                    },
                }
            })),
        )?;
        map.end()
    }
}

struct Faq<'a, 'data>(
    &'a crate::markdown::Faq,
    &'a crate::authored::PageData<'data>,
);
impl Serialize for Faq<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Answer<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            text: &'a str,
        }
        #[derive(Serialize)]
        struct Question<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            name: &'a str,
            #[serde(rename = "acceptedAnswer")]
            answer: Answer<'a>,
        }
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@type", "FAQPage")?;
        if let Some(address) = &self.0.address {
            map.serialize_entry(
                "@id",
                &format!("_:faq_{}", Sha256Digest::of(address.as_bytes())),
            )?;
        }
        map.serialize_entry(
            "isPartOf",
            &serde_json::json!({"@id":crate::authored::public_identity::work(self.1.content.node, self.1.route, false),"identifier":self.1.content.id}),
        )?;
        map.serialize_entry(
            "mainEntity",
            &Sequence(self.0.questions.iter().map(|item| Question {
                kind: "Question",
                name: &item.question,
                answer: Answer {
                    kind: "Answer",
                    text: &item.answer_html,
                },
            })),
        )?;
        map.end()
    }
}

pub(super) struct Journey<'a>(
    pub &'a [JourneyMember],
    pub &'a [ContentView<'a>],
    pub Option<&'a Discovery<'a>>,
);
impl Serialize for Journey<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Item<'a> {
            #[serde(rename = "@type")]
            kind: &'static str,
            position: usize,
            item: JourneyContent<'a>,
        }
        let selected = self.0.iter().enumerate().filter(|(_, value)| {
            self.2
                .is_none_or(|discovery| discovery.contains(&value.content))
        });
        let mut map = serializer.serialize_map(Some(3))?;
        map.serialize_entry("@type", "ItemList")?;
        map.serialize_entry("numberOfItems", &selected.clone().count())?;
        map.serialize_entry(
            "itemListElement",
            &Sequence(selected.map(|(index, value)| Item {
                kind: "ListItem",
                position: index + 1,
                item: JourneyContent(&value.content, self.1),
            })),
        )?;
        map.end()
    }
}

struct JourneyContent<'a>(&'a Key, &'a [ContentView<'a>]);
impl Serialize for JourneyContent<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let endpoint = self
            .1
            .iter()
            .find(|step| &step.id == self.0)
            .ok_or_else(|| {
                serde::ser::Error::custom("resolved journey member endpoint is missing")
            })?;
        Content(endpoint.node, endpoint.resolved_route, endpoint.listing).serialize(serializer)
    }
}
