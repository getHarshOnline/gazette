//! Authored documentation order is a view over containment, never another residence.

use super::{ContentKind, ContentNode, Diagnostic, Error, Key, Limits};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One exact content destination or a named group of ordered destinations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum NavigationItem {
    /// A container expands its own complete structural subtree.
    Content {
        /// Existing content identity within the owning collection's subtree.
        content: Key,
    },
    /// A group supplies a label and ordering, not a content identity or parent.
    Group {
        /// Authored plain-text group label.
        group: String,
        /// Ordered destinations and nested groups.
        pages: Vec<NavigationItem>,
    },
}

impl<'de> Deserialize<'de> for NavigationItem {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        use serde::de::{Error as _, MapAccess, Visitor};
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "lowercase")]
        enum Field {
            Content,
            Group,
            Pages,
        }
        struct Named;
        impl<'de> Visitor<'de> for Named {
            type Value = NavigationItem;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a content destination or named navigation group")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let (mut content, mut group, mut pages) = (None, None, None);
                while let Some(field) = map.next_key::<Field>()? {
                    match field {
                        Field::Content => {
                            if content.is_some() {
                                return Err(A::Error::duplicate_field("content"));
                            }
                            content = Some(map.next_value::<Key>()?);
                        }
                        Field::Group => {
                            if group.is_some() {
                                return Err(A::Error::duplicate_field("group"));
                            }
                            group = Some(map.next_value::<String>()?);
                        }
                        Field::Pages => {
                            if pages.is_some() {
                                return Err(A::Error::duplicate_field("pages"));
                            }
                            pages = Some(map.next_value::<Vec<NavigationItem>>()?);
                        }
                    }
                }
                if let Some(content) = content {
                    if group.is_some() || pages.is_some() {
                        return Err(A::Error::custom("content destination accepts only content"));
                    }
                    return Ok(NavigationItem::Content { content });
                }
                Ok(NavigationItem::Group {
                    group: group.ok_or_else(|| A::Error::missing_field("group"))?,
                    pages: pages.ok_or_else(|| A::Error::missing_field("pages"))?,
                })
            }
        }
        decoder.deserialize_map(Named)
    }
}

pub(super) fn charge(
    items: &[NavigationItem],
    edges: &mut usize,
    text: &mut usize,
    limits: Limits,
) -> Result<(), Error> {
    let mut stack: Vec<_> = items.iter().map(|item| (item, 0usize)).collect();
    while let Some((item, depth)) = stack.pop() {
        if depth > 64 {
            return Err(Error::Limit("documentation navigation depth"));
        }
        super::resolve::charge(edges, 1, limits.edges, "edges")?;
        let value = match item {
            NavigationItem::Content { content } => content.as_str(),
            NavigationItem::Group { group, pages } => {
                stack.extend(pages.iter().map(|item| (item, depth + 1)));
                group
            }
        };
        super::resolve::charge(text, value.len(), limits.text_bytes, "text bytes")?;
    }
    Ok(())
}

pub(super) fn validate(
    nodes: &BTreeMap<Key, ContentNode>,
    children: &BTreeMap<Key, Vec<Key>>,
    documentation: &BTreeSet<Key>,
    limits: Limits,
    errors: &mut Vec<Diagnostic>,
) -> Result<(), Error> {
    enum Step<'a> {
        Item(&'a NavigationItem),
        Content(&'a Key),
    }
    let mut remaining = limits.edges;
    for owner in nodes.values() {
        let ContentKind::Collection {
            navigation: Some(items),
            ..
        } = &owner.structure
        else {
            continue;
        };
        let report = |errors: &mut Vec<Diagnostic>, code, target: Option<&Key>| {
            super::resolve::report(errors, code, &owner.id, target)
        };
        if !documentation.contains(&owner.id) {
            report(errors, "collection_navigation_purpose", None);
        }
        if items.is_empty() {
            report(errors, "collection_navigation_empty", None);
        }
        let mut expected = BTreeSet::new();
        let mut descendants: Vec<_> = children.get(&owner.id).into_iter().flatten().collect();
        while let Some(id) = descendants.pop() {
            remaining = remaining
                .checked_sub(1)
                .ok_or(Error::Limit("documentation navigation traversal"))?;
            expected.insert(id);
            descendants.extend(children.get(id).into_iter().flatten());
        }
        let mut seen = BTreeSet::new();
        let mut stack: Vec<_> = items.iter().rev().map(Step::Item).collect();
        while let Some(step) = stack.pop() {
            remaining = remaining
                .checked_sub(1)
                .ok_or(Error::Limit("documentation navigation traversal"))?;
            match step {
                Step::Item(NavigationItem::Group { group, pages }) => {
                    if group.trim().is_empty() || group.chars().any(char::is_control) {
                        report(errors, "collection_navigation_label", None);
                    }
                    if pages.is_empty() {
                        report(errors, "collection_navigation_empty", None);
                    }
                    stack.extend(pages.iter().rev().map(Step::Item));
                }
                Step::Item(NavigationItem::Content { content }) => {
                    stack.push(Step::Content(content))
                }
                Step::Content(id) => {
                    if !nodes.contains_key(id) {
                        report(errors, "collection_navigation_reference", Some(id));
                        continue;
                    }
                    if !expected.contains(id) {
                        report(errors, "collection_navigation_outside", Some(id));
                        continue;
                    }
                    if !seen.insert(id) {
                        report(errors, "collection_navigation_duplicate", Some(id));
                        continue;
                    }
                    match &nodes[id].structure {
                        ContentKind::Collection {
                            navigation: Some(items),
                            ..
                        } => stack.extend(items.iter().rev().map(Step::Item)),
                        ContentKind::Journey { members, .. } => stack.extend(
                            members
                                .iter()
                                .rev()
                                .map(|member| Step::Content(&member.content)),
                        ),
                        _ => stack.extend(
                            children
                                .get(id)
                                .into_iter()
                                .flatten()
                                .rev()
                                .map(Step::Content),
                        ),
                    }
                }
            }
        }
        for id in expected.difference(&seen) {
            report(errors, "collection_navigation_missing", Some(id));
        }
    }
    Ok(())
}
