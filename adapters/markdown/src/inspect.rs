//! Whole-source facts and emitted text share parser structure, not HTML reconstruction.

use crate::{
    bounded::{Budget, Output},
    source::Lines,
};
use comrak::{Node, arena_tree::NodeEdge, nodes::NodeValue};
use gazette_core::markdown::{
    Directive, Error, Footnote, FootnoteReference, Heading, Html, Identity, Limits, Link, LinkForm,
    LinkKind, Math, Syntax, Text,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write,
};

pub(crate) fn check_tree(root: Node<'_>, limits: Limits) -> Result<usize, Error> {
    let mut nodes = 0usize;
    let mut depth = 0usize;
    for edge in root.traverse() {
        match edge {
            NodeEdge::Start(_) => {
                nodes = nodes.checked_add(1).ok_or(Error::Limit("syntax nodes"))?;
                if nodes > limits.nodes {
                    return Err(Error::Limit("syntax nodes"));
                }
                if depth > limits.depth {
                    return Err(Error::Limit("syntax depth"));
                }
                depth += 1;
            }
            NodeEdge::End(_) => depth -= 1,
        }
    }
    Ok(nodes)
}

pub(crate) fn emitted(node: Node<'_>) -> bool {
    !node.ancestors().skip(1).any(|parent| {
        matches!(
            &parent.data().value,
            NodeValue::Image(_)
                | NodeValue::FootnoteDefinition(comrak::nodes::NodeFootnoteDefinition {
                    total_references: 0,
                    ..
                })
        )
    })
}

// Comrak's math_code selects inline code-math; a native fenced `math` block
// remains a CodeBlock. Gazette's explicit math profile admits both through the
// same source fact, without reparsing delimiters or rewriting the syntax tree.
pub(crate) fn formula(
    value: &NodeValue,
    profile: gazette_core::markdown::Profile,
) -> Option<(&str, bool)> {
    match value {
        NodeValue::Math(math) => Some((&math.literal, math.display_math)),
        NodeValue::CodeBlock(code)
            if profile == gazette_core::markdown::Profile::PublicationMath
                && code.fenced
                && code.info == "math" =>
        {
            Some((&code.literal, true))
        }
        _ => None,
    }
}

pub(crate) fn analyze(
    root: Node<'_>,
    source: &str,
    identity: Identity,
    limits: Limits,
) -> Result<Syntax, Error> {
    let node_count = check_tree(root, limits)?;
    let lines = Lines::new(source);
    let mut budget = Budget(limits.fact_bytes);
    let mut syntax = Syntax {
        identity,
        node_count,
        headings: vec![],
        links: vec![],
        html: vec![],
        footnotes: vec![],
        footnote_references: vec![],
        math: vec![],
        directives: vec![],
        text: vec![],
    };
    // Qualify definitions before collecting any fact whose emission depends on
    // an ancestor definition. Parser counts include image-label and orphan
    // references; Gazette's closure is the single source used below and later
    // by the renderer.
    for node in root.descendants() {
        let data = node.data();
        if matches!(data.value, NodeValue::Document) {
            continue;
        }
        let location = lines.location(data.sourcepos)?;
        match &data.value {
            NodeValue::FootnoteDefinition(note) => syntax.footnotes.push(Footnote {
                location,
                name: budget.copy(&note.name)?,
                references: 0,
            }),
            NodeValue::FootnoteReference(note) => {
                let definition =
                    node.ancestors()
                        .skip(1)
                        .find_map(|parent| match &parent.data().value {
                            NodeValue::FootnoteDefinition(note) => Some(note.name.clone()),
                            _ => None,
                        });
                if let Some(name) = &definition {
                    budget.charge(name.len())?;
                }
                syntax.footnote_references.push(FootnoteReference {
                    location,
                    name: budget.copy(&note.name)?,
                    number: 0,
                    definition,
                    in_image: inside_image(node),
                    emitted: false,
                });
            }
            _ => {}
        }
    }
    syntax
        .footnotes
        .sort_by_key(|footnote| footnote.location.span);
    syntax
        .footnote_references
        .sort_by_key(|reference| reference.location.span);
    qualify_footnotes(&mut syntax)?;
    let references: BTreeMap<_, _> = syntax
        .footnotes
        .iter()
        .map(|footnote| (footnote.name.as_str(), footnote.references))
        .collect();
    for node in root.descendants() {
        if let NodeValue::FootnoteDefinition(note) = &mut node.data_mut().value {
            note.total_references = references.get(note.name.as_str()).copied().unwrap_or(0);
        }
    }
    for node in root.descendants() {
        let data = node.data();
        if matches!(data.value, NodeValue::Document) {
            continue;
        }
        let location = lines.location(data.sourcepos)?;
        if let Some((tex, display)) = formula(&data.value, identity.profile) {
            syntax.math.push(Math {
                location,
                tex: budget.copy(tex)?,
                display,
                emitted: emitted(node),
            });
        }
        match &data.value {
            NodeValue::BlockDirective(directive) => {
                let (header, info) =
                    lines.directive_header(data.sourcepos, directive.fence_length)?;
                syntax.directives.push(Directive {
                    location,
                    header,
                    info: budget.copy(info)?,
                    emitted: emitted(node),
                });
            }
            NodeValue::Heading(heading) => {
                if let Some(location) = duplicate_heading_id(node, &lines)? {
                    return Err(Error::Unsupported {
                        location,
                        reason: "multiple explicit heading IDs are ambiguous",
                    });
                }
                let attrs = data.attrs.as_ref();
                let text = node.collect_text();
                budget.charge(text.len())?;
                let id = attrs
                    .and_then(|a| a.id.as_deref())
                    .map(|id| budget.copy(id))
                    .transpose()?;
                let classes = attrs
                    .into_iter()
                    .flat_map(|a| &a.classes)
                    .map(|c| budget.copy(c))
                    .collect::<Result<_, _>>()?;
                let attributes = attrs
                    .into_iter()
                    .flat_map(|a| &a.pairs)
                    .map(|(k, v)| Ok((budget.copy(k)?, budget.copy(v)?)))
                    .collect::<Result<_, Error>>()?;
                syntax.headings.push(Heading {
                    location,
                    level: heading.level,
                    text,
                    id,
                    classes,
                    attributes,
                    emitted: emitted(node),
                });
            }
            NodeValue::Link(link) | NodeValue::Image(link) => syntax.links.push(Link {
                location,
                form: LinkForm::Markdown,
                kind: if matches!(data.value, NodeValue::Image(_)) {
                    LinkKind::Image
                } else {
                    LinkKind::Link
                },
                destination: budget.copy(&link.url)?,
                title: budget.copy(&link.title)?,
                image_alt: if matches!(data.value, NodeValue::Image(_)) {
                    Some(image_alt(node, &mut budget)?)
                } else {
                    None
                },
                emitted: emitted(node),
            }),
            NodeValue::WikiLink(link) => syntax.links.push(Link {
                location,
                kind: LinkKind::Link,
                form: LinkForm::WikiLink,
                destination: budget.copy(&link.url)?,
                title: String::new(),
                image_alt: None,
                emitted: emitted(node),
            }),
            NodeValue::HtmlBlock(block) => syntax.html.push(Html {
                location,
                literal: budget.copy(&block.literal)?,
                in_footnote_or_image: nested_special(node),
            }),
            NodeValue::HtmlInline(literal) => syntax.html.push(Html {
                location,
                literal: budget.copy(literal)?,
                in_footnote_or_image: nested_special(node),
            }),
            _ => {}
        }
    }
    for edge in root.traverse() {
        let (node, entering) = match edge {
            NodeEdge::Start(n) => (n, true),
            NodeEdge::End(n) => (n, false),
        };
        let data = node.data();
        let value = match (&data.value, entering) {
            (NodeValue::HtmlBlock(html), true) => Some(html.literal.as_str()),
            (NodeValue::HtmlInline(html), true) => Some(html.as_str()),
            _ => text_piece(&data.value, entering),
        };
        if let Some(value) = value {
            syntax.text.push(Text {
                location: lines.location(data.sourcepos)?,
                value: budget.copy(value)?,
            });
        }
    }
    // Definitions stay where authored, while other extensions may normalize order.
    syntax.headings.sort_by_key(|h| h.location.span);
    syntax.links.sort_by_key(|l| l.location.span);
    syntax.math.sort_by_key(|m| m.location.span);
    syntax.directives.sort_by_key(|d| d.location.span);
    Ok(syntax)
}

/// Locate a second ID token only after Comrak has recognized the complete
/// heading attribute block. This is a source-accounting check over parser
/// syntax, not an independent Markdown or attribute parser.
fn duplicate_heading_id(
    node: Node<'_>,
    lines: &Lines<'_>,
) -> Result<Option<gazette_core::source::Location>, Error> {
    if node
        .data()
        .attrs
        .as_ref()
        .and_then(|attributes| attributes.id.as_ref())
        .is_none()
    {
        return Ok(None);
    }

    let heading = lines.location(node.data().sourcepos)?;
    let mut content_end = heading.span.start;
    for child in node.descendants().skip(1) {
        let end = lines.location(child.data().sourcepos)?.span.end;
        content_end = content_end.max(end);
    }
    let remainder = lines.slice(content_end..heading.span.end)?;
    let open = remainder.find('{').ok_or(Error::InvalidSource)?;
    let attribute_start = content_end.checked_add(open).ok_or(Error::InvalidSource)?;
    let attribute_tail = lines.slice(attribute_start..heading.span.end)?;
    let close = attribute_tail.rfind('}').ok_or(Error::InvalidSource)?;
    let attribute_end = attribute_start
        .checked_add(close + 1)
        .ok_or(Error::InvalidSource)?;
    let attributes = lines.slice(attribute_start..attribute_end)?;

    let mut quoted = false;
    let mut escaped = false;
    let mut ids = 0usize;
    let mut previous = None;
    for (offset, character) in attributes.char_indices() {
        if quoted {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else if character == '"' {
            quoted = true;
        } else if character == '#'
            && previous
                .is_some_and(|character: char| character == '{' || character.is_ascii_whitespace())
        {
            ids += 1;
            if ids > 1 {
                let end = id_token_end(attributes, offset)?;
                let start = attribute_start
                    .checked_add(offset)
                    .ok_or(Error::InvalidSource)?;
                let end = attribute_start
                    .checked_add(end)
                    .ok_or(Error::InvalidSource)?;
                return lines.exact(start..end).map(Some);
            }
        }
        previous = Some(character);
    }
    Ok(None)
}

fn id_token_end(attributes: &str, start: usize) -> Result<usize, Error> {
    let value_start = start.checked_add(1).ok_or(Error::InvalidSource)?;
    let value = attributes.get(value_start..).ok_or(Error::InvalidSource)?;
    let mut characters = value.char_indices();
    if characters
        .next()
        .is_some_and(|(_, character)| character == '"')
    {
        let mut escaped = false;
        for (offset, character) in characters {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                return value_start
                    .checked_add(offset + character.len_utf8())
                    .ok_or(Error::InvalidSource);
            }
        }
        return Err(Error::InvalidSource);
    }
    for (offset, character) in value.char_indices() {
        if character.is_ascii_whitespace() || character == '}' {
            return value_start.checked_add(offset).ok_or(Error::InvalidSource);
        }
    }
    Err(Error::InvalidSource)
}

/// Derive the exact normalized footnote closure instead of trusting the
/// parser counts, which include references in unused definitions and images.
fn qualify_footnotes(syntax: &mut Syntax) -> Result<(), Error> {
    let definitions: BTreeSet<_> = syntax
        .footnotes
        .iter()
        .map(|footnote| footnote.name.clone())
        .collect();
    let mut reachable = BTreeSet::<String>::new();
    for reference in &syntax.footnote_references {
        if reference.definition.is_none()
            && !reference.in_image
            && definitions.contains(&reference.name)
        {
            reachable.insert(reference.name.clone());
        }
    }
    loop {
        let mut changed = false;
        for reference in &syntax.footnote_references {
            if !reference.in_image
                && reference
                    .definition
                    .as_deref()
                    .is_some_and(|parent| reachable.contains(parent))
                && definitions.contains(&reference.name)
            {
                changed |= reachable.insert(reference.name.clone());
            }
        }
        if !changed {
            break;
        }
    }
    let mut counts = BTreeMap::<String, u32>::new();
    for reference in &mut syntax.footnote_references {
        reference.emitted = !reference.in_image
            && definitions.contains(&reference.name)
            && reference
                .definition
                .as_deref()
                .is_none_or(|parent| reachable.contains(parent));
        if reference.emitted {
            let count = counts.entry(reference.name.clone()).or_default();
            *count = count
                .checked_add(1)
                .ok_or(Error::Limit("footnote references"))?;
            reference.number = *count;
        }
    }
    for footnote in &mut syntax.footnotes {
        footnote.references = counts.get(&footnote.name).copied().unwrap_or(0);
    }
    Ok(())
}

fn nested_special(node: Node<'_>) -> bool {
    node.ancestors().skip(1).any(|p| {
        matches!(
            p.data().value,
            NodeValue::FootnoteDefinition(_) | NodeValue::Image(_)
        )
    })
}

fn inside_image(node: Node<'_>) -> bool {
    node.ancestors()
        .skip(1)
        .any(|parent| matches!(parent.data().value, NodeValue::Image(_)))
}

fn text_piece(value: &NodeValue, entering: bool) -> Option<&str> {
    match (value, entering) {
        (NodeValue::Text(text), true) => Some(text),
        (NodeValue::Code(code), true) => Some(&code.literal),
        (NodeValue::CodeBlock(code), true) => Some(&code.literal),
        (NodeValue::Math(formula), true) => Some(&formula.literal),
        (NodeValue::TaskItem(item), true) => Some(if item.symbol.is_some() {
            "[x] "
        } else {
            "[ ] "
        }),
        (NodeValue::SoftBreak, true) => Some(" "),
        (NodeValue::LineBreak, true) => Some("\n"),
        (
            NodeValue::Paragraph
            | NodeValue::Heading(_)
            | NodeValue::TableRow(_)
            | NodeValue::CodeBlock(_),
            false,
        ) => Some("\n"),
        (NodeValue::TableCell, false) => Some("\t"),
        _ => None,
    }
}

pub(crate) fn text_filtered<'a>(
    root: Node<'_>,
    limit: usize,
    mut omit: impl FnMut(Node<'_>) -> Result<bool, Error>,
    mut prefix: impl FnMut(Node<'_>) -> Result<Option<&'a str>, Error>,
) -> Result<String, Error> {
    let mut output = Output::new(limit);
    let mut skipped = None;
    for edge in root.traverse() {
        let (node, entering) = match edge {
            NodeEdge::Start(n) => (n, true),
            NodeEdge::End(n) => (n, false),
        };
        if let Some(parent) = skipped {
            if !entering && std::ptr::eq(node, parent) {
                skipped = None;
            }
            continue;
        }
        if entering && omit(node)? {
            skipped = Some(node);
            continue;
        }
        if entering && let Some(text) = prefix(node)? {
            writeln!(output, "{text}").map_err(|_| output.error())?;
        }
        let data = node.data();
        if let NodeValue::Alert(alert) = &data.value
            && entering
        {
            writeln!(output, "{}", alert.alert_type.default_title()).map_err(|_| output.error())?;
        } else if let Some(value) = text_piece(&data.value, entering) {
            output.write_str(value).map_err(|_| output.error())?;
        }
    }
    Ok(output.value)
}

// The formatter renders image children in Comrak's Plain mode. These are the
// exact unescaped AST text pieces in that mode; markup itself is not alt text.
fn image_alt(root: Node<'_>, budget: &mut Budget) -> Result<String, Error> {
    let mut text = String::new();
    for node in root.descendants().skip(1) {
        let data = node.data();
        let part = match &data.value {
            NodeValue::Text(value) => Some(value.as_ref()),
            NodeValue::HtmlInline(value) => Some(value.as_str()),
            NodeValue::Code(value) => Some(value.literal.as_str()),
            NodeValue::Math(value) => Some(value.literal.as_str()),
            NodeValue::LineBreak | NodeValue::SoftBreak => Some(" "),
            _ => None,
        };
        if let Some(part) = part {
            budget.charge(part.len())?;
            text.push_str(part);
        }
    }
    Ok(text)
}
