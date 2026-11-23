//! Explicit documentation layouts over native block directives and their parsed children.

use crate::{bounded::Output, source::Lines};
use comrak::nodes::{Node, NodeValue};
use gazette_core::{
    markdown::{Error, Syntax},
    source::Location,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Tabs,
    Tab,
    Steps,
    Step,
    CodeGroup,
    CardGroup,
    Card,
    AccordionGroup,
    Accordion,
    Frame,
}

impl Kind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "tabs" => Self::Tabs,
            "tab" => Self::Tab,
            "steps" => Self::Steps,
            "step" => Self::Step,
            "code-group" => Self::CodeGroup,
            "card-group" => Self::CardGroup,
            "card" => Self::Card,
            "accordion-group" => Self::AccordionGroup,
            "accordion" => Self::Accordion,
            "frame" => Self::Frame,
            _ => return None,
        })
    }

    fn component(self) -> &'static str {
        match self {
            Self::Tabs => "Tabs",
            Self::Tab => "Tab",
            Self::Steps => "Steps",
            Self::Step => "Step",
            Self::CodeGroup => "CodeGroup",
            Self::CardGroup => "CardGroup",
            Self::Card => "Card",
            Self::AccordionGroup => "AccordionGroup",
            Self::Accordion => "Accordion",
            Self::Frame => "Frame",
        }
    }

    fn titled(self) -> bool {
        matches!(self, Self::Tab | Self::Step | Self::Accordion)
    }
}

// These are layout syntax, not qualified HTML elements in the provider. Its
// renderer drops a raw <tab> and its children rather than preserving that HTML.
// HTML tokenization has already normalized the name; this is no JSX recognizer.
pub(super) fn reserved_html_name(name: &str) -> bool {
    Kind::parse(name).is_some() || matches!(name, "codegroup" | "accordiongroup" | "cardgroup")
}

pub(crate) struct Layout<'a> {
    pub(crate) node: Node<'a>,
    kind: Kind,
    pub(crate) title: Option<String>,
    id: Option<String>,
    card_link: Option<Node<'a>>,
}

impl Layout<'_> {
    pub(crate) fn reading_title(&self) -> Option<&str> {
        (self.kind != Kind::Card)
            .then_some(self.title.as_deref())
            .flatten()
    }
}

fn unsupported(location: Location, reason: &'static str) -> Error {
    Error::Unsupported { location, reason }
}

/// Validate the native tree before adding target-owned constructors. Labels are
/// JSON strings, not JSX properties or publication-component argument tables.
pub(crate) fn plan<'a>(
    root: Node<'a>,
    syntax: &Syntax,
    lines: &Lines<'_>,
) -> Result<Vec<Layout<'a>>, Error> {
    let facts: BTreeMap<_, _> = syntax
        .directives
        .iter()
        .map(|fact| (fact.location.span, fact))
        .collect();
    if facts.len() != syntax.directives.len() {
        return Err(Error::InvalidSource);
    }
    let mut selected = Vec::new();
    let mut indexes = BTreeMap::new();
    let mut closing_positions = BTreeSet::new();
    for node in root.descendants() {
        let data = node.data();
        let NodeValue::BlockDirective(directive) = &data.value else {
            continue;
        };
        let location = lines.location(data.sourcepos)?;
        let occurrence = facts
            .get(&location.span)
            .filter(|fact| fact.location == location)
            .ok_or(Error::InvalidSource)?;
        let header = occurrence.info.trim();
        let name_end = header.find(char::is_whitespace).unwrap_or(header.len());
        let kind = Kind::parse(&header[..name_end]).ok_or_else(|| {
            unsupported(occurrence.header, "unsupported documentation layout block")
        })?;
        let arguments = header[name_end..].trim();
        let title = if kind.titled() {
            let label: String = serde_json::from_str(arguments).map_err(|_| {
                unsupported(occurrence.header, "layout title must be one JSON string")
            })?;
            if label.trim().is_empty() || label.chars().any(char::is_control) {
                return Err(unsupported(
                    occurrence.header,
                    "layout title must be nonempty text without control characters",
                ));
            }
            Some(label)
        } else {
            if !arguments.is_empty() {
                return Err(unsupported(
                    occurrence.header,
                    "this documentation layout does not accept arguments",
                ));
            }
            None
        };

        // Comrak permits an EOF-closed directive. Documentation layouts require
        // their own explicit closing fence so a missing delimiter cannot silently
        // place the rest of a manual inside one tab or disclosure.
        let source = lines.slice(location.span.start..location.span.end)?;
        let last_line = source
            .trim_end_matches(['\r', '\n'])
            .rsplit(['\r', '\n'])
            .next()
            .unwrap_or("");
        let last_line = last_line.trim_end_matches([' ', '\t']);
        let fence = last_line.bytes().rev().take_while(|b| *b == b':').count();
        let prefix = &last_line[..last_line.len() - fence];
        if fence < directive.fence_length
            || !prefix.chars().all(|c| matches!(c, ' ' | '\t' | '>'))
            || !closing_positions.insert(location.span.end)
        {
            return Err(unsupported(
                location,
                "documentation layout needs its own closing fence",
            ));
        }
        indexes.insert(location.span, selected.len());
        selected.push(Layout {
            node,
            kind,
            title,
            id: None,
            card_link: None,
        });
    }
    if selected.len() != syntax.directives.len() {
        return Err(Error::InvalidSource);
    }

    for layout in &mut selected {
        if layout.kind != Kind::Card {
            continue;
        }
        let location = lines.location(layout.node.data().sourcepos)?;
        let paragraph = layout
            .node
            .first_child()
            .ok_or_else(|| unsupported(location, "card must begin with one Markdown title link"))?;
        let link = paragraph
            .first_child()
            .filter(|link| {
                matches!(paragraph.data().value, NodeValue::Paragraph)
                    && matches!(link.data().value, NodeValue::Link(_))
                    && link.next_sibling().is_none()
            })
            .ok_or_else(|| unsupported(location, "card must begin with one Markdown title link"))?;
        let text = link
            .first_child()
            .filter(|text| text.next_sibling().is_none())
            .ok_or_else(|| unsupported(location, "card title must be plain text"))?;
        let NodeValue::Text(title) = &text.data().value else {
            return Err(unsupported(location, "card title must be plain text"));
        };
        if title.trim().is_empty() || title.chars().any(char::is_control) {
            return Err(unsupported(
                location,
                "card title must be nonempty plain text",
            ));
        }
        // A Card is itself an anchor. Nested links or layout constructors would
        // create competing targets inside that anchor, so reject them explicitly.
        if layout.node.descendants().any(|child| {
            !std::ptr::eq(child, layout.node)
                && !std::ptr::eq(child, link)
                && matches!(
                    child.data().value,
                    NodeValue::Link(_) | NodeValue::Image(_) | NodeValue::BlockDirective(_)
                )
        }) {
            return Err(unsupported(
                location,
                "card description cannot contain links, images or layout blocks",
            ));
        }
        layout.title = Some(title.to_string());
        layout.card_link = Some(link);
    }

    for layout in &selected {
        let location = lines.location(layout.node.data().sourcepos)?;
        let parent = match layout.node.parent() {
            Some(parent) if matches!(parent.data().value, NodeValue::BlockDirective(_)) => indexes
                .get(&lines.location(parent.data().sourcepos)?.span)
                .map(|index| &selected[*index]),
            _ => None,
        };
        if matches!(layout.kind, Kind::Tab | Kind::Step)
            && parent.map(|parent| parent.kind)
                != Some(if layout.kind == Kind::Tab {
                    Kind::Tabs
                } else {
                    Kind::Steps
                })
        {
            return Err(unsupported(
                location,
                "tab and step blocks require their matching direct container",
            ));
        }
        let expected = match layout.kind {
            Kind::Tabs => Some(Kind::Tab),
            Kind::Steps => Some(Kind::Step),
            Kind::AccordionGroup => Some(Kind::Accordion),
            Kind::CardGroup => Some(Kind::Card),
            _ => None,
        };
        if let Some(expected) = expected {
            let mut titles = BTreeSet::new();
            if layout.node.first_child().is_none() {
                return Err(unsupported(
                    location,
                    "documentation layout group must contain an item",
                ));
            }
            for child in layout.node.children() {
                let child_layout = if matches!(child.data().value, NodeValue::BlockDirective(_)) {
                    indexes
                        .get(&lines.location(child.data().sourcepos)?.span)
                        .map(|index| &selected[*index])
                } else {
                    None
                };
                if child_layout.is_none_or(|child| child.kind != expected)
                    || child_layout
                        .and_then(|child| child.title.as_ref())
                        .is_some_and(|title| !titles.insert(title))
                {
                    return Err(unsupported(
                        location,
                        "layout group requires matching items with distinct titles",
                    ));
                }
            }
        }
        if layout.kind == Kind::CodeGroup {
            let mut labels = BTreeSet::new();
            if layout.node.first_child().is_none() {
                return Err(unsupported(
                    location,
                    "code group must contain a fenced code sample",
                ));
            }
            for child in layout.node.children() {
                let data = child.data();
                let NodeValue::CodeBlock(code) = &data.value else {
                    return Err(unsupported(
                        location,
                        "code group contains non-code content",
                    ));
                };
                let mut parts = code.info.splitn(2, char::is_whitespace);
                let language = parts.next().unwrap_or("");
                let label = parts.next().unwrap_or("").trim();
                if !code.fenced
                    || language.is_empty()
                    || label.is_empty()
                    || !labels.insert(label.to_owned())
                {
                    return Err(unsupported(
                        location,
                        "code group samples need a language and distinct explicit title",
                    ));
                }
            }
        }
    }
    Ok(selected)
}

pub(super) fn wrap<'a>(
    arena: &'a comrak::Arena<'a>,
    mut layouts: Vec<Layout<'a>>,
    mut occupied: BTreeSet<String>,
    limit: usize,
) -> Result<(), Error> {
    // The provider otherwise derives these IDs from labels, independently of
    // heading IDs. A tab named Overview must not steal the Overview heading's
    // anchor. Allocate against the actual bound headings and tokenized HTML IDs.
    let mut ordinal = 0usize;
    for layout in &mut layouts {
        if layout.kind.titled() || layout.kind == Kind::Card {
            layout.id = Some(loop {
                let id = format!("gazette-layout-{ordinal}");
                ordinal = ordinal.checked_add(1).ok_or(Error::Limit("layout IDs"))?;
                if occupied.insert(id.clone()) {
                    break id;
                }
            });
        }
    }
    for layout in layouts.into_iter().rev() {
        let mut opening = Output::new(limit);
        write!(opening, "<{}", layout.kind.component()).map_err(|_| opening.error())?;
        if let Some(link) = layout.card_link {
            let data = link.data();
            let NodeValue::Link(destination) = &data.value else {
                return Err(Error::InvalidSource);
            };
            opening.write_str(" href=\"").map_err(|_| opening.error())?;
            for c in destination.url.chars() {
                // Encode every nonalphanumeric byte as a character reference:
                // the checked native destination remains data in MDX attributes.
                if c.is_ascii_alphanumeric() {
                    opening.write_char(c).map_err(|_| opening.error())?;
                } else {
                    write!(opening, "&#{};", u32::from(c)).map_err(|_| opening.error())?;
                }
            }
            opening.write_char('"').map_err(|_| opening.error())?;
            drop(data);
            link.parent().ok_or(Error::InvalidSource)?.detach();
        }
        if let Some(title) = layout.title {
            opening
                .write_str(" title=\"")
                .map_err(|_| opening.error())?;
            // The CLI normalizes attribute entities, then the provider parses
            // string titles as Markdown. Escape Markdown punctuation before
            // encoding the attribute; a label is plain text at both stages.
            // Keep a string property so native deep-link validation still runs.
            for character in title.chars() {
                if character.is_ascii_punctuation() {
                    write!(opening, "&#92;&#{};", u32::from(character))
                        .map_err(|_| opening.error())?;
                } else {
                    opening.write_char(character).map_err(|_| opening.error())?;
                }
            }
            opening.write_char('"').map_err(|_| opening.error())?;
        }
        if let Some(id) = layout.id {
            write!(opening, " id=\"{id}\"").map_err(|_| opening.error())?;
        }
        if layout.kind == Kind::Accordion {
            // Qualified Mintlify 4.3.8: suppress automatic URL/clipboard writes
            // during native toggle events. Native state, ARIA and expansion stay
            // active; the target's navigation enhancement retains heading URLs.
            opening
                .write_str(" className=\"gazette-layout-disclosure\" _disabled")
                .map_err(|_| opening.error())?;
        }
        opening.write_char('>').map_err(|_| opening.error())?;
        let start = arena.alloc(NodeValue::Paragraph.into());
        start.append(arena.alloc(NodeValue::HtmlInline(opening.value).into()));
        layout.node.insert_before(start);
        while let Some(child) = layout.node.first_child() {
            child.detach();
            layout.node.insert_before(child);
        }
        let end = arena.alloc(NodeValue::Paragraph.into());
        end.append(
            arena.alloc(NodeValue::HtmlInline(format!("</{}>", layout.kind.component())).into()),
        );
        layout.node.insert_after(end);
        layout.node.detach();
    }
    Ok(())
}
