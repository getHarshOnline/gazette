//! Borrowed native ranking values alongside unchanged result metadata.

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

use super::{Discovery, PageProjection};
use crate::workspace::{SearchConfiguration, SearchField};

#[derive(Serialize)]
pub(super) struct IndexedSearch<'a, 'data, D> {
    #[serde(flatten)]
    pub(super) document: D,
    pub(super) indexed: IndexedText<'a, 'data>,
}

pub(super) struct IndexedText<'a, 'data> {
    pub(super) page: &'a PageProjection<'data>,
    pub(super) selection: &'a SearchConfiguration,
    pub(super) discovery: Option<&'a Discovery<'data>>,
}

impl PageProjection<'_> {
    /// Borrow selected plain ranking values alongside the complete native search metadata.
    /// No body or notes parser/resolver is run again, and no additional content ID is created.
    pub fn search_with<'a>(&'a self, selection: &'a SearchConfiguration) -> impl Serialize + 'a {
        IndexedSearch {
            document: self.search(),
            indexed: IndexedText {
                page: self,
                selection,
                discovery: None,
            },
        }
    }
}

impl Serialize for IndexedText<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.selection.fields().len()))?;
        let data = &self.page.data;
        for field in self.selection.fields() {
            match field {
                SearchField::Title => map.serialize_entry("title", &data.content.title)?,
                SearchField::Description => map.serialize_entry(
                    "description",
                    data.content.description.as_deref().unwrap_or(""),
                )?,
                SearchField::Keywords => map.serialize_entry("keywords", &data.content.keywords)?,
                SearchField::Route => {
                    map.serialize_entry("route", data.route.map_or("", |route| route.url.as_str()))?
                }
                SearchField::Body => map.serialize_entry("body", &self.page.body_text)?,
                SearchField::Notes => map.serialize_entry(
                    "notes",
                    data.notes
                        .as_ref()
                        .map_or("", |notes| notes.body_text.as_str()),
                )?,
                SearchField::Summary => map.serialize_entry(
                    "summary",
                    self.page
                        .summary
                        .as_ref()
                        .map_or("", |summary| summary.text.as_str()),
                )?,
                SearchField::Projects => map.serialize_entry("projects", &Projects(self))?,
            }
        }
        map.end()
    }
}

struct Projects<'a, 'data>(&'a IndexedText<'a, 'data>);
impl Serialize for Projects<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for link in &self.0.page.data.projects {
            if self
                .0
                .discovery
                .is_none_or(|discovery| discovery.includes_project(link))
            {
                sequence.serialize_element(link.project.id.as_str())?;
                if let Some(name) = &link.project.name
                    && !name.is_empty()
                {
                    sequence.serialize_element(name)?;
                }
            }
        }
        sequence.end()
    }
}
