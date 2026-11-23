//! Reading projections omit explicitly selected presentation payloads, not hidden CSS.

use std::collections::{BTreeMap, BTreeSet};

use comrak::{Node, arena_tree::NodeEdge, nodes::NodeValue};
use gazette_core::{markdown::Error, source::Span};

use crate::{plan::Bindings, source::Lines};

#[derive(Default)]
pub(crate) struct Selection {
    components: BTreeSet<Span>,
    notes: BTreeMap<String, usize>,
}

impl Selection {
    pub(crate) fn new(
        root: Node<'_>,
        lines: &Lines<'_>,
        bindings: &Bindings<'_>,
    ) -> Result<Self, Error> {
        let mut selected = Self {
            components: bindings
                .components
                .iter()
                .filter(|(_, binding)| binding.presentation_only)
                .map(|(span, _)| *span)
                .collect(),
            ..Self::default()
        };
        if selected.components.is_empty() {
            return Ok(selected);
        }
        // A note belongs to the reading projection only when a retained reference
        // reaches it. A reference inside an omitted payload cannot keep it alive.
        let mut references = BTreeMap::<Option<String>, Vec<String>>::new();
        let mut owner = None;
        let mut skipped = None;
        for edge in root.traverse() {
            let (node, entering) = match edge {
                NodeEdge::Start(node) => (node, true),
                NodeEdge::End(node) => (node, false),
            };
            if let Some(parent) = skipped {
                if !entering && std::ptr::eq(node, parent) {
                    skipped = None;
                }
                continue;
            }
            if entering && selected.component(node, lines)? {
                skipped = Some(node);
                continue;
            }
            match &node.data().value {
                NodeValue::FootnoteDefinition(note) => {
                    owner = entering.then(|| note.name.clone());
                }
                NodeValue::FootnoteReference(note) if entering => {
                    references
                        .entry(owner.clone())
                        .or_default()
                        .push(note.name.clone());
                }
                _ => {}
            }
        }
        let mut pending = vec![None];
        let mut visited = BTreeSet::new();
        while let Some(owner) = pending.pop() {
            if let Some(targets) = references.get(&owner) {
                for target in targets {
                    *selected.notes.entry(target.clone()).or_default() += 1;
                    if visited.insert(target.clone()) {
                        pending.push(Some(target.clone()));
                    }
                }
            }
        }
        Ok(selected)
    }

    fn component(&self, node: Node<'_>, lines: &Lines<'_>) -> Result<bool, Error> {
        let data = node.data();
        Ok(matches!(data.value, NodeValue::BlockDirective(_))
            && self
                .components
                .contains(&lines.location(data.sourcepos)?.span))
    }

    pub(crate) fn omits(&self, node: Node<'_>, lines: &Lines<'_>) -> Result<bool, Error> {
        if self.components.is_empty() {
            return Ok(false);
        }
        if self.component(node, lines)? {
            return Ok(true);
        }
        Ok(
            matches!(&node.data().value, NodeValue::FootnoteDefinition(note)
            if !self.notes.contains_key(&note.name)),
        )
    }

    pub(crate) fn prune(&self, root: Node<'_>, lines: &Lines<'_>) -> Result<(), Error> {
        if self.components.is_empty() {
            return Ok(());
        }
        // Admission and all source-bound bindings were checked before this step.
        // Only this render's fresh tree changes; retained syntax and HTML do not.
        let omitted = root
            .descendants()
            .filter_map(|node| match self.omits(node, lines) {
                Ok(true) => Some(Ok(node)),
                Ok(false) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for node in omitted {
            node.detach();
        }
        let mut counts = BTreeMap::<String, u32>::new();
        for node in root.descendants() {
            match &mut node.data_mut().value {
                NodeValue::FootnoteDefinition(note) => {
                    note.total_references = self.notes.get(&note.name).copied().unwrap_or(0) as u32;
                }
                NodeValue::FootnoteReference(note) => {
                    let count = counts.entry(note.name.clone()).or_default();
                    *count += 1;
                    note.ref_num = *count;
                }
                _ => {}
            }
        }
        Ok(())
    }
}
