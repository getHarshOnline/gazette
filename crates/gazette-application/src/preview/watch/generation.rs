//! Owned complete development generation and content-derived input identity.

use crate::build::{AcquiredPublicationSet, RenderedSite};
use gazette_core::{
    preview::Site,
    publication::{Key, routing::PublicBaseUrl},
    source::{Sha256Digest, TreeIdentity},
};
use serde::Serialize;
use std::collections::BTreeMap;

/// Content identity for change detection, not proof of publication permission.
/// Exact baseline/revision and development observations remain in `Generation::inputs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct InputIdentity {
    /// Exact fixed workspace request bytes.
    pub workspace: Sha256Digest,
    /// Current retained source trees, including presentation and unselected-site content.
    pub sources: BTreeMap<Key, TreeIdentity>,
    /// Independent documentation captures keyed by consuming site and configured source.
    /// Derived output identities are excluded from pre-generation change detection.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub documentation: BTreeMap<Key, BTreeMap<Key, TreeIdentity>>,
}

impl InputIdentity {
    /// Project the exact retained workspace and source identities without reopening inputs.
    pub fn of(inputs: &AcquiredPublicationSet) -> Self {
        Self {
            workspace: inputs.compilation().source_sha256,
            documentation: inputs
                .sites()
                .iter()
                .filter_map(|(site, input)| {
                    input
                        .as_ref()
                        .ok()?
                        .documentation_catalog()
                        .map(|catalog| (site.clone(), catalog.source_identities().clone()))
                })
                .collect(),
            sources: inputs
                .sources()
                .iter()
                .map(|(key, input)| (key.clone(), input.inventory().identity()))
                .collect(),
        }
    }
}

/// One complete owned development generation. Output borrows end before replacement;
/// no self-reference, payload cloning, registry override or mutable source read is needed.
pub struct Generation {
    pub(super) inputs: AcquiredPublicationSet,
    pub(super) sites: BTreeMap<Key, RenderedSite>,
    pub(super) urls: BTreeMap<Key, PublicBaseUrl>,
    pub(super) identity: InputIdentity,
}

impl Generation {
    /// Exact retained inputs and honest development-source provenance.
    pub fn inputs(&self) -> &AcquiredPublicationSet {
        &self.inputs
    }
    /// Every selected site's complete output and private rendering evidence.
    pub fn sites(&self) -> &BTreeMap<Key, RenderedSite> {
        &self.sites
    }
    /// Effective rendering bases; these are not necessarily local entry addresses.
    pub fn urls(&self) -> &BTreeMap<Key, PublicBaseUrl> {
        &self.urls
    }
    /// Content-derived capture identity; wall-clock polling time is excluded.
    pub fn input_identity(&self) -> &InputIdentity {
        &self.identity
    }

    pub(super) fn output(&self) -> BTreeMap<Key, Site<'_>> {
        self.sites
            .iter()
            .map(|(key, site)| {
                (
                    key.clone(),
                    Site {
                        output: site.output(),
                        media_types: site.media_types(),
                        search_page: site.search_page(),
                        base_url: &self.urls[key],
                    },
                )
            })
            .collect()
    }
}
