//! Site-scoped static transport from retained bytes, never capability or rights admission.

use std::{collections::BTreeMap, fmt};

use gazette_core::{
    publication::{Key, routing::RoutePath},
    source::{EntryPath, Sha256Digest},
};
use serde::Serialize;

use super::{Origin, ResolvedAuthored};
use crate::{corpus::File, workspace::ContentSource};

/// A byte-preserving local asset, with one compiled public address and output file.
/// Filename classification does not validate MIME, payload safety, rights or disclosure.
#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct StaticAsset<'a> {
    /// Absolute address beneath the explicitly configured publication base.
    pub url: String,
    /// Literal file beneath the selected site's output root.
    pub output: EntryPath,
    /// Exact retained bytes, excluding paths, timestamps and remote observations.
    pub sha256: Sha256Digest,
    /// Exact retained byte length, not decoded dimensions or MIME evidence.
    pub bytes: u64,
    /// Existing source role; private coordinates remain excluded from serialization.
    pub provenance: ContentSource<'a>,
    #[serde(skip)]
    #[schemars(skip)]
    source: Origin,
    #[serde(skip)]
    #[schemars(skip)]
    content: &'a [u8],
}

impl StaticAsset<'_> {
    /// Borrow original captured bytes without rereading or transforming native input.
    pub fn content(&self) -> &[u8] {
        self.content
    }
    /// Private source evidence, separate from public asset fields.
    pub fn source(&self) -> &Origin {
        &self.source
    }
}

/// Exact static selection and all remaining opaque compilation obligations for one site.
/// Uncompiled entries must not be omitted from a claimed complete site output.
#[derive(Debug)]
pub struct AssetPlan<'a> {
    entries: BTreeMap<EntryPath, StaticAsset<'a>>,
    uncompiled: Vec<EntryPath>,
    refusals: BTreeMap<EntryPath, crate::notebook::ScanFinding>,
}

impl<'a> AssetPlan<'a> {
    /// Private logical inputs joined to their sole compiled static representation.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = (&EntryPath, &StaticAsset<'a>)> {
        self.entries.iter()
    }
    /// Exact logical lookup; basenames, other sources and native paths are not fallbacks.
    pub fn get(&self, path: &EntryPath) -> Option<&StaticAsset<'a>> {
        self.entries.get(path)
    }
    /// Opaque inputs still requiring selection or a capability compiler.
    pub fn uncompiled(&self) -> &[EntryPath] {
        &self.uncompiled
    }
    /// Why a selected notebook download was withheld; the notebook still renders.
    pub fn download_refusal(&self, path: &EntryPath) -> Option<&crate::notebook::ScanFinding> {
        self.refusals.get(path)
    }
}

/// A complete site-scoped asset mapping could not be constructed.
#[derive(Clone, Debug)]
pub enum AssetError {
    /// Editor metadata cannot supply an output asset.
    Uncaptured(EntryPath),
    /// Requested site is absent from the exact compiled configuration.
    Site(Key),
    /// Asset transport needs the site's explicit public base.
    Base(Key),
    /// A retained entry cannot be mapped by the configured file-route grammar.
    Route(EntryPath),
    /// Derived records or text exceeded their logical limit.
    Limit(&'static str),
}
impl AssetError {
    /// Stable class, shared by direct page projection and whole-site assembly.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Uncaptured(_) => "asset_uncaptured",
            Self::Site(_) => "asset_site_reference",
            Self::Base(_) => "asset_public_base",
            Self::Route(_) => "asset_route",
            Self::Limit(_) => "asset_limit",
        }
    }
}
impl fmt::Display for AssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncaptured(path) => write!(
                f,
                "asset {path} has metadata only; capture its bytes before rendering"
            ),
            Self::Site(site) => write!(f, "unknown asset site {site}; select a configured site"),
            Self::Base(site) => write!(f, "asset site {site} needs an explicit public base"),
            Self::Route(path) => write!(
                f,
                "asset {path} cannot map beneath its configured directory route"
            ),
            Self::Limit(resource) => write!(
                f,
                "asset {resource} ceiling exceeded; no complete plan produced"
            ),
        }
    }
}
impl std::error::Error for AssetError {}

impl<'a> ResolvedAuthored<'a, '_> {
    /// Borrow one lazily compiled asset plan per configured site. A foreign site's
    /// asset failure does not force compilation of that site during local rendering.
    ///
    /// # Errors
    /// Rejects an unknown site, missing required base, invalid routes or exhausted
    /// bounds. Unsupported/unconfigured opaque entries remain explicit obligations.
    pub fn assets_for(&self, site: &Key) -> Result<&AssetPlan<'a>, AssetError> {
        self.assets
            .get(site)
            .ok_or_else(|| AssetError::Site(site.clone()))?
            .get_or_init(|| self.compile_assets(site))
            .as_ref()
            .map_err(Clone::clone)
    }

    fn compile_assets(&self, site: &Key) -> Result<AssetPlan<'a>, AssetError> {
        let workspace = &self.content.compilation().workspace;
        if let Some(path) = self.asset_metadata.keys().find(|path| {
            workspace
                .content_source(path)
                .is_some_and(|source| source.site == site)
        }) {
            return Err(AssetError::Uncaptured(path.clone()));
        }
        let base = self.content.base_url(site);
        let mut entries = BTreeMap::new();
        let mut uncompiled = Vec::new();
        let mut refusals = BTreeMap::new();
        let downloads = self.notebook_download_owners(site);
        let mut remaining = self.projection_limits.text_bytes;
        let mut records = 0usize;
        for (path, file) in self.corpus.files() {
            if workspace
                .content_source(path)
                .is_some_and(|source| source.is_retained_only(path))
            {
                continue;
            }
            if path.as_str().starts_with("__legal/") {
                // Legal entries are documents, never implicit companion transports.
                continue;
            }
            if self
                .selected_assets
                .as_ref()
                .is_some_and(|selected| !selected.contains(path))
            {
                continue;
            }
            let bytes = match file {
                File::Asset(bytes) => *bytes,
                File::Notebook(notebook) => notebook.source(),
                _ => continue,
            };
            let source = workspace.content_source(path);
            if source.is_some_and(|source| source.site != site) {
                continue;
            }
            records = records
                .checked_add(1)
                .filter(|&n| n <= self.projection_limits.nodes)
                .ok_or(AssetError::Limit("records"))?;
            charge(&mut remaining, path.as_str().len())?;
            let Some(source) = source else {
                uncompiled.push(path.clone());
                continue;
            };
            let route = match (file, downloads.get(path)) {
                // A selected download is emitted only after its pre-public scan passes.
                (File::Notebook(notebook), Some(owner)) => match notebook.scan_for_download() {
                    Some(finding) => {
                        refusals.insert(path.clone(), finding);
                        None
                    }
                    None => Some(self.notebook_download_route(owner, path)?),
                },
                _ => self.asset_route(source, path)?,
            };
            let Some(route) = route else {
                uncompiled.push(path.clone());
                continue;
            };
            let base = base.ok_or_else(|| AssetError::Base(site.clone()))?;
            // Bound derived amplification before retaining URL/output/origin copies.
            for length in [
                base.as_str().len(),
                route.as_str().len(),
                route.output().as_str().len(),
                path.as_str().len(),
            ] {
                charge(&mut remaining, length)?;
            }
            let sha256 = Sha256Digest::of(bytes);
            entries.insert(
                path.clone(),
                StaticAsset {
                    url: base.resolve(&route),
                    output: route.output().clone(),
                    sha256,
                    bytes: bytes.len() as u64,
                    provenance: source,
                    source: Origin {
                        path: path.clone(),
                        source_sha256: sha256,
                        span: None,
                    },
                    content: bytes,
                },
            );
        }
        Ok(AssetPlan {
            entries,
            uncompiled,
            refusals,
        })
    }

    /// Resolve package-owned static companions before the binding-wide transport.
    /// Both paths consume the same retained bytes and public route grammar.
    pub(super) fn asset_route(
        &self,
        source: ContentSource<'_>,
        path: &EntryPath,
    ) -> Result<Option<RoutePath>, AssetError> {
        if supported_static(path.as_str()) {
            for (owner, package) in &self.page_packages {
                if let Some(relative) = package.relative(path) {
                    let node = self
                        .content
                        .graph()
                        .node(owner)
                        .ok_or_else(|| AssetError::Route(path.clone()))?;
                    if &node.site != source.site {
                        return Err(AssetError::Route(path.clone()));
                    }
                    let route = node
                        .route
                        .as_ref()
                        .ok_or_else(|| AssetError::Route(path.clone()))?;
                    return route
                        .path
                        .join_file(relative)
                        .map(Some)
                        .map_err(|_| AssetError::Route(path.clone()));
                }
            }
        }
        static_route(source, path)
    }
}

/// Share literal static addressing without compiling or hashing a foreign site.
pub(super) fn static_route(
    source: ContentSource<'_>,
    path: &EntryPath,
) -> Result<Option<RoutePath>, AssetError> {
    let Some(destination) = source
        .tree_binding()
        .and_then(|binding| binding.static_assets.as_ref())
        .filter(|_| supported_static(path.as_str()))
    else {
        return Ok(None);
    };
    let relative: EntryPath = source
        .entry(path)
        .ok_or_else(|| AssetError::Route(path.clone()))?
        .parse()
        .map_err(|_| AssetError::Route(path.clone()))?;
    destination
        .route_prefix
        .join_file(&relative)
        .map(Some)
        .map_err(|_| AssetError::Route(path.clone()))
}

pub(super) fn supported_static(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, extension)| {
        [
            "png", "jpg", "jpeg", "gif", "webp", "avif", "pdf", "bib", "txt", "csv", "zip", "mp4",
            "webm", "vtt",
        ]
        .iter()
        .any(|allowed| extension.eq_ignore_ascii_case(allowed))
    })
}

fn charge(remaining: &mut usize, bytes: usize) -> Result<(), AssetError> {
    *remaining = remaining
        .checked_sub(bytes)
        .ok_or(AssetError::Limit("text bytes"))?;
    Ok(())
}
