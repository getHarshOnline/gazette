//! Bounded Markdown transport of resolved pages and generated reading views.

use std::io::{self, Write};

use gazette_core::publication::content::{AttributionRole, ContentKind, ContentNode};

use super::discovery::{bounded, escaped, plain};
use super::{
    Discovery, ListingPage, MarkdownAlternate, PageData, PageProjection, SitePagesError,
    TaxonomyListing,
};

impl PageProjection<'_> {
    /// Materialize an alternate without frontmatter, private coordinates or another resolver.
    ///
    /// # Errors
    /// Rejects a mismatched serving page, missing selected representation or exhausted bounds.
    pub fn markdown_alternate(
        &self,
        alternate: &MarkdownAlternate,
        discovery: &Discovery<'_>,
        maximum: usize,
    ) -> Result<Vec<u8>, SitePagesError> {
        if !discovery.owns(self.data.content.node) {
            return Err(SitePagesError::Discovery);
        }
        if self
            .data
            .route
            .is_none_or(|route| route.url != alternate.html_url)
        {
            return Err(SitePagesError::Machine(
                "alternate belongs to another serving page",
            ));
        }
        let body = self
            .body_markdown
            .as_deref()
            .ok_or(SitePagesError::Machine("selected Markdown body is missing"))?;
        bounded(maximum, "page Markdown bytes", |output| {
            header(output, &self.data.content.title, &alternate.html_url)?;
            for attribution in &self.data.attributions {
                let Some(name) = &attribution.principal.name else {
                    continue;
                };
                output.write_all(match attribution.role {
                    AttributionRole::Author => b"Author: ",
                    AttributionRole::RightsHolder => b"Rights holder: ",
                })?;
                plain(output, name)?;
                output.write_all(b"\n\n")?;
            }
            output.write_all(body.as_bytes())?;
            if let Some(listing) = &self.data.listing {
                output.write_all(b"\n\n")?;
                listing_body(output, listing)?;
            }
            reading_context(output, &self.data, discovery)?;
            Ok(())
        })
    }
}

/// Borrow the same eligible reading endpoints as the human/machine page model.
/// No member lookup, sequence inference or route construction occurs here.
fn reading_context(
    output: &mut impl Write,
    data: &PageData<'_>,
    discovery: &Discovery<'_>,
) -> io::Result<()> {
    for trail in &data.breadcrumbs {
        if trail.items.len() > 1 && discovery.includes_trail(trail) {
            output.write_all(b"\n\nReading path: ")?;
            for (index, item) in trail.items.iter().enumerate() {
                if index != 0 {
                    output.write_all(b" > ")?;
                }
                endpoint(output, item.node, discovery)?;
            }
            output.write_all(b"\n")?;
        }
    }
    for journey in &data.journeys {
        if discovery.includes_journey(journey) {
            for (label, item) in [
                ("Previous in journey", &journey.previous),
                ("Next in journey", &journey.next),
            ] {
                if let Some(item) = item {
                    write!(output, "\n{label}: ")?;
                    endpoint(output, item.node, discovery)?;
                    output.write_all(b"\n")?;
                }
            }
        }
    }
    if data.container_contents_placed {
        return Ok(());
    }
    write_container_contents(
        output,
        data.content.node,
        if matches!(data.content.structure, ContentKind::Journey { .. }) {
            &data.steps
        } else {
            &data.children
        },
        data.documentation_navigation.as_ref(),
        discovery,
    )
}

pub(super) fn write_container_contents(
    output: &mut impl Write,
    content: &ContentNode,
    nodes: &[super::ContentView<'_>],
    navigation: Option<&super::DocumentationNavigation<'_>>,
    discovery: &Discovery<'_>,
) -> io::Result<()> {
    let (heading, ordered) = match content.structure {
        ContentKind::Collection { .. } => ("Collection contents", false),
        ContentKind::Journey { .. } if discovery.includes_navigation_node(content) => {
            ("Journey sequence", true)
        }
        _ => return Ok(()),
    };
    if let Some(navigation) =
        super::container_contents::navigation(content, navigation, Some(discovery))
    {
        return documentation_contents(output, heading, nodes, navigation, discovery);
    }
    let mut emitted = 0;
    for node in nodes {
        if discovery.includes_navigation_node(node.node) {
            if emitted == 0 {
                write!(output, "\n\n## {heading}\n\n")?;
            }
            emitted += 1;
            if ordered {
                write!(output, "{emitted}. ")?;
            } else {
                output.write_all(b"- ")?;
            }
            endpoint(output, node.node, discovery)?;
            output.write_all(b"\n")?;
        }
    }
    Ok(())
}

/// Keep authored groups and direct-child order; nested page contents remain on their own page.
fn documentation_contents(
    output: &mut impl Write,
    heading: &str,
    nodes: &[super::ContentView<'_>],
    navigation: &super::DocumentationNavigation<'_>,
    discovery: &Discovery<'_>,
) -> io::Result<()> {
    use super::DocumentationNavigationEvent as Event;
    let nodes: std::collections::BTreeMap<_, _> = nodes
        .iter()
        .filter(|node| discovery.includes_navigation_node(node.node))
        .map(|node| (&node.id, node.node))
        .collect();
    let mut groups: Vec<&str> = Vec::new();
    let mut emitted_groups = 0;
    let mut depth = 0usize;
    let mut started = false;
    for event in &navigation.events {
        match event {
            Event::PageOpen { destination, .. } => {
                if depth == 0
                    && let Some(node) = nodes.get(&destination.node.id)
                {
                    if !started {
                        write!(output, "\n\n## {heading}\n\n")?;
                        started = true;
                    }
                    // Delay labels until an eligible direct entry follows, so filtering
                    // cannot leave empty groups or expose their labels alone.
                    for (level, title) in groups.iter().enumerate().skip(emitted_groups) {
                        list_item(output, level)?;
                        plain(output, title)?;
                        output.write_all(b"\n")?;
                    }
                    emitted_groups = groups.len();
                    list_item(output, groups.len())?;
                    endpoint(output, node, discovery)?;
                    output.write_all(b"\n")?;
                }
                depth += 1;
            }
            Event::PageClose { .. } => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| io::Error::other("unbalanced documentation pages"))?;
            }
            Event::GroupOpen { title } if depth == 0 => groups.push(*title),
            Event::GroupClose if depth == 0 => {
                groups
                    .pop()
                    .ok_or_else(|| io::Error::other("unbalanced documentation groups"))?;
                emitted_groups = emitted_groups.min(groups.len());
            }
            _ => {}
        }
    }
    if depth != 0 || !groups.is_empty() {
        return Err(io::Error::other("unbalanced documentation outline"));
    }
    Ok(())
}

fn list_item(output: &mut impl Write, depth: usize) -> io::Result<()> {
    for _ in 0..depth {
        output.write_all(b"  ")?;
    }
    output.write_all(b"- ")
}

fn endpoint(
    output: &mut impl Write,
    node: &ContentNode,
    discovery: &Discovery<'_>,
) -> io::Result<()> {
    if let Some(route) = discovery.navigation_route(node) {
        link(output, &node.title, &route.url)
    } else {
        plain(output, &node.title)
    }
}

impl ListingPage {
    /// Project exactly the same ordered slice and pagination as the HTML view.
    ///
    /// # Errors
    /// Rejects a foreign alternate or complete-output byte exhaustion.
    pub fn markdown_alternate(
        &self,
        alternate: &MarkdownAlternate,
        maximum: usize,
    ) -> Result<Vec<u8>, SitePagesError> {
        if self.url != alternate.html_url {
            return Err(SitePagesError::Machine(
                "alternate belongs to another listing",
            ));
        }
        bounded(maximum, "listing Markdown bytes", |output| {
            header(output, &self.title, &self.url)?;
            listing_body(output, self)
        })
    }
}

impl TaxonomyListing<'_> {
    /// Project existing catalog labels and verified page links, without synthetic identities.
    ///
    /// # Errors
    /// Rejects a foreign alternate or complete-output byte exhaustion.
    pub fn markdown_alternate(
        &self,
        alternate: &MarkdownAlternate,
        maximum: usize,
    ) -> Result<Vec<u8>, SitePagesError> {
        if self.canonical_url != alternate.html_url {
            return Err(SitePagesError::Machine(
                "alternate belongs to another taxonomy view",
            ));
        }
        bounded(maximum, "taxonomy Markdown bytes", |output| {
            header(output, self.title, &self.canonical_url)?;
            for group in &self.groups {
                output.write_all(b"## ")?;
                plain(output, &group.term.label)?;
                output.write_all(b"\n\n")?;
                for page in &group.pages {
                    output.write_all(b"- ")?;
                    link(output, &page.title, &page.canonical_url)?;
                    output.write_all(b"\n")?;
                }
                output.write_all(b"\n")?;
            }
            Ok(())
        })
    }
}

fn header(output: &mut impl Write, title: &str, url: &str) -> io::Result<()> {
    output.write_all(b"Title: ")?;
    plain(output, title)?;
    output.write_all(b"\n\n")?;
    link(output, "Read HTML", url)?;
    output.write_all(b"\n\n")
}

fn listing_body(output: &mut impl Write, page: &ListingPage) -> io::Result<()> {
    writeln!(output, "Page {} of {}\n", page.page_number, page.page_count)?;
    for item in &page.items {
        output.write_all(b"- ")?;
        link(output, &item.title, &item.url)?;
        output.write_all(b"\n")?;
    }
    output.write_all(b"\n")?;
    for (label, url) in [("Previous", &page.previous), ("Next", &page.next)] {
        if let Some(url) = url {
            link(output, label, url)?;
            output.write_all(b"\n")?;
        }
    }
    Ok(())
}

fn link(output: &mut impl Write, title: &str, url: &str) -> io::Result<()> {
    output.write_all(b"[")?;
    plain(output, title)?;
    output.write_all(b"](")?;
    escaped(output, url, |c| matches!(c, '(' | ')' | '\\' | '&'))?;
    output.write_all(b")")
}
