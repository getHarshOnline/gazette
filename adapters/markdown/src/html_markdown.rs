//! Readable Markdown from admitted component HTML, using the same bounded native tree.

use comrak::{
    Arena, Node,
    nodes::{
        ListType, NodeCode, NodeCodeBlock, NodeHeading, NodeHtmlBlock, NodeLink, NodeList,
        NodeMath, NodeTable, NodeValue, TableAlignment,
    },
};
use gazette_core::{
    markdown::{Error, Limits},
    source::Location,
};
use html5ever::ns;

use crate::{
    bounded::{Budget, Output},
    html_tree::{self, Handle, Kind},
};

pub(crate) fn encode(html: &str, limits: Limits, location: Location) -> Result<String, Error> {
    let tree = html_tree::parse(html, limits).map_err(|error| match error {
        Error::Formatting => Error::Unsupported {
            location,
            reason: "selected HTML does not have an unambiguous native fragment tree",
        },
        error => error,
    })?;
    let arena = Arena::new();
    let root = arena.alloc(NodeValue::Document.into());
    let mut lowering = Lowering {
        arena: &arena,
        budget: Budget(limits.output_bytes),
        limits,
        location,
    };
    if let Err(error) = lowering.children(&tree.root, root) {
        return match error {
            Error::Unsupported { .. } => crate::html_transport::encode(html, limits, location),
            error => Err(error),
        };
    }
    if root.validate().is_err() {
        return crate::html_transport::encode(html, limits, location);
    }
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.math_dollars = true;
    options.render.prefer_fenced = true;
    let mut output = Output::new(limits.output_bytes);
    comrak::format_commonmark(root, &options, &mut output).map_err(|_| output.error())?;
    Ok(output.value)
}

struct Lowering<'a> {
    arena: &'a Arena<'a>,
    budget: Budget,
    limits: Limits,
    location: Location,
}

impl<'a> Lowering<'a> {
    fn append(&self, parent: Node<'a>, value: NodeValue) -> Node<'a> {
        let node = self.arena.alloc(value.into());
        parent.append(node);
        node
    }

    fn children(&mut self, source: &Handle, parent: Node<'a>) -> Result<(), Error> {
        let mut paragraph = None;
        let children = source.children.borrow();
        for (index, child) in children.iter().enumerate() {
            // The inline SVG presentation and its retained image are one authored
            // occurrence. Markdown uses the resource URL carried by that image.
            if let Some(address) = attribute(child, "data-gazette-image-ref")
                && children.get(index + 1).is_some_and(|next| {
                    element(next, "img")
                        && attribute(next, "data-gazette-image").as_ref() == Some(&address)
                })
            {
                continue;
            }
            if parent.data().value.block()
                && !parent.data().value.contains_inlines()
                && inline(child)
            {
                if matches!(&child.kind, Kind::Text(text) if text.borrow().chars().all(flow_space))
                    && paragraph.is_none()
                {
                    continue;
                }
                let target =
                    *paragraph.get_or_insert_with(|| self.append(parent, NodeValue::Paragraph));
                self.node(child, target)?;
            } else {
                paragraph = None;
                self.node(child, parent)?;
            }
        }
        Ok(())
    }

    fn text(&mut self, source: &Handle) -> Result<String, Error> {
        let mut text = String::new();
        self.text_into(source, &mut text)?;
        Ok(text)
    }

    fn text_into(&mut self, source: &Handle, output: &mut String) -> Result<(), Error> {
        if let Kind::Text(text) = &source.kind {
            let text = text.borrow();
            self.budget.charge(text.len())?;
            output.push_str(&text);
        }
        for child in source.children.borrow().iter() {
            self.text_into(child, output)?;
        }
        Ok(())
    }

    fn anchor(&mut self, source: &Handle, parent: Node<'a>) -> Result<(), Error> {
        if let Some(id) = attribute(source, "id") {
            let escaped = id
                .replace('&', "&amp;")
                .replace('"', "&quot;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            let html = format!("<a id=\"{escaped}\"></a>");
            self.budget.charge(html.len())?;
            if !parent.data().value.block() || parent.data().value.contains_inlines() {
                self.append(parent, NodeValue::HtmlInline(html));
            } else {
                self.append(
                    parent,
                    NodeValue::HtmlBlock(NodeHtmlBlock {
                        block_type: 1,
                        literal: html,
                    }),
                );
            }
        }
        Ok(())
    }

    fn node(&mut self, source: &Handle, parent: Node<'a>) -> Result<(), Error> {
        let previous = parent.last_child();
        match self.node_inner(source, parent) {
            Err(Error::Unsupported { .. }) => {
                // A failed lowering may already have emitted an anchor or table
                // prefix. Replace only that attempt, keeping earlier siblings.
                while let Some(last) = parent.last_child() {
                    if previous.is_some_and(|previous| std::ptr::eq(previous, last)) {
                        break;
                    }
                    last.detach();
                }
                if (!parent.data().value.block() || parent.data().value.contains_inlines())
                    && has_block_html(source)
                {
                    return Err(Error::Unsupported {
                        location: self.location,
                        reason: "foreign HTML integration needs its original block boundary",
                    });
                }
                let html = crate::html_transport::encode_node(
                    source,
                    Limits {
                        output_bytes: self.budget.0,
                        ..self.limits
                    },
                    self.location,
                )?;
                self.budget.charge(html.len())?;
                let value = if !parent.data().value.block()
                    || parent.data().value.contains_inlines()
                {
                    NodeValue::HtmlInline(html)
                } else {
                    // SVG/MathML are inline HTML to CommonMark even when the
                    // retained subtree has block-level integration children.
                    // A minimal block boundary keeps that complete tree intact.
                    let html = if matches!(&source.kind, Kind::Element { name, .. } if name.ns != ns!(html))
                    {
                        self.budget.charge("<div></div>".len())?;
                        format!("<div>{html}</div>")
                    } else {
                        html
                    };
                    NodeValue::HtmlBlock(NodeHtmlBlock {
                        block_type: 1,
                        literal: html,
                    })
                };
                if !parent.can_contain_type(&value) {
                    return Err(Error::Unsupported {
                        location: self.location,
                        reason: "Markdown requires the containing HTML structure",
                    });
                }
                self.append(parent, value);
                Ok(())
            }
            result => result,
        }
    }

    fn node_inner(&mut self, source: &Handle, parent: Node<'a>) -> Result<(), Error> {
        let Kind::Element { name, template, .. } = &source.kind else {
            return match &source.kind {
                Kind::Text(text) => {
                    let text = text.borrow();
                    if !text.chars().all(flow_space)
                        || !parent.data().value.block()
                        || parent.data().value.contains_inlines()
                    {
                        self.budget.charge(text.len())?;
                        let text = flow_text(&text);
                        self.append(parent, NodeValue::Text(text.into()));
                    }
                    Ok(())
                }
                Kind::Document => self.children(source, parent),
                Kind::Comment(_) | Kind::Pi(_, _) => Ok(()),
                Kind::Element { .. } => unreachable!(),
            };
        };
        if attribute(source, "aria-hidden").as_deref() == Some("true") {
            return Ok(());
        }
        let tag = name.local.as_ref();
        if name.ns == ns!(mathml) && tag == "math" {
            let annotation = tex_annotation(source).ok_or(Error::Unsupported {
                location: self.location,
                reason: "Markdown math requires its retained TeX annotation",
            })?;
            let literal = self.text(&annotation)?;
            self.append(
                parent,
                NodeValue::Math(NodeMath {
                    dollar_math: true,
                    display_math: attribute(source, "display").as_deref() == Some("block"),
                    literal,
                }),
            );
            return Ok(());
        }
        if name.ns != ns!(html) {
            return Err(Error::Unsupported {
                location: self.location,
                reason: "Markdown requires an image reference or accessible semantic representation for foreign markup",
            });
        }
        if matches!(tag, "script" | "style" | "template") || template.is_some() {
            return Ok(());
        }
        if tag != "li" {
            self.anchor(source, parent)?;
        }
        if let Some(count) = attribute(source, "data-execution-count") {
            let text = format!("Execution count: {count}");
            self.budget.charge(text.len())?;
            let paragraph = self.append(parent, NodeValue::Paragraph);
            self.append(paragraph, NodeValue::Text(text.into()));
        }
        let child = match tag {
            "p" => {
                let paragraph = self.append(parent, NodeValue::Paragraph);
                if attribute(source, "class").is_some_and(|classes| {
                    classes
                        .split_whitespace()
                        .any(|class| class == "markdown-alert-title")
                }) {
                    self.append(paragraph, NodeValue::Strong)
                } else {
                    paragraph
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => self.append(
                parent,
                NodeValue::Heading(NodeHeading {
                    level: tag.as_bytes()[1] - b'0',
                    ..Default::default()
                }),
            ),
            "strong" | "b" => self.append(parent, NodeValue::Strong),
            "em" | "i" => self.append(parent, NodeValue::Emph),
            "del" | "s" => self.append(parent, NodeValue::Strikethrough),
            "blockquote" => self.append(parent, NodeValue::BlockQuote),
            "br" => {
                self.append(parent, NodeValue::LineBreak);
                return Ok(());
            }
            "hr" => {
                self.append(parent, NodeValue::ThematicBreak);
                return Ok(());
            }
            "summary" => {
                let paragraph = self.append(parent, NodeValue::Paragraph);
                self.append(paragraph, NodeValue::Strong)
            }
            "a" => {
                if let Some(url) = attribute(source, "href") {
                    self.budget.charge(url.len())?;
                    let title = attribute(source, "title").unwrap_or_default();
                    self.budget.charge(title.len())?;
                    let link =
                        self.append(parent, NodeValue::Link(Box::new(NodeLink { url, title })));
                    self.children(source, link)?;
                    if attribute(source, "data-gazette-destination-kind").as_deref()
                        == Some("cross-site")
                        && let Some(name) = attribute(source, "data-gazette-destination-site-name")
                        && !name.trim().is_empty()
                    {
                        let context = format!(" (on {})", flow_text(&name));
                        self.budget.charge(context.len())?;
                        self.append(parent, NodeValue::Text(context.into()));
                    }
                    return Ok(());
                } else {
                    parent
                }
            }
            "input" if attribute(source, "type").as_deref() == Some("checkbox") => {
                let label = if attribute(source, "checked").is_some() {
                    "[x] "
                } else {
                    "[ ] "
                };
                let label = self.budget.copy(label)?;
                self.append(parent, NodeValue::Text(label.into()));
                return Ok(());
            }
            // Markdown keeps the default image; mode sources have no Markdown form.
            "picture" => {
                for child in source.children.borrow().iter() {
                    if element(child, "img") {
                        self.node(child, parent)?;
                    }
                }
                return Ok(());
            }
            "img" => {
                let url = attribute(source, "src").ok_or(Error::InvalidPlan)?;
                let title = attribute(source, "title").unwrap_or_default();
                let alt = attribute(source, "alt").unwrap_or_default();
                self.budget.charge(url.len() + title.len() + alt.len())?;
                let image =
                    self.append(parent, NodeValue::Image(Box::new(NodeLink { url, title })));
                self.append(image, NodeValue::Text(alt.into()));
                return Ok(());
            }
            "code" | "kbd" | "samp" => {
                let literal = self.text(source)?;
                self.append(
                    parent,
                    NodeValue::Code(NodeCode {
                        literal,
                        ..Default::default()
                    }),
                );
                return Ok(());
            }
            "pre" | "textarea" => {
                let literal = self.text(source)?;
                let info = find(source, |node| element(node, "code"))
                    .and_then(|node| attribute(&node, "class"))
                    .and_then(|classes| {
                        classes
                            .split_whitespace()
                            .find_map(|class| class.strip_prefix("language-").map(str::to_owned))
                    })
                    .unwrap_or_default();
                self.budget.charge(info.len())?;
                self.append(
                    parent,
                    NodeValue::CodeBlock(Box::new(NodeCodeBlock {
                        fenced: true,
                        fence_char: b'`',
                        fence_length: 3,
                        literal,
                        info,
                        ..Default::default()
                    })),
                );
                return Ok(());
            }
            "ul" | "ol" => self.append(
                parent,
                NodeValue::List(NodeList {
                    list_type: if tag == "ol" {
                        ListType::Ordered
                    } else {
                        ListType::Bullet
                    },
                    start: attribute(source, "start")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1),
                    bullet_char: b'-',
                    tight: false,
                    ..Default::default()
                }),
            ),
            "li" => {
                let list = match parent.data().value {
                    NodeValue::List(list) => list,
                    _ => NodeList::default(),
                };
                let item = self.append(parent, NodeValue::Item(list));
                self.anchor(source, item)?;
                item
            }
            "table" => return self.table(source, parent),
            "sup" | "sub" | "ins" => {
                self.append(parent, NodeValue::HtmlInline(format!("<{tag}>")));
                self.children(source, parent)?;
                self.append(parent, NodeValue::HtmlInline(format!("</{tag}>")));
                return Ok(());
            }
            // Layout wrappers contribute no reader-facing syntax. Their admitted
            // child text remains in document order. Unknown controls and media
            // keep the lossless representation rather than losing their values.
            "div" | "section" | "aside" | "article" | "main" | "header" | "footer" | "nav"
            | "span" | "figure" | "details" | "dl" | "label" | "button" | "mark" | "small"
            | "time" | "output" | "figcaption" | "caption" | "dt" | "dd" => parent,
            _ => {
                return Err(Error::Unsupported {
                    location: self.location,
                    reason: "Markdown has no complete semantic lowering for this element",
                });
            }
        };
        self.children(source, child)
    }

    fn table(&mut self, source: &Handle, parent: Node<'a>) -> Result<(), Error> {
        let mut rows = Vec::new();
        table_rows(source, &mut rows);
        if rows.is_empty() {
            return self.children(source, parent);
        }
        let columns = rows
            .iter()
            .map(|row| {
                row.children
                    .borrow()
                    .iter()
                    .filter(|child| element(child, "td") || element(child, "th"))
                    .count()
            })
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return self.children(source, parent);
        }
        if let Some(caption) = source
            .children
            .borrow()
            .iter()
            .find(|node| element(node, "caption"))
        {
            self.node(caption, parent)?;
        }
        let table = self.append(
            parent,
            NodeValue::Table(Box::new(NodeTable {
                alignments: vec![TableAlignment::None; columns],
                num_columns: columns,
                num_rows: rows.len(),
                num_nonempty_cells: rows.len().saturating_mul(columns),
            })),
        );
        for (index, row) in rows.iter().enumerate() {
            let row_node = self.append(table, NodeValue::TableRow(index == 0));
            for cell in row
                .children
                .borrow()
                .iter()
                .filter(|node| element(node, "td") || element(node, "th"))
            {
                if attribute(cell, "rowspan").is_some_and(|v| v != "1")
                    || attribute(cell, "colspan").is_some_and(|v| v != "1")
                {
                    return Err(Error::Unsupported {
                        location: self.location,
                        reason: "Markdown table cannot preserve merged cells",
                    });
                }
                let cell_node = self.append(row_node, NodeValue::TableCell);
                self.children(cell, cell_node)?;
            }
        }
        Ok(())
    }
}

fn flow_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\r' | '\n' | '\u{000c}')
}

fn flow_text(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut space = false;
    for character in text.chars() {
        if flow_space(character) {
            if !space {
                output.push(' ');
            }
            space = true;
        } else {
            output.push(character);
            space = false;
        }
    }
    output
}

fn inline(node: &Handle) -> bool {
    match &node.kind {
        Kind::Text(_) => true,
        Kind::Element { name, .. } if name.ns == ns!(mathml) && name.local.as_ref() == "math" => {
            tex_annotation(node).is_some()
        }
        Kind::Element { name, .. } if name.local.as_ref() == "span" => {
            node.children.borrow().iter().all(inline)
        }
        // A mode picture is one image: its sources only offer alternative renditions.
        Kind::Element { name, .. } if name.local.as_ref() == "picture" => node
            .children
            .borrow()
            .iter()
            .all(|child| inline(child) || element(child, "source")),
        Kind::Element { name, .. } => matches!(
            name.local.as_ref(),
            "a" | "output"
                | "strong"
                | "b"
                | "em"
                | "i"
                | "del"
                | "s"
                | "br"
                | "img"
                | "code"
                | "kbd"
                | "samp"
                | "sup"
                | "sub"
                | "input"
        ),
        _ => false,
    }
}

fn has_block_html(node: &Handle) -> bool {
    matches!(&node.kind, Kind::Element { name, .. } if name.ns == ns!(html) && !inline(node))
        || node.children.borrow().iter().any(has_block_html)
}

fn tex_annotation(node: &Handle) -> Option<Handle> {
    find(node, |node| {
        element(node, "annotation")
            && attribute(node, "encoding").as_deref() == Some("application/x-tex")
    })
}

fn attribute(node: &Handle, key: &str) -> Option<String> {
    match &node.kind {
        Kind::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .find(|attr| attr.name.local.as_ref() == key)
            .map(|attr| attr.value.to_string()),
        _ => None,
    }
}

fn element(node: &Handle, tag: &str) -> bool {
    matches!(&node.kind, Kind::Element { name, .. } if name.local.as_ref() == tag)
}

fn find(node: &Handle, predicate: impl Fn(&Handle) -> bool + Copy) -> Option<Handle> {
    if predicate(node) {
        return Some(node.clone());
    }
    node.children
        .borrow()
        .iter()
        .find_map(|child| find(child, predicate))
}

fn table_rows(node: &Handle, rows: &mut Vec<Handle>) {
    for child in node.children.borrow().iter() {
        if element(child, "tr") {
            rows.push(child.clone());
        } else if element(child, "thead") || element(child, "tbody") || element(child, "tfoot") {
            table_rows(child, rows);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/html_markdown.rs"]
mod tests;
