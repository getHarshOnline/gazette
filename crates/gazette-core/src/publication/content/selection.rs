//! Consuming declaration-owner selection, not a publication-policy verdict.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Declaration, Diagnostic, Error, Graph, Key, Limits};

/// An explicit selection for an existing object. Neither value attests rights,
/// freshness, publisher permission or the safety of its nested public fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Disposition {
    /// Retain the declaration and its owned relationships unchanged.
    Include,
    /// Omit the declaration and relationships it owns.
    Exclude,
}

impl Graph {
    /// Consume this graph into one explicitly selected structural model.
    ///
    /// Every original identity needs a disposition. Memberships belong to their
    /// leaf, editorial edges to their source, and project declarations to their
    /// declaring content. Selected containers retain their original structure;
    /// journeys are never shortened. The existing resolver recomputes inherited
    /// context and primary journeys, without retaining excluded evidence.
    /// This operation selects declarations; it does not establish public admission.
    ///
    /// # Errors
    /// Rejects missing/unknown decisions, selected references to excluded objects,
    /// or limits. No partially selected graph is returned.
    pub fn into_selected(
        self,
        decisions: &BTreeMap<Key, Disposition>,
        limits: Limits,
    ) -> Result<Self, Error> {
        if decisions.len() > limits.nodes {
            return Err(Error::Limit("selection decisions"));
        }
        let mut errors: Vec<_> = self
            .nodes
            .keys()
            .filter(|id| !decisions.contains_key(*id))
            .map(|id| Diagnostic {
                code: "content_selection_missing",
                source: id.clone(),
                target: None,
            })
            .chain(
                decisions
                    .keys()
                    .filter(|id| !self.nodes.contains_key(*id))
                    .map(|id| Diagnostic {
                        code: "content_selection_unknown",
                        source: id.clone(),
                        target: None,
                    }),
            )
            .collect();
        if !errors.is_empty() {
            errors.sort();
            return Err(Error::Invalid(errors));
        }
        let included = |id: &Key| decisions.get(id) == Some(&Disposition::Include);
        Self::resolve(
            Declaration {
                nodes: self
                    .nodes
                    .into_values()
                    .filter(|node| included(&node.id))
                    .collect(),
                memberships: self
                    .memberships
                    .into_iter()
                    .filter(|edge| included(&edge.content))
                    .collect(),
                related: self
                    .related
                    .into_iter()
                    .filter(|edge| included(&edge.source))
                    .collect(),
                browsing: self
                    .browsing
                    .into_iter()
                    .filter(|edge| included(&edge.source))
                    .collect(),
                projects: self
                    .projects
                    .into_iter()
                    .filter(|edge| included(&edge.content))
                    .collect(),
            },
            limits,
        )
    }
}
