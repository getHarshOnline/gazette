//! Normalized adapter formatting with narrow resolved headings and excerpt layout.

use crate::{
    bounded::{Budget, Output},
    inspect,
    plan::Bindings,
    source::Lines,
};
use comrak::{
    Node,
    html::{ChildRendering, Context},
    nodes::{NodeHtmlBlock, NodeValue},
};
use gazette_core::markdown::{
    Error, Fragment, Limits, LinkForm, LinkKind, OpeningBoundary, Plan, Rendered, Syntax,
};
use gazette_core::source::Span;
use gazette_ports::markdown::ComponentRenderer;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write};

pub(crate) fn document(
    source: &str,
    syntax: &Syntax,
    plan: &Plan,
    bindings: Bindings<'_>,
    components: Option<&dyn ComponentRenderer>,
    limits: Limits,
) -> Result<Rendered, Error> {
    match project(source, syntax, plan, bindings, components, limits, false)? {
        Projection::Html(rendered) => Ok(rendered),
        Projection::Markdown(_) => Err(Error::InvalidPlan),
    }
}

pub(crate) fn markdown(
    source: &str,
    syntax: &Syntax,
    plan: &Plan,
    bindings: Bindings<'_>,
    components: Option<&dyn ComponentRenderer>,
    limits: Limits,
) -> Result<String, Error> {
    match project(source, syntax, plan, bindings, components, limits, true)? {
        Projection::Markdown(rendered) => Ok(rendered),
        Projection::Html(_) => Err(Error::InvalidPlan),
    }
}

enum Projection {
    Html(Rendered),
    Markdown(String),
}

fn project(
    source: &str,
    syntax: &Syntax,
    plan: &Plan,
    bindings: Bindings<'_>,
    components: Option<&dyn ComponentRenderer>,
    limits: Limits,
    markdown: bool,
) -> Result<Projection, Error> {
    let arena = comrak::Arena::new();
    let options = crate::options(syntax.identity.profile, false);
    let root = comrak::parse_document(&arena, source, &options);
    inspect::check_tree(root, limits)?;
    let lines = Lines::new(source);
    let Bindings {
        headings,
        links,
        notes,
        references,
        math,
        components: component_bindings,
    } = &bindings;
    let aliases: BTreeMap<_, _> = plan
        .footnotes
        .iter()
        .map(|n| (n.occurrence.name.as_str(), n.alias.as_str()))
        .collect();
    let mut seen_headings = BTreeSet::new();
    let mut seen_links = BTreeSet::new();
    let mut seen_notes = BTreeSet::new();
    let mut seen_references = BTreeSet::new();
    let mut seen_math = BTreeSet::new();
    let mut seen_components = BTreeSet::new();
    let mut marker = None;
    let mut omitted_notes = Vec::new();
    let mut replacements = Budget(limits.output_bytes);
    for node in root.descendants() {
        if matches!(node.data().value, NodeValue::Document) {
            continue;
        }
        let position = node.data().sourcepos;
        let location = lines.location(position)?;
        let span = location.span;
        let link_kind = match &node.data().value {
            NodeValue::Link(_) | NodeValue::WikiLink(_) => Some(LinkKind::Link),
            NodeValue::Image(_) => Some(LinkKind::Image),
            _ => None,
        };
        let emitted_link = link_kind.is_some() && inspect::emitted(node);
        if inspect::emitted(node)
            && let Some((tex, display)) =
                inspect::formula(&node.data().value, syntax.identity.profile)
        {
            let binding = math.get(&span).ok_or(Error::InvalidPlan)?;
            if binding.occurrence.location != location
                || binding.occurrence.tex != tex
                || binding.occurrence.display != display
                || !seen_math.insert(span)
            {
                return Err(Error::InvalidPlan);
            }
        }
        let mut data = node.data_mut();
        match &mut data.value {
            NodeValue::BlockDirective(directive) if inspect::emitted(node) => {
                let binding = component_bindings.get(&span).ok_or(Error::InvalidPlan)?;
                let (header, info) = lines.directive_header(position, directive.fence_length)?;
                if binding.occurrence.location != location
                    || binding.occurrence.header != header
                    || binding.occurrence.info != info
                    || !seen_components.insert(span)
                {
                    return Err(Error::InvalidPlan);
                }
                if components.is_none() {
                    return Err(Error::Unsupported {
                        location,
                        reason: "component requires its selected presentation renderer",
                    });
                }
                if binding.container_contents.is_some()
                    && (node.first_child().is_some()
                        || node.parent().is_none_or(|parent| {
                            !matches!(parent.data().value, NodeValue::Document)
                        }))
                {
                    return Err(Error::Unsupported {
                        location,
                        reason: "container contents require an empty top-level block",
                    });
                }
            }
            NodeValue::Heading(heading) => {
                let binding = headings.get(&span).ok_or(Error::InvalidPlan)?;
                if !binding.occurrence.emitted
                    || binding.occurrence.location != location
                    || binding.occurrence.level != heading.level
                    || !seen_headings.insert(span)
                {
                    return Err(Error::InvalidPlan);
                }
            }
            NodeValue::Link(link) | NodeValue::Image(link) if emitted_link => {
                let binding = links.get(&span).ok_or(Error::InvalidPlan)?;
                if Some(binding.occurrence.kind) != link_kind
                    || binding.occurrence.form != LinkForm::Markdown
                    || binding.occurrence.location != location
                    || link.url.as_str() != binding.occurrence.destination.as_str()
                    || link.title.as_str() != binding.occurrence.title.as_str()
                    || !seen_links.insert(span)
                {
                    return Err(Error::InvalidPlan);
                }
                replacements.charge(binding.destination.len())?;
                link.url.clone_from(&binding.destination);
            }
            NodeValue::WikiLink(link) if emitted_link => {
                let binding = links.get(&span).ok_or(Error::InvalidPlan)?;
                if binding.occurrence.kind != LinkKind::Link
                    || binding.occurrence.form != LinkForm::WikiLink
                    || binding.occurrence.location != location
                    || link.url != binding.occurrence.destination
                    || !binding.occurrence.title.is_empty()
                    || !seen_links.insert(span)
                {
                    return Err(Error::InvalidPlan);
                }
                replacements.charge(binding.destination.len())?;
                link.url.clone_from(&binding.destination);
            }
            NodeValue::FootnoteDefinition(note) => {
                let binding = notes.get(&span).ok_or(Error::InvalidPlan)?;
                if binding.occurrence.location != location || binding.occurrence.name != note.name {
                    return Err(Error::InvalidPlan);
                }
                note.total_references = binding.occurrence.references;
                if binding.occurrence.references == 0 {
                    omitted_notes.push(node);
                } else if !seen_notes.insert(span) {
                    return Err(Error::InvalidPlan);
                }
                replacements.charge(binding.alias.len())?;
                note.name.clone_from(&binding.alias);
            }
            NodeValue::FootnoteReference(note) => {
                let reference = references.get(&span).ok_or(Error::InvalidPlan)?;
                if reference.location != location || reference.name != note.name {
                    return Err(Error::InvalidPlan);
                }
                let alias = aliases.get(note.name.as_str()).ok_or(Error::InvalidPlan)?;
                replacements.charge(alias.len())?;
                note.name = (*alias).to_owned();
                if reference.emitted {
                    note.ref_num = reference.number;
                    if !seen_references.insert(span) {
                        return Err(Error::InvalidPlan);
                    }
                }
            }
            NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => {
                if !plan
                    .excerpt
                    .as_ref()
                    .is_some_and(|e| e.marker.location == location)
                    || marker.replace(node).is_some()
                {
                    return Err(Error::InvalidPlan);
                }
            }
            NodeValue::Raw(_) => return Err(Error::InvalidSource),
            _ => {}
        }
    }
    for node in omitted_notes {
        node.detach();
    }
    if seen_headings.len() != syntax.headings.iter().filter(|h| h.emitted).count()
        || seen_links.len() != syntax.links.iter().filter(|l| l.emitted).count()
        || seen_notes.len() != syntax.footnotes.iter().filter(|n| n.references > 0).count()
        || seen_references.len()
            != syntax
                .footnote_references
                .iter()
                .filter(|reference| reference.emitted)
                .count()
        || seen_math.len() != math.len()
        || seen_components.len() != component_bindings.len()
        || marker.is_some() != plan.excerpt.is_some()
    {
        return Err(Error::InvalidPlan);
    }
    crate::alerts::normalize(root, &lines)?;
    let semantics = crate::semantics::Selection::new(root, &lines, &bindings)?;
    let work = Cell::new(limits.nodes);
    let bytes = Cell::new(limits.output_bytes);
    let failure = RefCell::new(None);
    let opening_end = Cell::new(None);
    let state = State {
        lines: &lines,
        bindings: &bindings,
        semantics: &semantics,
        plan,
        excerpt: false,
        failure: &failure,
        components,
        limits,
        work: &work,
        bytes: &bytes,
        suppress: None,
        depth: 0,
        opening_end: &opening_end,
        opening_target: None,
        written: None,
    };
    if markdown {
        semantics.prune(root, &lines)?;
        return markdown_tree(root, &arena, &options, state).map(Projection::Markdown);
    }
    let body = fragment(root, &options, state.clone())?;
    let excerpt = if let Some(marker) = marker {
        truncate(marker);
        let omitted: Vec<_> = root
            .descendants()
            .filter(|n| {
                matches!(
                    n.data().value,
                    NodeValue::FootnoteDefinition(_) | NodeValue::FootnoteReference(_)
                )
            })
            .collect();
        for node in omitted {
            node.detach();
        }
        work.set(limits.nodes);
        bytes.set(limits.output_bytes);
        Some(fragment(
            root,
            &options,
            State {
                excerpt: true,
                ..state
            },
        )?)
    } else {
        None
    };
    Ok(Projection::Html(Rendered {
        body,
        excerpt,
        opening: opening_end
            .get()
            .map(|heading_end| OpeningBoundary { heading_end }),
    }))
}

// CommonMark has no allocated heading IDs, annotated links, selected components
// or MathML. Lower only those admitted subtrees through the HTML formatter used
// above; the remaining tree is formatted by Comrak, not copied from source.
fn markdown_tree<'a>(
    root: Node<'a>,
    arena: &'a comrak::Arena<'a>,
    options: &comrak::Options<'_>,
    state: State<'_>,
) -> Result<String, Error> {
    // One native appendix keeps global definition order/backlink indices. Native
    // Markdown definitions would lose references inside a static component body.
    let appendix = arena.alloc(NodeValue::Document.into());
    let mut transport = Budget(state.limits.output_bytes);
    let definitions: Vec<_> = root
        .descendants()
        .filter(|node| matches!(node.data().value, NodeValue::FootnoteDefinition(_)))
        .collect();
    for definition in definitions {
        appendix.append(definition);
    }
    let appendix_html = if appendix.first_child().is_some() {
        let location = state.lines.location(
            appendix
                .first_child()
                .ok_or(Error::InvalidPlan)?
                .data()
                .sourcepos,
        )?;
        let html = fragment(appendix, options, state.clone())?.html;
        let encoded = crate::html_markdown::encode(
            &html,
            Limits {
                output_bytes: transport.0,
                ..state.limits
            },
            location,
        )?;
        transport.charge(encoded.len())?;
        Some(encoded)
    } else {
        None
    };
    let rich = |node: Node<'_>| -> Result<bool, Error> {
        if !inspect::emitted(node) {
            return Ok(false);
        }
        let data = node.data();
        if inspect::formula(&data.value, state.plan.identity.profile).is_some() {
            return Ok(true);
        }
        match &data.value {
            NodeValue::Heading(_)
            | NodeValue::Alert(_)
            | NodeValue::BlockDirective(_)
            | NodeValue::WikiLink(_)
            | NodeValue::FootnoteReference(_)
            | NodeValue::HtmlBlock(_)
            | NodeValue::HtmlInline(_) => Ok(true),
            NodeValue::Link(_) => {
                let span = state.lines.location(data.sourcepos)?.span;
                Ok(state
                    .bindings
                    .links
                    .get(&span)
                    .ok_or(Error::InvalidPlan)?
                    .annotation
                    .is_some())
            }
            _ => Ok(false),
        }
    };
    let mut replacements = Vec::new();
    for node in root.descendants() {
        if !rich(node)? {
            continue;
        }
        let mut parent = node.parent();
        let mut enclosed = false;
        while let Some(ancestor) = parent {
            if rich(ancestor)? {
                enclosed = true;
                break;
            }
            parent = ancestor.parent();
        }
        if !enclosed {
            // Format before mutating any descendants: the bound HTML formatter
            // still validates their original native forms and source locations.
            let block = matches!(
                node.data().value,
                NodeValue::Heading(_)
                    | NodeValue::Alert(_)
                    | NodeValue::BlockDirective(_)
                    | NodeValue::HtmlBlock(_)
                    | NodeValue::CodeBlock(_)
            );
            let location = state.lines.location(node.data().sourcepos)?;
            if let Some(contents) = state
                .bindings
                .components
                .get(&location.span)
                .and_then(|binding| binding.container_contents.as_ref())
            {
                let markdown = contents
                    .reading_markdown
                    .as_deref()
                    .ok_or(Error::InvalidPlan)?;
                let encoded = crate::container_contents::reading(
                    markdown,
                    Limits {
                        output_bytes: transport.0,
                        ..state.limits
                    },
                )?;
                transport.charge(encoded.len())?;
                replacements.push((node, true, encoded));
                continue;
            }
            let html = fragment(node, options, state.clone())?.html;
            let encoded = crate::html_markdown::encode(
                &html,
                Limits {
                    output_bytes: transport.0,
                    ..state.limits
                },
                location,
            )?;
            transport.charge(encoded.len())?;
            let encoded = if block {
                encoded
            } else {
                encoded.trim_end_matches('\n').to_owned()
            };
            replacements.push((node, block, encoded));
        }
    }
    for (node, block, html) in replacements {
        while let Some(child) = node.first_child() {
            child.detach();
        }
        node.data_mut().value = if block {
            NodeValue::HtmlBlock(NodeHtmlBlock {
                block_type: 1,
                literal: html,
            })
        } else {
            NodeValue::HtmlInline(html)
        };
    }
    if let Some(html) = appendix_html {
        root.append(
            arena.alloc(
                NodeValue::HtmlBlock(NodeHtmlBlock {
                    block_type: 1,
                    literal: html,
                })
                .into(),
            ),
        );
    }
    let mut output = Output::new(state.limits.output_bytes);
    comrak::format_commonmark(root, options, &mut output).map_err(|_| output.error())?;
    Ok(output.value)
}

#[derive(Clone)]
struct State<'a> {
    lines: &'a Lines<'a>,
    bindings: &'a Bindings<'a>,
    plan: &'a Plan,
    semantics: &'a crate::semantics::Selection,
    excerpt: bool,
    failure: &'a RefCell<Option<Error>>,
    components: Option<&'a dyn ComponentRenderer>,
    limits: Limits,
    work: &'a Cell<usize>,
    bytes: &'a Cell<usize>,
    suppress: Option<Span>,
    depth: usize,
    opening_end: &'a Cell<Option<usize>>,
    opening_target: Option<Span>,
    written: Option<&'a Cell<usize>>,
}

struct TrackingOutput<'a> {
    output: &'a mut Output,
    written: &'a Cell<usize>,
}
impl fmt::Write for TrackingOutput<'_> {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        self.output.write_str(value)?;
        self.written.set(self.output.value.len());
        Ok(())
    }
}

fn fragment(
    root: Node<'_>,
    options: &comrak::Options<'_>,
    state: State<'_>,
) -> Result<Fragment, Error> {
    if state.depth > state.limits.depth {
        return Err(Error::Limit("component nesting"));
    }
    let presentation_only = state
        .suppress
        .and_then(|span| state.bindings.components.get(&span))
        .is_some_and(|binding| binding.presentation_only);
    let mut link_destinations = Vec::new();
    for node in root.descendants() {
        state.work.set(
            state
                .work
                .get()
                .checked_sub(1)
                .ok_or(Error::Limit("component rendering work"))?,
        );
        if presentation_only
            && matches!(
                node.data().value,
                NodeValue::Link(_) | NodeValue::WikiLink(_)
            )
        {
            let span = state.lines.location(node.data().sourcepos)?.span;
            let binding = state.bindings.links.get(&span).ok_or(Error::InvalidPlan)?;
            link_destinations.push(binding.destination.clone());
        }
    }
    let mut output = Output::new(state.limits.output_bytes);
    let written = Cell::new(0);
    let opening_target =
        if state.depth == 0 && !state.excerpt && matches!(root.data().value, NodeValue::Document) {
            root.first_child().and_then(|node| {
                if !matches!(node.data().value, NodeValue::Heading(_)) {
                    return None;
                }
                let span = state.lines.location(node.data().sourcepos).ok()?.span;
                state
                    .bindings
                    .headings
                    .get(&span)
                    .filter(|binding| binding.level == 1)
                    .map(|_| span)
            })
        } else {
            None
        };
    let state = State {
        opening_target,
        written: Some(&written),
        ..state
    };
    let result = comrak::html::format_document_with_formatter(
        root,
        options,
        &mut TrackingOutput {
            output: &mut output,
            written: &written,
        },
        &comrak::options::Plugins {
            render: comrak::options::RenderPlugins {
                codefence_syntax_highlighter: Some(&super::highlight::Highlight),
                ..Default::default()
            },
        },
        format_node,
        state.clone(),
    );
    if let Some(error) = state.failure.borrow_mut().take() {
        return Err(error);
    }
    result.map_err(|_| output.error())?;
    state.bytes.set(
        state
            .bytes
            .get()
            .checked_sub(output.value.len())
            .ok_or(Error::Limit("component rendering bytes"))?,
    );
    Ok(Fragment {
        html: output.value,
        link_destinations,
        text: inspect::text_filtered(
            root,
            state.limits.output_bytes,
            |node| {
                // Nested fragments are template inputs. Keep their complete text,
                // just like their HTML; filter only the final body or excerpt text.
                if state.depth == 0 {
                    state.semantics.omits(node, state.lines)
                } else {
                    Ok(false)
                }
            },
            |node| {
                if state.depth != 0 {
                    return Ok(None);
                }
                let data = node.data();
                if matches!(data.value, NodeValue::BlockDirective(_)) {
                    let span = state.lines.location(data.sourcepos)?.span;
                    Ok(state
                        .bindings
                        .components
                        .get(&span)
                        .and_then(|binding| binding.reading_prefix.as_deref()))
                } else {
                    Ok(None)
                }
            },
        )?,
    })
}

fn format_node(
    context: &mut Context<State<'_>>,
    node: Node<'_>,
    entering: bool,
) -> Result<ChildRendering, fmt::Error> {
    let data = node.data();
    // Preserve the parser's image-only paragraph boundary for Theme layout.
    // The default formatter still owns tight lists, footnotes and linked images.
    if matches!(data.value, NodeValue::Paragraph) && standalone_image(node) {
        drop(data);
        if entering {
            context.write_str("<div data-gazette-image-block>\n")?;
        }
        let rendered = comrak::html::format_node_default(context, node, entering)?;
        if !entering {
            context.write_str("</div>\n")?;
        }
        return Ok(rendered);
    }
    // Keep the native table grid intact inside a separate overflow boundary.
    // Themes own its geometry; Markdown alternates remain ordinary Markdown.
    if matches!(data.value, NodeValue::Table(_)) {
        drop(data);
        if entering {
            context.write_str("<div data-gazette-table>\n")?;
        }
        let rendered = comrak::html::format_node_default(context, node, entering)?;
        if !entering {
            context.write_str("</div>\n")?;
        }
        return Ok(rendered);
    }
    if matches!(data.value, NodeValue::BlockDirective(_)) {
        let location = match context.user.lines.location(data.sourcepos) {
            Ok(location) => location,
            Err(error) => {
                *context.user.failure.borrow_mut() = Some(error);
                return Err(fmt::Error);
            }
        };
        drop(data);
        if context.user.suppress == Some(location.span) {
            return Ok(ChildRendering::HTML);
        }
        if entering {
            let result = (|| {
                let binding = context
                    .user
                    .bindings
                    .components
                    .get(&location.span)
                    .ok_or(Error::InvalidPlan)?;
                let renderer = context.user.components.ok_or(Error::InvalidPlan)?;
                if context.user.excerpt && binding.container_contents.is_some() {
                    return Ok(String::new());
                }
                let body = fragment(
                    node,
                    context.options,
                    State {
                        suppress: Some(location.span),
                        depth: context.user.depth + 1,
                        ..context.user.clone()
                    },
                )?;
                let html = renderer.render(binding, &body, context.user.excerpt)?;
                if html.len() > context.user.limits.output_bytes {
                    return Err(Error::Limit("component output bytes"));
                }
                Ok(html)
            })();
            match result {
                Ok(html) => context.write_str(&html)?,
                Err(error) => {
                    *context.user.failure.borrow_mut() = Some(error);
                    return Err(fmt::Error);
                }
            }
        }
        return Ok(ChildRendering::Skip);
    }
    let emitted_math = inspect::formula(&data.value, context.user.plan.identity.profile).is_some()
        && inspect::emitted(node);
    if !emitted_math
        && !matches!(
            data.value,
            NodeValue::Heading(_)
                | NodeValue::Link(_)
                | NodeValue::WikiLink(_)
                | NodeValue::Image(_)
                | NodeValue::HtmlBlock(_)
                | NodeValue::HtmlInline(_)
        )
    {
        drop(data);
        return comrak::html::format_node_default(context, node, entering);
    }
    let span = match context.user.lines.location(data.sourcepos) {
        Ok(location) => location.span,
        Err(error) => {
            *context.user.failure.borrow_mut() = Some(error);
            return Err(fmt::Error);
        }
    };
    match &data.value {
        NodeValue::Heading(_) => {
            let Some(binding) = context.user.bindings.headings.get(&span).copied() else {
                *context.user.failure.borrow_mut() = Some(Error::InvalidPlan);
                return Err(fmt::Error);
            };
            if entering {
                write!(context, "<h{}", binding.level)?;
                if !context.user.excerpt {
                    context.write_str(" id=\"")?;
                    context.escape(&binding.id)?;
                    context.write_char('"')?;
                }
                if !binding.occurrence.classes.is_empty() {
                    context.write_str(" class=\"")?;
                    for (index, class) in binding.occurrence.classes.iter().enumerate() {
                        if index != 0 {
                            context.write_char(' ')?;
                        }
                        context.escape(class)?;
                    }
                    context.write_char('"')?;
                }
                context.write_char('>')?;
            } else {
                writeln!(context, "</h{}>", binding.level)?;
                if context.user.opening_target == Some(span)
                    && let Some(written) = context.user.written
                {
                    context.user.opening_end.set(Some(written.get()));
                }
            }
            Ok(ChildRendering::HTML)
        }
        NodeValue::Image(link) => {
            let Some(binding) = context.user.bindings.links.get(&span).copied() else {
                *context.user.failure.borrow_mut() = Some(Error::InvalidPlan);
                return Err(fmt::Error);
            };
            let Some(address) = &binding.image_address else {
                drop(data);
                return comrak::html::format_node_default(context, node, entering);
            };
            if entering {
                if let Some(svg) = &binding.inline_svg {
                    context.write_str("<span")?;
                    attribute(context, "data-gazette-image-ref", address)?;
                    let alt = binding.occurrence.image_alt.as_deref().unwrap_or_default();
                    if alt.is_empty() {
                        attribute(context, "aria-hidden", "true")?;
                    } else {
                        attribute(context, "role", "img")?;
                        attribute(context, "aria-label", alt)?;
                    }
                    if !link.title.is_empty() {
                        attribute(context, "title", &link.title)?;
                    }
                    context.write_char('>')?;
                    if context.user.excerpt {
                        svg.write(context, &format!("{address}:excerpt"))?;
                    } else {
                        svg.write(context, address)?;
                    }
                    context.write_str("</span>")?;
                }
                // A dark alternative lets the browser choose before any script runs.
                // The image keeps its default candidates for every other mode.
                if let Some(dark) = binding
                    .image_sources
                    .as_ref()
                    .and_then(|sources| sources.dark.as_ref())
                {
                    context.write_str("<picture data-gazette-image-modes><source")?;
                    attribute(context, "data-gazette-image-mode", "dark")?;
                    attribute(context, "media", dark.mode.dark_media())?;
                    if dark.candidates.is_empty() {
                        attribute(context, "srcset", &dark.url)?;
                    } else {
                        attribute(context, "srcset", &srcset(&dark.candidates))?;
                        if let Some(sources) = &binding.image_sources {
                            attribute(context, "sizes", &sources.sizes)?;
                        }
                    }
                    context.write_char('>')?;
                }
                context.write_str("<img")?;
                if binding.inline_svg.is_some() {
                    context.write_str(" hidden")?;
                }
                attribute(context, "data-gazette-image", address)?;
                attribute(context, "src", &binding.destination)?;
                if let Some(sources) = binding
                    .image_sources
                    .as_ref()
                    .filter(|sources| !sources.candidates.is_empty())
                {
                    attribute(context, "srcset", &srcset(&sources.candidates))?;
                    attribute(context, "sizes", &sources.sizes)?;
                }
                if let Some(loading) = binding.image_loading {
                    attribute(context, "loading", loading.as_str())?;
                }
                if let Some(priority) = binding.image_fetch_priority {
                    attribute(context, "fetchpriority", priority.as_str())?;
                }
                if binding.image_width.is_some() {
                    context.write_str(" data-gazette-image-sized")?;
                }
                let width = binding
                    .image_width
                    .map(std::num::NonZeroU32::get)
                    .or(binding.image_dimensions.map(|size| size.width))
                    .or_else(|| binding.svg_layout.and_then(|svg| svg.html_width()));
                if let Some(width) = width {
                    attribute(context, "width", &width.to_string())?;
                }
                if let Some(size) = binding.image_dimensions {
                    let height = binding.image_width.map_or(u64::from(size.height), |width| {
                        ((u64::from(width.get()) * u64::from(size.height)
                            + u64::from(size.width) / 2)
                            / u64::from(size.width))
                        .max(1)
                    });
                    attribute(context, "height", &height.to_string())?;
                }
                if binding.image_dimensions.is_none()
                    && let Some(height) = binding
                        .svg_layout
                        .zip(width)
                        .and_then(|(svg, width)| svg.html_height(width))
                {
                    attribute(context, "height", &height.to_string())?;
                }
                context.write_str(" alt=\"")?;
                Ok(ChildRendering::Plain)
            } else {
                context.write_char('"')?;
                if !link.title.is_empty() {
                    attribute(context, "title", &link.title)?;
                }
                context.write_str(" />")?;
                if binding
                    .image_sources
                    .as_ref()
                    .is_some_and(|sources| sources.dark.is_some())
                {
                    context.write_str("</picture>")?;
                }
                Ok(ChildRendering::HTML)
            }
        }
        NodeValue::Link(_) | NodeValue::WikiLink(_) => {
            let Some(binding) = context.user.bindings.links.get(&span).copied() else {
                *context.user.failure.borrow_mut() = Some(Error::InvalidPlan);
                return Err(fmt::Error);
            };
            let Some(annotation) = &binding.annotation else {
                drop(data);
                return comrak::html::format_node_default(context, node, entering);
            };
            if entering {
                context.write_str("<a href=\"")?;
                context.escape(&binding.destination)?;
                context.write_char('"')?;
                if let NodeValue::Link(link) = &data.value
                    && !link.title.is_empty()
                {
                    context.write_str(" title=\"")?;
                    context.escape(&link.title)?;
                    context.write_char('"')?;
                }
                attribute(context, "data-gazette-link", &annotation.address)?;
                attribute(
                    context,
                    "data-gazette-destination-kind",
                    destination_kind(annotation.kind),
                )?;
                if let Some(value) = &annotation.content {
                    attribute(context, "data-gazette-destination-content", value.as_str())?;
                }
                if let Some(value) = &annotation.site {
                    attribute(context, "data-gazette-destination-site", value.as_str())?;
                }
                if let Some(name) = &annotation.site_name {
                    attribute(context, "data-gazette-destination-site-name", name)?;
                }
                if !annotation.relationships.is_empty() {
                    let values = annotation
                        .relationships
                        .iter()
                        .copied()
                        .map(relationship)
                        .collect::<Vec<_>>()
                        .join(" ");
                    attribute(context, "data-gazette-destination-relationships", &values)?;
                }
                if let Some(value) = &annotation.theme {
                    attribute(context, "data-gazette-destination-theme", value.as_str())?;
                }
                if let Some(value) = &annotation.palette {
                    attribute(context, "data-gazette-destination-palette", value.as_str())?;
                }
                context.write_char('>')?;
            } else {
                context.write_str("</a>")?;
            }
            Ok(ChildRendering::HTML)
        }
        _ if emitted_math => {
            let Some(binding) = context.user.bindings.math.get(&span).copied() else {
                *context.user.failure.borrow_mut() = Some(Error::InvalidPlan);
                return Err(fmt::Error);
            };
            if entering {
                if matches!(data.value, NodeValue::CodeBlock(_)) {
                    // The retained MathML root is inline vocabulary to a
                    // CommonMark reader. This inert shared boundary preserves
                    // its actual block placement without adding a wrapper.
                    context.write_str("<!-- gazette-display-math -->")?;
                }
                context.write_str(&binding.mathml)?;
                if matches!(data.value, NodeValue::CodeBlock(_)) {
                    context.write_char('\n')?;
                }
            }
            Ok(ChildRendering::Skip)
        }
        NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => {
            if !context.user.excerpt && entering {
                let Some(excerpt) = context
                    .user
                    .plan
                    .excerpt
                    .as_ref()
                    .filter(|e| e.marker.location.span == span)
                else {
                    *context.user.failure.borrow_mut() = Some(Error::InvalidPlan);
                    return Err(fmt::Error);
                };
                if matches!(data.value, NodeValue::HtmlBlock(_)) {
                    // Keep the empty allocated span a standalone fragment in
                    // both representations, not a newly introduced paragraph.
                    context.write_str("<!-- gazette-continuation -->")?;
                }
                context.write_str("<span id=\"")?;
                context.escape(&excerpt.continuation)?;
                context.write_str("\"></span>")?;
            }
            Ok(ChildRendering::Skip)
        }
        _ => comrak::html::format_node_default(context, node, entering),
    }
}

fn standalone_image(node: Node<'_>) -> bool {
    fn count(node: Node<'_>) -> Option<usize> {
        match &node.data().value {
            NodeValue::Image(_) => Some(1),
            NodeValue::Text(text) if text.trim().is_empty() => Some(0),
            NodeValue::SoftBreak => Some(0),
            NodeValue::Paragraph | NodeValue::Link(_) | NodeValue::WikiLink(_) => {
                node.children().try_fold(0usize, |total, child| {
                    let total = total.checked_add(count(child)?)?;
                    (total <= 1).then_some(total)
                })
            }
            _ => None,
        }
    }
    count(node) == Some(1)
}

fn attribute(context: &mut Context<State<'_>>, name: &str, value: &str) -> Result<(), fmt::Error> {
    write!(context, " {name}=\"")?;
    context.escape(value)?;
    context.write_char('"')
}

fn destination_kind(value: gazette_core::markdown::LinkDestinationKind) -> &'static str {
    use gazette_core::markdown::LinkDestinationKind;
    match value {
        LinkDestinationKind::InternalNative => "internal-native",
        LinkDestinationKind::CrossSite => "cross-site",
        LinkDestinationKind::External => "external",
        LinkDestinationKind::Resource => "resource",
    }
}

fn relationship(value: gazette_core::markdown::LinkRelationship) -> &'static str {
    use gazette_core::markdown::LinkRelationship;
    match value {
        LinkRelationship::Related => "related",
        LinkRelationship::Collection => "collection",
        LinkRelationship::CollectionMember => "collection-member",
        LinkRelationship::Journey => "journey",
        LinkRelationship::JourneyMember => "journey-member",
    }
}

fn truncate(marker: Node<'_>) {
    let mut child = marker;
    while let Some(parent) = child.parent() {
        let following: Vec<_> = child.following_siblings().skip(1).collect();
        for sibling in following {
            sibling.detach();
        }
        child = parent;
    }
    marker.detach();
}

fn srcset(candidates: &[gazette_core::markdown::ImageCandidate]) -> String {
    candidates
        .iter()
        .map(|candidate| format!("{} {}w", candidate.url, candidate.width))
        .collect::<Vec<_>>()
        .join(", ")
}
