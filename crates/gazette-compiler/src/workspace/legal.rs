//! Explicit owner-held legal sources and their independent publication outputs.

use std::collections::{BTreeMap, BTreeSet};

use gazette_core::{publication::routing::RoutePath, source::EntryPath};
use serde::{Deserialize, Serialize};

use super::{Brand, Diagnostic, Key, Source, resolve::report};

/// One retained legal source selected for one explicitly identified site output.
/// Source identity and entry never supply a site, route, controller or approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LegalBinding {
    /// Exact configured capture source, independently of its native location.
    pub source: Key,
    /// Confined capture-relative document entry; not a public or corpus route.
    pub entry: EntryPath,
    /// Expected identity in the retained owner's reduced source metadata.
    pub source_content: Key,
    /// Explicit identity of this publication's composed legal page.
    pub id: Key,
    /// Explicit site-relative output route, without a filename-derived default.
    pub route: RoutePath,
    /// Released old addresses on the same site that redirect to `route`, with
    /// the same meaning and whole-graph checks as an authored route's aliases.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<RoutePath>,
    /// Closed output purpose; legal composition cannot create an article or container.
    pub family: LegalFamily,
}

/// Legal-source composition supplies only ordinary Utility pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LegalFamily {
    /// Structural legal copy, not an article, collection or journey.
    Utility,
}

pub(super) fn validate(
    brands: &BTreeMap<Key, Brand>,
    sources: &BTreeMap<Key, Source>,
    errors: &mut Vec<Diagnostic>,
) {
    let mut output_ids = BTreeSet::new();
    for publication in brands.values().flat_map(|brand| &brand.publications) {
        let field = format!("brands.publications[{:?}].legal", publication.site.as_str());
        let mut bindings = BTreeSet::new();
        for binding in &publication.legal {
            if !sources.contains_key(&binding.source) {
                report(
                    errors,
                    "workspace_reference",
                    &format!("{field}.source"),
                    Some(&binding.source),
                );
            }
            if !output_ids.insert(&binding.id) {
                report(
                    errors,
                    "workspace_duplicate",
                    &format!("{field}.id"),
                    Some(&binding.id),
                );
            }
            // Across sites this is deliberate reuse. Within one publication a
            // second selection of the same exact source entry is not another body.
            if !bindings.insert((&binding.source, &binding.entry)) {
                report(
                    errors,
                    "workspace_legal_binding_duplicate",
                    &format!("{field}.entry"),
                    Some(&binding.id),
                );
            }
        }
    }
}
