//! Deterministic GitHub Pages files, without deployment or account effects.

use gazette_core::{
    hosting::HostingProvider,
    publication::routing::PublicBaseUrl,
    source::{self, CapturedSource, Content, InventoryBuilder, Sha256Digest},
};
use std::collections::BTreeMap;

/// Native composition's pure GitHub Pages file provider.
pub struct HostFiles;

impl gazette_ports::hosting::HostFiles for HostFiles {
    fn generate(
        &self,
        provider: HostingProvider,
        public_base: &PublicBaseUrl,
        limits: source::Limits,
    ) -> Result<CapturedSource, source::Error> {
        let HostingProvider::GithubPages = provider;
        let domain = public_base
            .domain()
            .filter(|domain| domain.contains('.') && !domain.ends_with(".localhost"))
            .ok_or_else(|| source::Error::InvalidInventory {
                path: "CNAME".into(),
                reason: "GitHub Pages hosting requires a production DNS domain",
            })?;
        let mut files: BTreeMap<source::EntryPath, Vec<u8>> =
            BTreeMap::from([(".nojekyll".parse()?, Vec::new())]);
        // GitHub's default address needs no custom-domain declaration.
        if !domain.ends_with(".github.io") {
            files.insert("CNAME".parse()?, format!("{domain}\n").into_bytes());
        }
        let mut inventory = InventoryBuilder::new(limits);
        for (path, bytes) in &files {
            inventory.insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )?;
        }
        CapturedSource::from_parts(inventory.finish(), files)
    }
}
