//! JSON Feed 1.1 serializers borrowing the same `PageData` and rendered body.

use serde::{
    Serialize, Serializer,
    ser::{SerializeMap, SerializeSeq},
};

use super::{Feed, FeedError, author_names};
use crate::authored::PageProjection;

pub(super) fn render(feed: &Feed<'_, '_>, maximum: usize) -> Result<Vec<u8>, FeedError> {
    super::encode(&Document(feed), maximum)
}

struct Document<'a, 'data>(&'a Feed<'a, 'data>);

impl Serialize for Document<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let feed = self.0;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("version", "https://jsonfeed.org/version/1.1")?;
        map.serialize_entry("title", &feed.channel.data.content.title)?;
        let route = super::route(feed, feed.channel).map_err(serde::ser::Error::custom)?;
        map.serialize_entry("home_page_url", route)?;
        map.serialize_entry("feed_url", &feed.url)?;
        if let Some(description) = &feed.channel.data.content.description {
            map.serialize_entry("description", description)?;
        }
        // Root authors/language imply fallback in this format. Keep channel
        // facts in metadata; standard item facts come only from that item.
        map.serialize_entry("_gazette", &feed.metadata(feed.channel))?;
        #[derive(Serialize)]
        struct FeedIdentity<'a> {
            id: &'a super::super::Identity<'a>,
        }
        map.serialize_entry("_gazette_feed", &FeedIdentity { id: &feed.id })?;
        map.serialize_entry("items", &Items(feed))?;
        map.end()
    }
}

struct Items<'a, 'data>(&'a Feed<'a, 'data>);

impl Serialize for Items<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.items.len()))?;
        for item in &self.0.items {
            sequence.serialize_element(&Item {
                id: &item.id,
                page: item.page,
                feed: self.0,
            })?;
        }
        sequence.end()
    }
}

struct Item<'a, 'data> {
    feed: &'a Feed<'a, 'data>,
    id: &'a super::super::Identity<'a>,
    page: &'a PageProjection<'data>,
}

impl Serialize for Item<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let page = self.page;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", self.id)?;
        let route = page
            .data
            .route
            .ok_or_else(|| serde::ser::Error::custom("missing serving route"))?;
        map.serialize_entry("url", &route.url)?;
        map.serialize_entry("title", &page.data.content.title)?;
        map.serialize_entry("content_html", &page.body_html)?;
        map.serialize_entry("content_text", &page.body_text)?;
        if let Some(summary) = &page.summary {
            map.serialize_entry("summary", &summary.text)?;
        }
        if let Some(language) = &page.data.content.language {
            map.serialize_entry("language", language)?;
        }
        for (name, date) in [
            ("date_published", &page.data.content.published),
            ("date_modified", &page.data.content.modified),
        ] {
            if let Some(date) = date
                && date.as_timestamp().is_some()
            {
                map.serialize_entry(name, date)?;
            }
        }
        if author_names(page).next().is_some() {
            map.serialize_entry("authors", &Authors(page))?;
        }
        map.serialize_entry("_gazette", &self.feed.metadata(page))?;
        map.end()
    }
}

struct Authors<'a, 'data>(&'a PageProjection<'data>);

impl Serialize for Authors<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Author<'a> {
            name: &'a str,
        }
        let mut sequence = serializer.serialize_seq(None)?;
        for name in author_names(self.0) {
            sequence.serialize_element(&Author { name })?;
        }
        sequence.end()
    }
}
