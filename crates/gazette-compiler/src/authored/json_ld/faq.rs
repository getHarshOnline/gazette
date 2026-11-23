//! Keep FAQ sections alongside existing structural parts, without changing the
//! surrounding page type, navigation or listing's main entity.

use gazette_core::source::Sha256Digest;
use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

use super::{Content, Reference};
use crate::{
    authored::{Discovery, PageProjection},
    markdown::{Faq, FaqQuestion},
};

pub(super) struct Parts<'a, 'data> {
    pub page: &'a PageProjection<'data>,
    pub discovery: Option<&'a Discovery<'data>>,
    pub include_faq: bool,
}
impl Serialize for Parts<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for child in &self.page.data.children {
            if self
                .discovery
                .is_none_or(|discovery| discovery.node(child.node).is_some())
            {
                sequence.serialize_element(&Content(child.node, child.resolved_route))?;
            }
        }
        for component in self.page.component_uses().filter(|_| self.include_faq) {
            if let Some(faq) = &component.faq {
                sequence.serialize_element(&Group {
                    faq,
                    parent: &self.page.data.content.id,
                })?;
            }
        }
        sequence.end()
    }
}
struct Group<'a> {
    faq: &'a Faq,
    parent: &'a gazette_core::publication::Key,
}
impl Serialize for Group<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("@type", "FAQPage")?;
        if let Some(address) = &self.faq.address {
            map.serialize_entry(
                "@id",
                &format!("_:faq_{}", Sha256Digest::of(address.as_bytes())),
            )?;
        }
        map.serialize_entry("isPartOf", &Reference("content", self.parent))?;
        map.serialize_entry("mainEntity", &Questions(&self.faq.questions))?;
        map.end()
    }
}
struct Questions<'a>(&'a [FaqQuestion]);
impl Serialize for Questions<'_> {
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
        serializer.collect_seq(self.0.iter().map(|item| Question {
            kind: "Question",
            name: &item.question,
            answer: Answer {
                kind: "Answer",
                text: &item.answer_html,
            },
        }))
    }
}
