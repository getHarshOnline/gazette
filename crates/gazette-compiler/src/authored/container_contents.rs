//! Shallow placement facts lowered once from the existing navigation projection.

use super::{
    ContentView, Discovery, DocumentationNavigation, DocumentationNavigationEvent, ProjectionError,
};
use gazette_core::{
    markdown::{
        ContainerContentsArtwork, ContainerContentsBinding, ContainerContentsEntry,
        ContainerContentsEvent, ContainerContentsKind, ContainerContentsPresentation,
        ContainerContentsView,
    },
    publication::{
        Key,
        content::{ContentKind, ContentNode},
    },
};

pub(super) struct Container<'a, 'data> {
    pub content: &'a ContentNode,
    pub entries: &'a [ContentView<'data>],
    pub documentation: bool,
    pub navigation: Option<&'a DocumentationNavigation<'data>>,
    pub discovery: Option<&'a Discovery<'data>>,
}

struct Budget(usize);
impl Budget {
    fn charge(&mut self, bytes: usize) -> Result<(), ProjectionError> {
        self.0 = self.0.checked_sub(bytes).ok_or(ProjectionError::Limit(
            "container contents projection bytes",
        ))?;
        Ok(())
    }
    fn text(&mut self, text: &str) -> Result<String, ProjectionError> {
        self.charge(text.len())?;
        Ok(text.to_owned())
    }
    fn key(&mut self, key: &Key) -> Result<Key, ProjectionError> {
        self.charge(key.as_str().len())?;
        Ok(key.clone())
    }
    fn optional(&mut self, text: Option<&str>) -> Result<Option<String>, ProjectionError> {
        text.map(|text| self.text(text)).transpose()
    }
}

pub(super) fn prepare(
    container: Container<'_, '_>,
    reading: Option<&Discovery<'_>>,
    maximum: usize,
) -> Result<ContainerContentsBinding, ProjectionError> {
    let kind = match container.content.structure {
        ContentKind::Collection { .. } => ContainerContentsKind::Collection,
        ContentKind::Journey { .. } => ContainerContentsKind::Journey,
        ContentKind::Page { .. } => {
            return Err(ProjectionError::Binding {
                id: container.content.id.clone(),
                reason: "container contents require a container",
            });
        }
    };
    let mut budget = Budget(maximum);
    let mut entries = Vec::new();
    for entry in container.entries {
        // Direct journey steps remain authored meaning. Collection widgets use
        // the same eligible child selection as DiscoveryPage's Nodes wrapper.
        if kind == ContainerContentsKind::Collection
            && container
                .discovery
                .is_some_and(|discovery| !discovery.contains(&entry.id))
        {
            continue;
        }
        budget.charge(std::mem::size_of::<ContainerContentsEntry>())?;
        entries.push(ContainerContentsEntry {
            id: budget.key(&entry.id)?,
            site: budget.key(&entry.site)?,
            title: budget.text(&entry.title)?,
            description: budget.optional(entry.description.as_deref())?,
            url: budget.optional(entry.resolved_route.map(|route| route.url.as_str()))?,
            presentation: entry
                .presentation
                .as_ref()
                .map(|selected| {
                    Ok::<_, ProjectionError>(ContainerContentsPresentation {
                        theme: budget.key(selected.theme.as_key())?,
                        palette: selected
                            .palette
                            .as_ref()
                            .map(|key| budget.key(key))
                            .transpose()?,
                    })
                })
                .transpose()?,
            artwork: entry
                .artwork
                .as_ref()
                .map(|artwork| {
                    Ok::<_, ProjectionError>(ContainerContentsArtwork {
                        default: budget.text(&artwork.default)?,
                        light: budget.optional(artwork.light.as_deref())?,
                        dark: budget.optional(artwork.dark.as_deref())?,
                    })
                })
                .transpose()?,
        });
    }
    let outline = navigation(container.content, container.navigation, container.discovery)
        .map(|navigation| outline(navigation, &entries, &mut budget))
        .transpose()?;
    let reading_markdown = reading
        .map(|discovery| {
            let bytes =
                super::discovery::bounded(maximum, "container contents Markdown bytes", |output| {
                    super::markdown_alternate::write_container_contents(
                        output,
                        container.content,
                        container.entries,
                        container.navigation,
                        discovery,
                    )
                })
                .map_err(|_| ProjectionError::Limit("container contents Markdown bytes"))?;
            String::from_utf8(bytes).map_err(|_| ProjectionError::Binding {
                id: container.content.id.clone(),
                reason: "generated container Markdown is not UTF-8",
            })
        })
        .transpose()?;
    Ok(ContainerContentsBinding {
        presentation: ContainerContentsView {
            kind,
            documentation: container.documentation,
            entries,
            outline,
        },
        reading_markdown,
    })
}

/// Select the same authored direct-child outline for presentation and reading.
pub(super) fn navigation<'a, 'data>(
    content: &ContentNode,
    navigation: Option<&'a DocumentationNavigation<'data>>,
    discovery: Option<&Discovery<'_>>,
) -> Option<&'a DocumentationNavigation<'data>> {
    navigation.filter(|navigation| {
        matches!(&content.structure, ContentKind::Collection { navigation: Some(items), .. } if !items.is_empty())
            && navigation.current && navigation.root.node.id == content.id
            && discovery.is_none_or(|discovery| discovery.includes_navigation_node(navigation.root.node))
    })
}

fn outline(
    navigation: &DocumentationNavigation<'_>,
    entries: &[ContainerContentsEntry],
    budget: &mut Budget,
) -> Result<Vec<ContainerContentsEvent>, ProjectionError> {
    use DocumentationNavigationEvent as Event;
    let ids: std::collections::BTreeSet<_> = entries.iter().map(|entry| &entry.id).collect();
    let mut result = Vec::new();
    let mut groups = Vec::new();
    let mut depth = 0usize;
    for event in &navigation.events {
        match event {
            Event::PageOpen { destination, .. } => {
                if depth == 0 && ids.contains(&destination.node.id) {
                    budget.charge(std::mem::size_of::<ContainerContentsEvent>())?;
                    result.push(ContainerContentsEvent::Entry {
                        content: budget.key(&destination.node.id)?,
                    });
                }
                depth += 1;
            }
            Event::PageClose { .. } => {
                depth = depth
                    .checked_sub(1)
                    .ok_or(ProjectionError::Limit("container outline depth"))?;
            }
            Event::GroupOpen { title } if depth == 0 => {
                budget.charge(std::mem::size_of::<ContainerContentsEvent>())?;
                groups.push(result.len());
                result.push(ContainerContentsEvent::GroupOpen {
                    title: budget.text(title)?,
                });
            }
            Event::GroupClose if depth == 0 => {
                let opening = groups
                    .pop()
                    .ok_or(ProjectionError::Limit("container outline groups"))?;
                if result.len() == opening + 1 {
                    result.truncate(opening);
                } else {
                    budget.charge(std::mem::size_of::<ContainerContentsEvent>())?;
                    result.push(ContainerContentsEvent::GroupClose);
                }
            }
            _ => {}
        }
    }
    if depth != 0 || !groups.is_empty() {
        return Err(ProjectionError::Limit("container outline balance"));
    }
    Ok(result)
}
