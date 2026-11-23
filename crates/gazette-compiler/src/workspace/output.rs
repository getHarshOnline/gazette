//! Private destination and local-address configuration, independent of public meaning and effects.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU16,
};

use gazette_core::publication::routing::PublicBaseUrl;
use serde::{Deserialize, Serialize};

use super::{Key, SourceRoot, Workspace};

/// One configured output location. Resolving it neither writes nor replaces files.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationOutput {
    /// Explicit absolute or confined workspace-relative directory.
    /// Relative coordinates require the selected native workspace, never the process directory.
    pub directory: SourceRoot,
}

impl Workspace {
    /// Construct exact loopback addresses for a selected local site set without opening listeners.
    ///
    /// Public URL prefixes are retained. This changes no configured public identity and grants
    /// neither native reads nor serving, output, release or publication permission.
    ///
    /// # Errors
    /// Refuses empty/duplicate/unknown sites, missing publication addresses or local ports,
    /// and duplicate ports in the selected set. No partial or public-address fallback is returned.
    pub fn local_urls(&self, sites: &[Key]) -> Result<BTreeMap<Key, PublicBaseUrl>, &'static str> {
        if sites.is_empty() {
            return Err("local addressing requires a nonempty explicit site set");
        }
        let mut identities = BTreeSet::new();
        let mut ports: BTreeSet<NonZeroU16> = BTreeSet::new();
        let mut urls = BTreeMap::new();
        for site in sites {
            if !identities.insert(site) {
                return Err("duplicate site in local addressing selection");
            }
            if self.site(site).is_none() {
                return Err("local addressing names an undeclared site");
            }
            let publication = self
                .publication_for(site)
                .ok_or("local addressing requires the selected publication binding")?;
            let port = publication.local_port.ok_or(
                "local addressing requires an explicit local port for every selected site",
            )?;
            if !ports.insert(port) {
                return Err("selected local publications have conflicting ports");
            }
            let public = publication
                .public_base_url
                .as_ref()
                .ok_or("local addressing requires the configured public base and prefix")?;
            let public = url::Url::parse(public.as_str())
                .map_err(|_| "configured public base could not be projected")?;
            let local = format!("http://127.0.0.1:{}{}", port.get(), public.path())
                .parse()
                .map_err(|_| "configured public prefix could not be projected locally")?;
            urls.insert(site.clone(), local);
        }
        Ok(urls)
    }
}
