//! Project qualified alias implementation fragments through the existing HTML model.

use super::super::type_impls::Implementation;
use super::{Node, Page, State, Tag, attribute, label, state, tokenize};
use crate::{Error, invalid};
use std::collections::BTreeMap;

impl State<'_> {
    pub(super) fn type_data(&mut self, tag: &Tag) -> Result<bool, Error> {
        let source = attribute(tag, "src");
        let self_path = attribute(tag, "data-self-path");
        if self_path.is_none() && !source.is_some_and(|src| src.contains("type.impl/")) {
            return Ok(false);
        }
        let reject = || self.fail("unqualified Rustdoc type implementation script");
        if !self.inside
            || self.fragment
            || !self.page.alias
            || self.page.source
            || tag.name.as_ref() != "script"
            || self.page.type_script.is_some()
            || tag
                .attrs
                .iter()
                .any(|attr| !matches!(attr.name.local.as_ref(), "src" | "data-self-path" | "async"))
        {
            return Err(reject());
        }
        let source = source
            .filter(|src| label(src) && src.contains("type.impl/") && src.ends_with(".js"))
            .ok_or_else(reject)?;
        let self_path = self_path
            .filter(|path| {
                path.starts_with(&format!("{}::", self.page.crate_name))
                    && !path.chars().any(|ch| ch.is_control() || ch.is_whitespace())
            })
            .ok_or_else(reject)?;
        self.page.type_script = Some((source.into(), self_path.into()));
        Ok(true)
    }
}

pub(super) fn append(
    page: &mut Page,
    rows: Vec<Implementation>,
    origin: &str,
    data_origin: &str,
    maximum: usize,
) -> Result<(), Error> {
    let (native_root, _) = data_origin
        .rsplit_once("type.impl/")
        .filter(|(root, _)| root.is_empty() || root.ends_with('/'))
        .ok_or_else(|| {
            invalid(
                data_origin,
                "Rustdoc implementation data has no native root",
            )
        })?;
    let collections = collections_section(page, data_origin);
    if rows.iter().any(|row| !row.is_trait) {
        insert_group(
            page,
            "implementations",
            "Implementations",
            Vec::new(),
            origin,
        )?;
    }
    if rows.iter().any(|row| row.is_trait) {
        insert_group(
            page,
            "trait-implementations",
            "Trait Implementations",
            Vec::new(),
            origin,
        )?;
    }
    let mut inherent = Vec::new();
    let mut traits = Vec::new();
    for row in rows {
        let mut input = state(origin, maximum);
        input.inside = true;
        input.fragment = true;
        let mut fragment = tokenize(&row.html, input)?;
        let collections = collections
            .filter(|_| !row.is_trait && fragment.page.anchors.contains("method.insert"));
        page.decoded = page
            .decoded
            .checked_add(fragment.decoded)
            .filter(|count| *count <= maximum)
            .ok_or_else(|| invalid(origin, "Rustdoc decoded-byte ceiling exceeded"))?;
        page.tokens = page
            .tokens
            .checked_add(fragment.tokens)
            .filter(|count| *count <= maximum.min(250_000))
            .ok_or_else(|| invalid(origin, "Rustdoc token ceiling exceeded"))?;
        // Preserve unique native IDs and give each repeated section its own destination.
        // Reserve the fragment's other original IDs before allocating any suffix.
        let mut reserved = fragment.page.anchors.clone();
        let mut renamed = BTreeMap::new();
        for index in 0..fragment.page.nodes.len() {
            let Some(old) = node_id(&fragment.page.nodes[index]).map(str::to_owned) else {
                continue;
            };
            reserved.remove(&old);
            let mut id = old.clone();
            let mut ordinal = 0usize;
            while page.anchors.contains(&id) || reserved.contains(&id) {
                ordinal = ordinal
                    .checked_add(1)
                    .ok_or_else(|| invalid(origin, "Rustdoc alias anchor ordinal overflow"))?;
                id = format!("{old}-{ordinal}");
            }
            page.anchors.insert(id.clone());
            if id != old {
                if let Node::Start { attributes, .. } = &mut fragment.page.nodes[index] {
                    attributes.insert("id".into(), id.clone());
                }
                renamed.insert(format!("#{old}"), format!("#{id}"));
            }
        }
        // Native type data links from the Rustdoc output root, which may be nested
        // inside the retained capture alongside prepared source and job evidence.
        let root_prefix = "../".repeat(origin.bytes().filter(|byte| *byte == b'/').count());
        for node in &mut fragment.page.nodes {
            if let Node::Start { attributes, .. } = node
                && let Some(href) = attributes.get_mut("href")
            {
                if let Some(replacement) = renamed.get(href) {
                    *href = replacement.clone();
                } else if let Some(section) = collections
                    && href == "index.html#insert-and-complex-keys"
                {
                    *href = section.into();
                } else if !href.starts_with('#')
                    && !href.starts_with("https://")
                    && !href.starts_with("http://")
                {
                    *href = format!("{root_prefix}{native_root}{href}");
                }
                page.hrefs.insert(href.clone());
            }
        }
        if row.is_trait {
            traits.extend(fragment.page.nodes);
        } else {
            inherent.extend(fragment.page.nodes);
        }
    }
    if !inherent.is_empty() {
        insert_group(page, "implementations", "Implementations", inherent, origin)?;
    }
    if !traits.is_empty() {
        insert_group(
            page,
            "trait-implementations",
            "Trait Implementations",
            traits,
            origin,
        )?;
    }
    Ok(())
}

fn collections_section(page: &Page, data_origin: &str) -> Option<&'static str> {
    const OWNER: &str = "type.impl/alloc/collections/btree/map/struct.BTreeMap.js";
    if data_origin != OWNER && !data_origin.ends_with(&format!("/{OWNER}")) {
        return None;
    }
    // These native versions copy BTreeMap::insert's relative documentation link
    // into alias data. Its section belongs to std::collections, not alloc's map
    // module or the capture root. Qualify the native owner and standard-doc root.
    let (root, section) = match page.generator.as_str() {
        "1.94.0 (4a4ef493e 2026-03-02)" => (
            "1.94.0",
            "https://doc.rust-lang.org/1.94.0/std/collections/index.html#insert-and-complex-keys",
        ),
        "1.96.0-nightly (e370b60cf 2026-03-07)" => (
            "nightly",
            "https://doc.rust-lang.org/nightly/std/collections/index.html#insert-and-complex-keys",
        ),
        _ => return None,
    };
    page.hrefs
        .contains(&format!(
            "https://doc.rust-lang.org/{root}/alloc/collections/btree/map/struct.BTreeMap.html"
        ))
        .then_some(section)
}

fn node_id(node: &Node) -> Option<&str> {
    match node {
        Node::Start { attributes, .. } => attributes.get("id").map(String::as_str),
        _ => None,
    }
}

fn insert_group(
    page: &mut Page,
    id: &str,
    title: &str,
    nodes: Vec<Node>,
    origin: &str,
) -> Result<(), Error> {
    let list = format!("{id}-list");
    if let Some(start) = page
        .nodes
        .iter()
        .position(|node| node_id(node) == Some(&list))
    {
        if !matches!(&page.nodes[start],Node::Start{name,..} if name=="div") {
            return Err(invalid(origin, "invalid Rustdoc implementation group"));
        }
        let mut depth = 0usize;
        for end in start..page.nodes.len() {
            match &page.nodes[end] {
                Node::Start { name, .. } if !super::void(name) => depth += 1,
                Node::End(_) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| invalid(origin, "invalid Rustdoc implementation group"))?;
                    if depth == 0 {
                        page.nodes.splice(end..end, nodes);
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        return Err(invalid(origin, "unclosed Rustdoc implementation group"));
    }
    if !page.anchors.insert(id.into()) || !page.anchors.insert(list.clone()) {
        return Err(invalid(origin, "conflicting Rustdoc implementation group"));
    }
    let mut group = vec![
        Node::Start {
            name: "h2".into(),
            attributes: BTreeMap::from([("id".into(), id.into())]),
        },
        Node::Text(title.into()),
        Node::End("h2".into()),
        Node::Start {
            name: "div".into(),
            attributes: BTreeMap::from([("id".into(), list)]),
        },
    ];
    group.extend(nodes);
    group.push(Node::End("div".into()));
    let last = page
        .nodes
        .len()
        .checked_sub(1)
        .ok_or_else(|| invalid(origin, "empty Rustdoc alias page"))?;
    let position = if id == "implementations" {
        page.nodes
            .iter()
            .position(|node| node_id(node) == Some("trait-implementations"))
            .unwrap_or(last)
    } else {
        last
    };
    page.nodes.splice(position..position, group);
    Ok(())
}
