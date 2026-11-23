//! RSS 2.0: names are Dublin Core creators, not fabricated email authors.

use gazette_core::publication::content::EditorialDate;

use super::{
    Feed, FeedError, author_names, route,
    xml::{NAMESPACE, Xml},
};

pub(super) fn render(feed: &Feed<'_, '_>, maximum: usize) -> Result<Vec<u8>, FeedError> {
    let channel = feed.channel;
    let description = channel
        .data
        .content
        .description
        .as_deref()
        .ok_or_else(|| feed.missing(channel, "description"))?;
    let mut xml = Xml::new(maximum)?;
    xml.start(
        "rss",
        &[
            ("version", "2.0"),
            ("xmlns:gazette", NAMESPACE),
            ("xmlns:dc", "http://purl.org/dc/elements/1.1/"),
            ("xmlns:atom", "http://www.w3.org/2005/Atom"),
        ],
    )?;
    xml.start("channel", &[])?;
    xml.element("gazette:id", &[], &feed.id.to_string())?;
    xml.element("title", &[], &channel.data.content.title)?;
    xml.element("link", &[], route(feed, channel)?)?;
    xml.element("description", &[], description)?;
    xml.start(
        "atom:link",
        &[
            ("rel", "self"),
            ("type", "application/rss+xml"),
            ("href", &feed.url),
        ],
    )?;
    xml.end("atom:link")?;
    date(
        &mut xml,
        "lastBuildDate",
        channel.data.content.modified.as_ref(),
    )?;
    for name in author_names(channel) {
        xml.element("dc:creator", &[], name)?;
    }
    xml.metadata(&feed.metadata(channel))?;
    for item in &feed.items {
        let page = item.page;
        xml.start("item", &[])?;
        xml.element("guid", &[("isPermaLink", "false")], &item.id.to_string())?;
        xml.element("title", &[], &page.data.content.title)?;
        xml.element("link", &[], route(feed, page)?)?;
        xml.element("description", &[], &page.body_html)?;
        for name in author_names(page) {
            xml.element("dc:creator", &[], name)?;
        }
        date(&mut xml, "pubDate", page.data.content.published.as_ref())?;
        xml.metadata(&feed.metadata(page))?;
        xml.end("item")?;
    }
    xml.end("channel")?;
    xml.end("rss")?;
    xml.finish()
}

fn date(xml: &mut Xml, name: &str, date: Option<&EditorialDate>) -> Result<(), FeedError> {
    if let Some(timestamp) = date.and_then(EditorialDate::as_timestamp)
        && timestamp.timestamp_subsec_nanos() == 0
    {
        // RSS's RFC 822 wire date has whole seconds. Never round; the original
        // authored value (including date-only/fractions) stays in metadata.
        xml.element(
            name,
            &[],
            &timestamp.format("%a, %d %b %Y %H:%M:%S %z").to_string(),
        )?;
    }
    Ok(())
}
