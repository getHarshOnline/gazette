//! One graph selection over already rendered page projections; no second compilation.

use super::{Discovery, PageProjection, ResolvedAuthored};
use crate::workspace::{FeedConfiguration, FeedResidence, FeedScope, FeedSelectionKind};
use gazette_core::publication::{
    Key,
    content::{ContentKind, PageFamily},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::{Serialize, Serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

mod wire;

/// A complete configured local feed projection, not public admission.
/// Drafts remain included unless an explicit publication selector filters them.
/// Every item borrows the same typed page used by the renderer and other outputs.
#[derive(Debug)]
pub struct Feed<'a, 'data> {
    configuration: &'a FeedConfiguration,
    id: Identity<'a>,
    url: String,
    channel: &'a PageProjection<'data>,
    items: Vec<FeedItem<'a, 'data>>,
    discovery: Option<&'a Discovery<'data>>,
    omit_when_empty: bool,
}

#[derive(Debug)]
struct FeedItem<'a, 'data> {
    id: Identity<'a>,
    page: &'a PageProjection<'data>,
}

// Borrow the namespace once per selection, rather than allocating its full
// prefix for every entry before the bounded wire writer can reject excess.
#[derive(Debug)]
struct Identity<'a> {
    base: &'a str,
    space: &'static str,
    key: &'a Key,
}

impl fmt::Display for Identity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}#{}={}",
            self.base,
            self.space,
            utf8_percent_encode(self.key.as_str(), NON_ALPHANUMERIC)
        )
    }
}

impl Serialize for Identity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// A selected feed cannot be emitted completely from the retained input.
#[derive(Debug)]
pub enum FeedError {
    /// Discovery belongs to another owner or suppresses the explicitly selected channel.
    Discovery,
    /// The feed is not part of the site's resolved brand selection.
    Configuration(Key),
    /// A channel/member is absent, foreign, wrong-kind or not the original graph node.
    Binding {
        /// Exact selected feed.
        feed: Key,
        /// Exact implicated content.
        content: Key,
    },
    /// The selected format requires an explicitly authored field.
    Missing {
        /// Exact selected feed.
        feed: Key,
        /// Exact implicated content.
        content: Key,
        /// Missing or unrepresentable field; no fallback was supplied.
        field: &'static str,
    },
    /// Complete serialization exceeds the available output byte budget.
    Limit,
    /// The complete wire representation is invalid or cannot be serialized.
    Encoding(String),
}

impl FeedError {
    /// Stable failure class, independent of private native paths.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Discovery => "feed_discovery",
            Self::Configuration(_) => "feed_configuration",
            Self::Binding { .. } => "feed_binding",
            Self::Missing { .. } => "feed_field_required",
            Self::Limit => "feed_limit",
            Self::Encoding(_) => "feed_encoding",
        }
    }
}
impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discovery => f.write_str(
                "feed discovery selection is mismatched or its channel is not discoverable",
            ),
            Self::Configuration(id) => {
                write!(f, "feed {id} is not configured for this publication")
            }
            Self::Binding { feed, content } => write!(
                f,
                "feed {feed} cannot bind {content} to its complete same-site retained projection"
            ),
            Self::Missing {
                feed,
                content,
                field,
            } => write!(
                f,
                "feed {feed} requires explicit {field} on {content}; no partial feed produced"
            ),
            Self::Limit => f.write_str("complete feed exceeds output byte limit"),
            Self::Encoding(error) => {
                write!(f, "feed encoding failed: {error}; no partial feed produced")
            }
        }
    }
}
impl std::error::Error for FeedError {}

impl Feed<'_, '_> {
    /// Exact serving URL resolved from the selected publication base and file route.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Existing channel title; feed discovery never invents a format-specific title.
    pub fn title(&self) -> &str {
        &self.channel.data.content.title
    }

    /// Serialize the selected standard format, retaining the shared metadata.
    /// None explicitly omits configured empty output; no placeholder bytes are returned.
    ///
    /// # Errors
    /// Rejects missing format-required facts, invalid XML and complete output excess.
    /// This performs no native reads, staging or public-admission checks.
    pub fn render(&self, maximum: usize) -> Result<Option<Vec<u8>>, FeedError> {
        if self.omit_when_empty && self.items.is_empty() {
            return Ok(None);
        }
        wire::render(self, maximum).map(Some)
    }

    fn missing(&self, page: &PageProjection<'_>, field: &'static str) -> FeedError {
        FeedError::Missing {
            feed: self.configuration.id.clone(),
            content: page.data.content.id.clone(),
            field,
        }
    }
}

struct Metadata<'a, 'data> {
    page: &'a PageProjection<'data>,
    discovery: Option<&'a Discovery<'data>>,
}

impl Serialize for Metadata<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.discovery {
            Some(discovery) => self
                .page
                .listing(discovery)
                .ok_or_else(|| serde::ser::Error::custom("feed metadata lost discovery binding"))?
                .data()
                .serialize(serializer),
            None => self.page.data.serialize(serializer),
        }
    }
}

impl<'data> Feed<'_, 'data> {
    fn metadata<'a>(&'a self, page: &'a PageProjection<'data>) -> Metadata<'a, 'data> {
        Metadata {
            page,
            discovery: self.discovery,
        }
    }
}

impl ResolvedAuthored<'_, '_> {
    /// Select a feed from existing brand configuration and already materialized pages.
    ///
    /// Site/collection output follows canonical URL order; journeys retain declared
    /// traversal. Channel pages are metadata, not repeated entries. Explicit publication
    /// filters preserve authored lifecycle and date precision; no source reads or body
    /// re-rendering occurs. Every scoped candidate binds before any filtering.
    ///
    /// # Errors
    /// Rejects an unconfigured selection, wrong-kind channel, missing/foreign member,
    /// missing route or a page not bound to this exact resolved graph.
    pub fn project_feed<'a, 'data>(
        &'a self,
        site: &Key,
        configuration: &'a FeedConfiguration,
        pages: &'a BTreeMap<Key, PageProjection<'data>>,
    ) -> Result<Feed<'a, 'data>, FeedError> {
        self.project_feed_with_discovery(site, configuration, pages, None)
    }

    /// Select emitted feed items from the original graph and complete page projections.
    /// A suppressed item is not a rewritten page; explicit body links remain authored meaning.
    /// Original selection/order and every selected page binding are validated before filtering.
    ///
    /// # Errors
    /// Rejects mismatched discovery, a suppressed channel and existing feed binding failures.
    pub fn project_discovery_feed<'a, 'data>(
        &'a self,
        site: &Key,
        configuration: &'a FeedConfiguration,
        pages: &'a BTreeMap<Key, PageProjection<'data>>,
        discovery: &'a Discovery<'data>,
    ) -> Result<Feed<'a, 'data>, FeedError> {
        if !discovery.matches(self) || !discovery.contains(&configuration.channel) {
            return Err(FeedError::Discovery);
        }
        self.project_feed_with_discovery(site, configuration, pages, Some(discovery))
    }

    fn project_feed_with_discovery<'a, 'data>(
        &'a self,
        site: &Key,
        configuration: &'a FeedConfiguration,
        pages: &'a BTreeMap<Key, PageProjection<'data>>,
        discovery: Option<&'a Discovery<'data>>,
    ) -> Result<Feed<'a, 'data>, FeedError> {
        let publication = self
            .content()
            .compilation()
            .workspace
            .publication_for(site)
            .ok_or_else(|| FeedError::Configuration(configuration.id.clone()))?;
        let settings = publication
            .machine
            .feeds
            .as_ref()
            .filter(|feeds| feeds.channels.contains(configuration))
            .ok_or_else(|| FeedError::Configuration(configuration.id.clone()))?;
        let base = self
            .content()
            .base_url(site)
            .ok_or_else(|| FeedError::Configuration(configuration.id.clone()))?;
        let graph = self.content().graph();
        let binding = |id: &Key| FeedError::Binding {
            feed: configuration.id.clone(),
            content: id.clone(),
        };
        let page = |id: &Key| -> Result<&'a PageProjection<'data>, FeedError> {
            let value = pages.get(id).ok_or_else(|| binding(id))?;
            let node = graph.node(id).ok_or_else(|| binding(id))?;
            if node.site != *site
                || !std::ptr::eq(node, value.data.content.node)
                || value.data.route.is_none()
                || value.data.route != self.content().route(id)
            {
                return Err(binding(id));
            }
            Ok(value)
        };
        let channel = page(&configuration.channel)?;
        let mut selected = Vec::new();
        let scope = configuration
            .scope
            .or_else(|| settings.selection.as_ref().map(|_| FeedScope::Site {}))
            .ok_or_else(|| FeedError::Configuration(configuration.id.clone()))?;
        match scope {
            FeedScope::Site {} => selected.extend(
                graph
                    .nodes()
                    .filter(|node| node.site == *site)
                    .map(|node| &node.id),
            ),
            FeedScope::Journey {} => {
                let members = graph
                    .journey_members(&configuration.channel)
                    .ok_or_else(|| binding(&configuration.channel))?;
                selected.extend(members.iter().map(|member| &member.content));
            }
            FeedScope::Collection { recursive } => {
                if !matches!(
                    channel.data.content.structure,
                    ContentKind::Collection { .. }
                ) {
                    return Err(binding(&configuration.channel));
                }
                let mut children: BTreeMap<&Key, BTreeSet<&Key>> = BTreeMap::new();
                for node in graph.nodes() {
                    if let Some(parent) = graph.editorial_parent(&node.id) {
                        children.entry(&parent.id).or_default().insert(&node.id);
                    }
                }
                let mut queue = vec![&configuration.channel];
                let mut found = BTreeSet::new();
                while let Some(parent) = queue.pop() {
                    if let Some(members) = children.get(parent) {
                        for id in members {
                            if found.insert(*id)
                                && recursive
                                && graph.node(id).is_some_and(|node| {
                                    matches!(
                                        node.structure,
                                        ContentKind::Collection { .. }
                                            | ContentKind::Journey { .. }
                                    )
                                })
                            {
                                queue.push(id);
                            }
                        }
                    }
                }
                selected.extend(found);
            }
        }
        let identity = |space, key| Identity {
            base: settings.identity_base.as_str(),
            space,
            key,
        };
        let mut items = selected
            .into_iter()
            .filter(|id| *id != &configuration.channel)
            .map(|id| {
                page(id).map(|page| FeedItem {
                    id: identity("content", id),
                    page,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(selection) = &settings.selection {
            items.retain(|item| {
                let node = item.page.data.content.node;
                match (selection.kind, selection.residence) {
                    (FeedSelectionKind::Articles, FeedResidence::All) => {
                        matches!(
                            node.structure,
                            ContentKind::Page {
                                family: PageFamily::Article,
                                ..
                            }
                        ) && !graph.is_documentation(&node.id)
                            && selection.lifecycle.contains(&node.lifecycle)
                            && (!selection.require_published_date || node.published.is_some())
                    }
                }
            });
        }
        if let Some(discovery) = discovery {
            items.retain(|item| discovery.contains(&item.page.data.content.id));
        }
        if scope != (FeedScope::Journey {}) {
            items.sort_by(|a, b| {
                a.page
                    .data
                    .route
                    .map(|r| &r.url)
                    .cmp(&b.page.data.route.map(|r| &r.url))
            });
        }
        Ok(Feed {
            configuration,
            id: identity("feed", &configuration.id),
            url: base.resolve(&configuration.path),
            channel,
            items,
            discovery,
            omit_when_empty: settings.omit_when_empty,
        })
    }
}
