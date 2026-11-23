//! Complete deterministic sitemap packaging over the already selected canonical records.

use super::{SitePages, SitePagesError, bounded};
use gazette_core::{
    publication::routing::RoutePath,
    source::{CapturedSource, Content, EntryPath, InventoryBuilder, Limits, Sha256Digest},
};
use quick_xml::{
    Writer,
    events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event},
};
use std::{
    collections::BTreeMap,
    io::{self, Write},
    ops::Range,
};

const URLS: usize = 50_000;
const BYTES: usize = 52_428_800;

pub(super) fn single(pages: &SitePages<'_>, maximum: usize) -> Result<Vec<u8>, SitePagesError> {
    let urls = urls(pages)?;
    if urls.len() > URLS {
        return Err(SitePagesError::Limit("sitemap URLs"));
    }
    for url in &urls {
        check_url(url)?;
    }
    document("urlset", "url", urls.into_iter(), maximum.min(BYTES))
}

pub(super) fn assemble(
    pages: &SitePages<'_>,
    limits: Limits,
) -> Result<CapturedSource, SitePagesError> {
    let count = pages
        .publication
        .machine
        .sitemap_urls_per_file
        .unwrap_or(URLS);
    if !(1..=URLS).contains(&count) {
        return Err(SitePagesError::Limit("sitemap URLs per file"));
    }
    let maximum = usize::try_from(limits.file_bytes)
        .unwrap_or(usize::MAX)
        .min(BYTES);
    let urls = urls(pages)?;
    let ranges = partition(&urls, count, maximum, limits.entries)?;
    let mut builder = InventoryBuilder::new(limits);
    let mut files = BTreeMap::new();
    let root: EntryPath = "sitemap.xml".parse().map_err(SitePagesError::Output)?;
    if ranges.len() == 1 {
        let bytes = emission(
            &builder,
            &root,
            "urlset",
            "url",
            urls[ranges[0].clone()].iter().copied(),
            maximum,
        )?;
        insert(&mut builder, &mut files, root, bytes)?;
    } else {
        let base = pages
            .owner
            .base_url(&pages.publication.site)
            .ok_or(SitePagesError::BrandRequired)?;
        let paths = (1..=ranges.len())
            .map(|ordinal| {
                let route: RoutePath = format!("/sitemap-{ordinal:05}.xml")
                    .parse()
                    .map_err(|_| SitePagesError::Limit("sitemap partition route"))?;
                let url = base.resolve(&route);
                check_url(&url)?;
                Ok((route.output().clone(), url))
            })
            .collect::<Result<Vec<_>, SitePagesError>>()?;
        let index = emission(
            &builder,
            &root,
            "sitemapindex",
            "sitemap",
            paths.iter().map(|(_, url)| url.as_str()),
            maximum,
        )?;
        insert(&mut builder, &mut files, root, index)?;
        for (range, (path, _)) in ranges.into_iter().zip(paths) {
            let bytes = emission(
                &builder,
                &path,
                "urlset",
                "url",
                urls[range].iter().copied(),
                maximum,
            )?;
            insert(&mut builder, &mut files, path, bytes)?;
        }
    }
    CapturedSource::from_parts(builder.finish(), files).map_err(SitePagesError::Output)
}

fn urls<'a>(pages: &'a SitePages<'_>) -> Result<Vec<&'a str>, SitePagesError> {
    let mut urls: Vec<_> = pages
        .pages
        .iter()
        .filter(|page| {
            page.node
                .route
                .as_ref()
                .is_none_or(|route| route.canonical.is_none())
        })
        .map(|page| page.route.canonical_url.as_str())
        .chain(pages.taxonomies.iter().map(|listing| listing.url.as_str()))
        .chain(pages.listing_pages.iter().map(String::as_str))
        .collect();
    urls.sort_unstable();
    if urls.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SitePagesError::Collision);
    }
    Ok(urls)
}

fn check_url(url: &str) -> Result<(), SitePagesError> {
    if url.chars().count() >= 2048 {
        return Err(SitePagesError::Limit("sitemap URL characters"));
    }
    Ok(())
}

fn partition(
    urls: &[&str],
    count: usize,
    maximum: usize,
    entries: usize,
) -> Result<Vec<Range<usize>>, SitePagesError> {
    // Measure exact framing and each escaped entry with the same writer used for emission.
    let framing = document("urlset", "url", std::iter::empty(), maximum)?.len();
    let mut size = framing;
    let mut start = 0;
    let mut ranges = Vec::new();
    for (index, url) in urls.iter().enumerate() {
        check_url(url)?;
        let entry = bounded(maximum, "sitemap entry bytes", |output| {
            location(&mut Writer::new(output), "url", url)
        })?
        .len();
        if entry > maximum.saturating_sub(framing) {
            return Err(SitePagesError::Limit("sitemap entry bytes"));
        }
        if index - start == count || entry > maximum.saturating_sub(size) {
            ranges.push(start..index);
            if ranges.len() >= URLS || ranges.len() + 2 > entries {
                return Err(SitePagesError::Limit("sitemap file count"));
            }
            start = index;
            size = framing;
        }
        size += entry;
    }
    ranges.push(start..urls.len());
    Ok(ranges)
}

fn emission<'a>(
    builder: &InventoryBuilder,
    path: &EntryPath,
    root: &'static str,
    item: &'static str,
    urls: impl Iterator<Item = &'a str>,
    maximum: usize,
) -> Result<Vec<u8>, SitePagesError> {
    builder.check_path(path).map_err(SitePagesError::Output)?;
    let remaining = usize::try_from(builder.file_ceiling()).unwrap_or(usize::MAX);
    document(root, item, urls, maximum.min(remaining)).map_err(|error| {
        if remaining < maximum && matches!(error, SitePagesError::Limit(_)) {
            SitePagesError::Output(gazette_core::source::Error::Limit {
                path: path.as_str().into(),
                resource: "file or total bytes",
            })
        } else {
            error
        }
    })
}

fn document<'a>(
    root: &'static str,
    item: &'static str,
    urls: impl Iterator<Item = &'a str>,
    maximum: usize,
) -> Result<Vec<u8>, SitePagesError> {
    bounded(maximum, "sitemap bytes", |output| {
        let mut xml = Writer::new(output);
        xml.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        let mut element = BytesStart::new(root);
        element.push_attribute(("xmlns", "http://www.sitemaps.org/schemas/sitemap/0.9"));
        xml.write_event(Event::Start(element))?;
        for url in urls {
            location(&mut xml, item, url)?;
        }
        xml.write_event(Event::End(BytesEnd::new(root)))
    })
}

fn location(xml: &mut Writer<impl Write>, item: &str, url: &str) -> io::Result<()> {
    xml.write_event(Event::Start(BytesStart::new(item)))?;
    xml.write_event(Event::Start(BytesStart::new("loc")))?;
    xml.write_event(Event::Text(BytesText::new(url)))?;
    xml.write_event(Event::End(BytesEnd::new("loc")))?;
    xml.write_event(Event::End(BytesEnd::new(item)))
}

fn insert(
    builder: &mut InventoryBuilder,
    files: &mut BTreeMap<EntryPath, Vec<u8>>,
    path: EntryPath,
    bytes: Vec<u8>,
) -> Result<(), SitePagesError> {
    builder
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(&bytes),
            },
        )
        .map_err(SitePagesError::Output)?;
    files.insert(path, bytes);
    Ok(())
}
