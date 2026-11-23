//! Atom 1.0 without inferred authors or timestamp promotion (RFC 4287).

use gazette_core::publication::content::AttributionRole;

use super::{
    Feed, FeedError, route,
    xml::{NAMESPACE, Xml},
};
use crate::authored::PageProjection;

pub(super) fn render(feed: &Feed<'_, '_>, maximum: usize) -> Result<Vec<u8>, FeedError> {
    let mut xml = Xml::new(maximum)?;
    xml.start(
        "feed",
        &[
            ("xmlns", "http://www.w3.org/2005/Atom"),
            ("xmlns:gazette", NAMESPACE),
        ],
    )?;
    xml.element("id", &[], &feed.id.to_string())?;
    xml.element(
        "title",
        &[("type", "text")],
        &feed.channel.data.content.title,
    )?;
    link(&mut xml, "self", &feed.url)?;
    link(&mut xml, "alternate", route(feed, feed.channel)?)?;
    updated(&mut xml, feed, feed.channel)?;
    authors(&mut xml, feed, feed.channel, feed.items.is_empty())?;
    if let Some(description) = &feed.channel.data.content.description {
        xml.element("subtitle", &[("type", "text")], description)?;
    }
    xml.metadata(&feed.metadata(feed.channel))?;
    for item in &feed.items {
        let page = item.page;
        let language = page
            .data
            .content
            .language
            .as_ref()
            .map(|tag| ("xml:lang", tag.as_str()));
        xml.start("entry", language.as_slice())?;
        xml.element("id", &[], &item.id.to_string())?;
        xml.element("title", &[("type", "text")], &page.data.content.title)?;
        link(&mut xml, "alternate", route(feed, page)?)?;
        updated(&mut xml, feed, page)?;
        authors(&mut xml, feed, page, true)?;
        if let Some(date) = &page.data.content.published
            && date.as_timestamp().is_some()
        {
            xml.element("published", &[], &date.to_string())?;
        }
        if let Some(summary) = &page.summary {
            xml.element("summary", &[("type", "text")], &summary.text)?;
        }
        xml.element("content", &[("type", "html")], &page.body_html)?;
        xml.metadata(&feed.metadata(page))?;
        xml.end("entry")?;
    }
    xml.end("feed")?;
    xml.finish()
}

fn link(xml: &mut Xml, relation: &str, url: &str) -> Result<(), FeedError> {
    xml.start("link", &[("rel", relation), ("href", url)])?;
    xml.end("link")
}

fn updated(xml: &mut Xml, feed: &Feed<'_, '_>, page: &PageProjection<'_>) -> Result<(), FeedError> {
    let date = page
        .data
        .content
        .modified
        .as_ref()
        .filter(|date| date.as_timestamp().is_some())
        .ok_or_else(|| feed.missing(page, "modified timestamp"))?;
    xml.element("updated", &[], &date.to_string())
}

fn authors(
    xml: &mut Xml,
    feed: &Feed<'_, '_>,
    page: &PageProjection<'_>,
    required: bool,
) -> Result<(), FeedError> {
    let mut found = false;
    for role in page
        .data
        .attributions
        .iter()
        .filter(|role| role.role == AttributionRole::Author)
    {
        let name = role
            .principal
            .name
            .as_deref()
            .ok_or_else(|| feed.missing(page, "author name"))?;
        found = true;
        xml.start("author", &[])?;
        xml.element("name", &[], name)?;
        xml.end("author")?;
    }
    if required && !found {
        return Err(feed.missing(page, "named author"));
    }
    Ok(())
}
