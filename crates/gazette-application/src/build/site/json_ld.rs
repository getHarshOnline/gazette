//! Join page and listing JSON-LD in their emitted order.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// Transport only: borrow exact page-model values in the shared canonical order.
#[derive(Serialize)]
pub(super) struct SiteJsonLd<'a> {
    #[serde(rename = "@context", skip_serializing_if = "Option::is_none")]
    context: Option<&'a Value>,
    #[serde(rename = "@graph")]
    graph: Vec<&'a Value>,
}

impl<'a> SiteJsonLd<'a> {
    pub(super) fn from_documents(graph: Vec<&'a Value>) -> Self {
        let context = graph
            .iter()
            .find(|value| value.get("@graph").is_some())
            .and_then(|value| value.get("@context"));
        let mut described = BTreeMap::new();
        let graph = graph
            .into_iter()
            .flat_map(|value| {
                value
                    .get("@graph")
                    .and_then(Value::as_array)
                    .map_or_else(|| std::slice::from_ref(value), Vec::as_slice)
            })
            .filter(|node| {
                let Some(identity) = node.get("@id").and_then(Value::as_str) else {
                    return true;
                };
                match described.entry(identity) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(*node);
                        true
                    }
                    std::collections::btree_map::Entry::Occupied(entry) => *entry.get() != *node,
                }
            })
            .collect();
        Self { context, graph }
    }
}
