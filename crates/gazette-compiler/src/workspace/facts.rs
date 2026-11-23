//! Validate source-selection metadata without acquiring or duplicating maintained facts.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::publication::{
    Principal,
    facts::{Selection, Source, Unavailable},
};

use super::{Diagnostic, Key, Workspace};

pub(super) fn validate(
    selections: &mut [Selection],
    principals: &BTreeMap<Key, Principal>,
    errors: &mut Vec<Diagnostic>,
) {
    selections.sort();
    let mut targets = BTreeSet::new();
    for selection in selections {
        let principal = selection.target.principal();
        let mut report = |code| {
            errors.push(Diagnostic {
                code,
                field: "fact_sources".into(),
                key: Some(principal.clone()),
            })
        };
        if !targets.insert(selection.target.clone()) {
            report("workspace_fact_source_duplicate");
        }
        let Some(record) = principals.get(principal) else {
            report("workspace_fact_source_principal");
            continue;
        };
        match &selection.source {
            Source::Local if record.name.is_none() => report("workspace_fact_source_local_missing"),
            Source::External { .. } if record.name.is_some() => {
                report("workspace_fact_source_conflict")
            }
            _ => (),
        }
    }
}

impl Workspace {
    /// Explicit source choices in stable target/source order; values stay in native catalog fields.
    pub fn fact_sources(&self) -> &[Selection] {
        &self.fact_sources
    }

    /// Require a fully local fact set until frozen external capture consumption is admitted.
    ///
    /// Structural configuration inspection may retain unresolved selections. Publication
    /// consumers cannot: availability is not resolution and no success capture is invented.
    /// This pure guard neither calls a provider nor reads mutable external state.
    ///
    /// # Errors
    /// Returns the first exact external selection in stable order, without fallback.
    pub fn require_resolved_facts(&self) -> Result<(), Unavailable> {
        for selection in &self.fact_sources {
            if let Source::External { binding } = &selection.source {
                return Err(Unavailable {
                    target: selection.target.clone(),
                    binding: binding.clone(),
                });
            }
        }
        Ok(())
    }
}
