//! Publication-owned feed selection, independent of wire rendering and authored facts.

use gazette_core::publication::routing::{PublicBaseUrl, RoutePath};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::Key;

/// Explicit syndication namespace and channels for one publication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Feeds {
    /// Stable HTTP(S) identity namespace, not the mutable serving address.
    /// Keep it unchanged across route, feed-path and deployment changes.
    pub identity_base: PublicBaseUrl,
    /// Explicit publication-wide article filtering, layered over each selected graph scope.
    /// Omission retains the channel's complete explicit structural selection.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub selection: Option<FeedSelection>,
    /// Omit every format's file when the fully validated, filtered selection is empty.
    /// False retains standards-compliant empty output and its format-required fields.
    #[serde(default)]
    pub omit_when_empty: bool,
    /// Exact output selections; no default feed, title, dates or authors are inferred.
    #[serde(deserialize_with = "crate::tables::many")]
    pub channels: Vec<FeedConfiguration>,
}

/// One output over an existing channel object and its selected content graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedConfiguration {
    /// Stable feed key, separate from both its source channel and output address.
    pub id: Key,
    /// Existing same-site content supplying title, description and editorial metadata.
    pub channel: Key,
    /// Explicit file route beneath the site's configured public base.
    pub path: RoutePath,
    /// Selected standards-based wire representation.
    pub format: FeedFormat,
    /// Exact graph selection; the channel itself is never repeated as an entry.
    /// Omission requires an explicit publication selector and uses its same-site candidate set.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::tables::optional"
    )]
    pub scope: Option<FeedScope>,
}

/// Publication-owned filtering over the selected native graph, not a second content catalogue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedSelection {
    /// Explicit native page family selected for syndication.
    pub kind: FeedSelectionKind,
    /// Explicit residence policy, independent of the channel's traversal scope.
    pub residence: FeedResidence,
    /// Nonempty distinct authored content states; file dates are never consulted.
    pub lifecycle: Vec<gazette_core::publication::ContentLifecycle>,
    /// Require an authored publication date, preserving calendar/timestamp precision.
    pub require_published_date: bool,
}

/// Closed native selection families; no subject- or repository-derived classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FeedSelectionKind {
    /// Ordinary article-family pages only, not container landings or other page families.
    Articles,
}

/// Explicit article residence policy, independent from editorial traversal order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FeedResidence {
    /// Standalone and contained articles; their one editorial home remains unchanged.
    All,
}

/// Supported wire projections of the same retained publication values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FeedFormat {
    /// Atom 1.0.
    Atom,
    /// RSS 2.0.
    Rss,
    /// JSON Feed 1.1.
    Json,
}

/// Membership and traversal remain distinct from lifecycle/date-based filtering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FeedScope {
    /// Every other same-site page in canonical URL order.
    Site {},
    /// The channel's direct structural children and collection members.
    Collection {
        /// Include descendant collections, journeys and their editorial members without duplicates.
        recursive: bool,
    },
    /// The channel journey's complete authored traversal order.
    Journey {},
}

pub(super) fn validate(
    publication: &mut super::BrandPublication,
    errors: &mut Vec<super::Diagnostic>,
    identities: &mut BTreeMap<String, BTreeSet<Key>>,
) {
    let Some(feeds) = &mut publication.machine.feeds else {
        return;
    };
    let ids = identities
        .entry(feeds.identity_base.as_str().to_owned())
        .or_default();
    let mut paths = BTreeSet::new();
    if let Some(selection) = &feeds.selection {
        let mut states = Vec::new();
        if selection.lifecycle.is_empty()
            || selection.lifecycle.iter().any(|state| {
                if states.contains(state) {
                    true
                } else {
                    states.push(*state);
                    false
                }
            })
        {
            errors.push(super::Diagnostic {
                code: "workspace_feed_lifecycle",
                field: "brands.publications.machine.feeds.selection.lifecycle".into(),
                key: Some(publication.site.clone()),
            });
        }
    }
    if feeds.channels.is_empty() {
        errors.push(super::Diagnostic {
            code: "workspace_feeds_empty",
            field: "brands.publications.machine.feeds.channels".into(),
            key: Some(publication.site.clone()),
        });
    }
    for feed in &feeds.channels {
        if feed.scope.is_none() && feeds.selection.is_none() {
            errors.push(super::Diagnostic {
                code: "workspace_feed_scope_required",
                field: "brands.publications.machine.feeds.channels.scope".into(),
                key: Some(feed.id.clone()),
            });
        }
        let code = if !ids.insert(feed.id.clone()) || !paths.insert(feed.path.output()) {
            Some("workspace_feed_duplicate")
        } else if feed.path.as_str().ends_with('/') {
            Some("workspace_feed_path")
        } else {
            None
        };
        if let Some(code) = code {
            errors.push(super::Diagnostic {
                code,
                field: "brands.publications.machine.feeds.channels".into(),
                key: Some(feed.id.clone()),
            });
        }
    }
    feeds.channels.sort_by(|a, b| a.id.cmp(&b.id));
}
