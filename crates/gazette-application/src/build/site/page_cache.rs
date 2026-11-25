//! Release identity and eligible public reading bytes from the finished page inventory.

use std::collections::BTreeMap;

use gazette_compiler::{
    authored::{Discovery, SitePages},
    workspace::PageCache,
};
use gazette_core::source::{EntryPath, Sha256Digest};
use serde::Serialize;

use super::{SiteRenderError, encode};

#[derive(Serialize)]
struct Page {
    bytes: usize,
    sha256: Sha256Digest,
}

pub(super) fn path() -> Result<EntryPath, SiteRenderError> {
    "page-cache.json".parse().map_err(SiteRenderError::Output)
}

pub(super) fn descriptor(
    selection: &SitePages<'_>,
    discovery: &Discovery<'_>,
    files: &BTreeMap<EntryPath, Vec<u8>>,
    policy: PageCache,
    maximum: usize,
) -> Result<Vec<u8>, SiteRenderError> {
    if policy.retire {
        return encode(
            &serde_json::json!({"version": 1, "retire": true}),
            maximum,
            "page cache retirement bytes",
        )
        .map_err(SiteRenderError::Render);
    }
    let publication = selection.publication();
    if policy.page_bytes == 0
        || policy.page_bytes > policy.warm_bytes
        || policy.warm_bytes > policy.storage_bytes
        || policy.warm_pages == 0
    {
        return Err(super::super::deployment::invalid(
            "invalid page cache limits",
        ));
    }
    let mut pages = BTreeMap::new();
    let legal = publication
        .legal
        .iter()
        .map(|binding| &binding.id)
        .collect::<std::collections::BTreeSet<_>>();
    for page in selection.pages() {
        if !discovery.contains(&page.node.id)
            || legal.contains(&page.node.id)
            || publication.search_page.as_ref() == Some(&page.node.id)
        {
            continue;
        }
        let Some(route) = &page.node.route else {
            continue;
        };
        if route.canonical.is_some()
            || publication
                .machine
                .terms
                .as_ref()
                .is_some_and(|terms| terms.path == route.path.as_str())
        {
            continue;
        }
        let Some(bytes) = files.get(route.path.output()) else {
            continue;
        };
        if bytes.len() as u64 <= u64::from(policy.page_bytes) {
            pages.insert(
                route.path.as_str(),
                Page {
                    bytes: bytes.len(),
                    sha256: Sha256Digest::of(bytes),
                },
            );
        }
    }
    let body = encode(&(policy, &pages), maximum, "page cache descriptor bytes")
        .map_err(SiteRenderError::Render)?;
    encode(
        &serde_json::json!({
            "version": 1,
            "release": Sha256Digest::of(&body),
            "policy": policy,
            "pages": pages,
        }),
        maximum,
        "page cache descriptor bytes",
    )
    .map_err(SiteRenderError::Render)
}
