//! Reader-initiated source preferences resolved from publication policy.
//!
//! The provider receives the canonical publication host, never a local preview
//! address or an individual article URL. Exact content overrides take priority
//! over the article default; disabling the publication suppresses every invitation.
//! Workspace compilation validates copy and source scope; authored resolution
//! validates page references. These types neither load a provider SDK nor observe
//! account state, publisher eligibility, or completion of the external action.

use std::collections::BTreeMap;

use gazette_core::publication::{
    content::{ContentKind, ContentNode, PageFamily},
    routing::PublicBaseUrl,
};
use serde::{Deserialize, Serialize};
use url::{Host, Url};

use super::{Diagnostic, Key, Workspace, resolve::report};

/// Publication policy for an optional preferred-source invitation.
///
/// Omission from [`super::PublicationProcessing`] is distinct from an explicitly
/// disabled policy. Workspace validation bounds labels to 160 UTF-8 bytes and
/// descriptions to 1,024, without control characters. Exact page overrides must
/// resolve to routed content in this publication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreferredSourcesSelection {
    /// Explicit invitation enablement; it grants no provider-side status.
    pub enabled: bool,
    /// Provider with a supported reader-initiated preference destination.
    pub provider: PreferredSourcesProvider,
    /// Human-facing invitation; Google receives only the canonical source host.
    pub label: String,
    /// Explanatory copy rendered alongside the action.
    pub description: String,
    /// Include authored articles, including articles in nested collections.
    pub articles: bool,
    /// Exact content identities override the publication default, never route guesses.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pages: BTreeMap<Key, PreferredSourceOverride>,
}

/// Supported preferred-source preference provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PreferredSourcesProvider {
    /// Google Search preferred sources.
    Google,
}

/// Per-content invitation policy overriding the publication defaults.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreferredSourceOverride {
    /// Explicit invitation enablement; it grants no provider-side status.
    pub enabled: bool,
    /// Optional replacement action label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional replacement explanatory copy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Resolved presentation data. A local serving override cannot change this target.
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
pub struct PreferredSourceView {
    /// Canonical provider preference URL.
    pub href: String,
    /// Action label supplied by publication configuration.
    pub label: String,
    /// Explanatory copy rendered alongside the action.
    pub description: String,
}

fn destination(base: &PublicBaseUrl) -> Option<String> {
    let source = Url::parse(base.as_str()).ok()?;
    let Host::Domain(host) = source.host()? else {
        return None;
    };
    if source.path() != "/"
        || source.port().is_some()
        || !host.contains('.')
        || host.ends_with(".localhost")
    {
        return None;
    }
    let mut target = Url::parse("https://www.google.com/preferences/source").ok()?;
    target.query_pairs_mut().append_pair("q", host);
    Some(target.to_string())
}

impl PreferredSourcesSelection {
    pub(crate) fn project(
        &self,
        node: &ContentNode,
        base: &PublicBaseUrl,
    ) -> Option<PreferredSourceView> {
        if !self.enabled {
            return None;
        }
        let page = self.pages.get(&node.id);
        let article = matches!(
            node.structure,
            ContentKind::Page {
                family: PageFamily::Article
            }
        );
        if !page.map_or(self.articles && article, |page| page.enabled) {
            return None;
        }
        Some(PreferredSourceView {
            href: destination(base)?,
            label: page
                .and_then(|p| p.label.as_ref())
                .unwrap_or(&self.label)
                .clone(),
            description: page
                .and_then(|p| p.description.as_ref())
                .unwrap_or(&self.description)
                .clone(),
        })
    }
}

pub(super) fn validate(workspace: &Workspace, errors: &mut Vec<Diagnostic>) {
    fn text_valid(text: &str, max: usize) -> bool {
        !text.trim().is_empty() && text.len() <= max && !text.chars().any(char::is_control)
    }
    for brand in workspace.brands() {
        for publication in &brand.publications {
            let Some(selection) = publication
                .processing
                .as_ref()
                .and_then(|p| p.preferred_sources.as_ref())
            else {
                continue;
            };
            let field = format!(
                "brands[{:?}].publications[{:?}].processing.preferred_sources",
                brand.id.as_str(),
                publication.site.as_str()
            );
            if !text_valid(&selection.label, 160)
                || !text_valid(&selection.description, 1024)
                || selection.pages.len() > 1024
                || selection.pages.values().any(|p| {
                    p.label.as_ref().is_some_and(|s| !text_valid(s, 160))
                        || p.description.as_ref().is_some_and(|s| !text_valid(s, 1024))
                })
            {
                report(errors, "workspace_preferred_sources_copy", &field, None);
            }
            if selection.enabled
                && publication
                    .public_base_url
                    .as_ref()
                    .is_none_or(|base| destination(base).is_none())
            {
                report(errors, "workspace_preferred_sources_origin", &field, None);
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/preferred_sources.rs"]
mod tests;
