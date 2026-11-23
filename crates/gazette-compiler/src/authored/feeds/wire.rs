//! Bounded standards projections of the selected, retained page views.

mod atom;
mod json;
mod rss;
mod xml;

use gazette_core::{publication::content::AttributionRole, render::LimitedWriter};
use serde::Serialize;

use super::{Feed, FeedError};
use crate::{authored::PageProjection, workspace::FeedFormat};

pub(super) fn render(feed: &Feed<'_, '_>, maximum: usize) -> Result<Vec<u8>, FeedError> {
    // Every selected record must have its actual resolved address before a wire
    // writer runs. No format may silently drop an unrouted item.
    route(feed, feed.channel)?;
    for item in &feed.items {
        route(feed, item.page)?;
    }
    match feed.configuration.format {
        FeedFormat::Atom => atom::render(feed, maximum),
        FeedFormat::Rss => rss::render(feed, maximum),
        FeedFormat::Json => json::render(feed, maximum),
    }
}

fn route<'a>(feed: &Feed<'_, '_>, page: &'a PageProjection<'_>) -> Result<&'a str, FeedError> {
    page.data
        .route
        .map(|route| route.url.as_str())
        .ok_or_else(|| feed.missing(page, "serving route"))
}

fn author_names<'a>(page: &'a PageProjection<'_>) -> impl Iterator<Item = &'a str> {
    page.data
        .attributions
        .iter()
        .filter(|role| role.role == AttributionRole::Author)
        .filter_map(|role| role.principal.name.as_deref())
}

fn encode(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>, FeedError> {
    let mut output = LimitedWriter::new(Vec::new(), maximum);
    let result = serde_json::to_writer(&mut output, value);
    if output.exceeded() {
        return Err(FeedError::Limit);
    }
    result.map_err(|error| FeedError::Encoding(error.to_string()))?;
    Ok(output.into_inner())
}
