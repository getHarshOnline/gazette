//! Collection display order only reorders existing direct structural children.

use std::collections::BTreeMap;

use super::{ContentKind, ContentNode, Diagnostic, Key};

pub(super) fn apply(
    nodes: &BTreeMap<Key, ContentNode>,
    residences: &BTreeMap<Key, Key>,
    children: &mut BTreeMap<Key, Vec<Key>>,
    errors: &mut Vec<Diagnostic>,
) {
    for owner in nodes.values() {
        let ContentKind::Collection { member_order, .. } = &owner.structure else {
            continue;
        };
        let mut positions = BTreeMap::new();
        for (position, member) in member_order.iter().enumerate() {
            let code = if !nodes.contains_key(member) {
                Some("collection_order_reference")
            } else if residences.get(member) != Some(&owner.id) {
                Some("collection_order_not_child")
            } else {
                None
            };
            if let Some(code) = code {
                errors.push(Diagnostic {
                    code,
                    source: owner.id.clone(),
                    target: Some(member.clone()),
                });
            }
            if positions.insert(member, position).is_some() {
                errors.push(Diagnostic {
                    code: "collection_order_duplicate",
                    source: owner.id.clone(),
                    target: Some(member.clone()),
                });
            }
        }
        if let Some(children) = children.get_mut(&owner.id) {
            children.sort_by(|left, right| {
                positions
                    .get(left)
                    .unwrap_or(&usize::MAX)
                    .cmp(positions.get(right).unwrap_or(&usize::MAX))
                    .then_with(|| left.cmp(right))
            });
        }
    }
}
