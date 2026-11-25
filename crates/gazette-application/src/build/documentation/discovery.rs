//! Validated provider pages bound to the configured documentation owner and captures.

use super::{AcquiredInputs, DocumentationAssembly, DocumentationError, selection};
use gazette_compiler::workspace::Key;
use gazette_core::{
    documentation::{CatalogEntry, ValidatedCatalog},
    publication::routing::{PublicBaseUrl, RoutePath},
    source::{Content, EntryPath, Inventory, Sha256Digest, TreeIdentity},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::Arc,
};

/// One provider page with its existing configured project identity.
#[derive(Debug)]
pub struct DocumentationCatalogEntry {
    /// Project explicitly selected by this documentation module.
    pub project: Key,
    /// Provider-admitted route, text, anchors and exact artifact evidence.
    pub page: CatalogEntry,
}

/// Immutable validated documentation evidence; neither a content graph nor publication approval.
#[derive(Debug)]
pub struct DocumentationCatalog {
    site: Key,
    brand: Key,
    configuration: Sha256Digest,
    sources: BTreeMap<Key, TreeIdentity>,
    source_entries: BTreeMap<Key, BTreeMap<EntryPath, Content>>,
    roots: Vec<PathBuf>,
    evidence_sha256: Sha256Digest,
    retained_bytes: usize,
    output_bytes: u64,
    output_entries: usize,
    output_depth: usize,
    output_file_bytes: u64,
    canonical_base: Option<PublicBaseUrl>,
    output: TreeIdentity,
    incomplete_roots: BTreeSet<EntryPath>,
    entries: Vec<DocumentationCatalogEntry>,
}
pub(super) fn invalid(reason: &'static str) -> DocumentationError {
    DocumentationError::Target(gazette_core::documentation::Error::Invalid {
        context: "documentation catalog".into(),
        reason,
    })
}
fn configuration(inputs: &AcquiredInputs) -> Result<Sha256Digest, DocumentationError> {
    evidence::digest(
        inputs.configuration_evidence(),
        gazette_core::documentation::CatalogLimits::default().bytes,
    )
}
impl DocumentationCatalog {
    pub(super) fn bind(
        inputs: &AcquiredInputs,
        catalog: ValidatedCatalog,
        output: &Inventory,
        protected: &[PathBuf],
    ) -> Result<Self, DocumentationError> {
        if catalog.output_identity() != output.identity() {
            return Err(invalid("catalog belongs to another target output"));
        }
        let (brand, repository, build) = selection(&inputs.compilation().workspace, inputs.site())?;
        let modules: BTreeMap<_, _> = build
            .modules
            .iter()
            .map(|module| (module.slug.as_str(), &module.project))
            .collect();
        let required: BTreeSet<_> = build
            .modules
            .iter()
            .flat_map(|module| {
                module.manual.iter().map(|manual| &manual.source).chain(
                    module
                        .navigation
                        .iter()
                        .filter_map(gazette_compiler::workspace::DocumentationNavigation::reference)
                        .map(|(_, _, input)| input.source()),
                )
            })
            .collect();
        let retained: BTreeMap<_, _> = required
            .into_iter()
            .map(|key| {
                inputs
                    .sources()
                    .get(key)
                    .map(|input| (key, input))
                    .ok_or_else(|| invalid("configured documentation source evidence is missing"))
            })
            .collect::<Result<_, _>>()?;
        let sources: BTreeMap<Key, TreeIdentity> = retained
            .iter()
            .map(|(key, input)| ((*key).clone(), input.inventory().identity()))
            .collect();
        let configuration = configuration(inputs)?;
        let mut remaining = gazette_core::documentation::CatalogLimits::default().bytes;
        for (key, input) in &retained {
            remaining = remaining
                .checked_sub(key.as_str().len())
                .ok_or_else(|| invalid("catalog source evidence bound exceeded"))?;
            for entry in input.inventory().entries() {
                remaining = remaining
                    .checked_sub(
                        entry
                            .path
                            .as_str()
                            .len()
                            .saturating_add(std::mem::size_of::<(EntryPath, Content)>()),
                    )
                    .ok_or_else(|| invalid("catalog source evidence bound exceeded"))?;
            }
        }
        let (evidence_sha256, encoded_bytes) = evidence::identity(
            &(
                "gazette-documentation-catalog/1",
                configuration,
                inputs.site(),
                &brand.id,
                &repository.public_base_url,
                &catalog,
                &sources,
            ),
            remaining,
        )?;
        let retained_bytes =
            gazette_core::documentation::CatalogLimits::default().bytes - remaining + encoded_bytes;
        let source_entries = retained
            .iter()
            .map(|(key, input)| {
                (
                    (*key).clone(),
                    input
                        .inventory()
                        .entries()
                        .iter()
                        .map(|entry| (entry.path.clone(), entry.content))
                        .collect(),
                )
            })
            .collect();
        let mut entries = Vec::with_capacity(catalog.entries().len());
        for page in catalog.entries() {
            let project = modules
                .get(page.module.as_str())
                .ok_or_else(|| invalid("catalog module has no configured project"))?;
            entries.push(DocumentationCatalogEntry {
                project: (*project).clone(),
                page: page.clone(),
            });
        }
        let mut roots = inputs
            .source_roots()
            .map_err(DocumentationError::Composition)?;
        if let crate::configuration::Evidence::Documents { native_root, .. } =
            inputs.configuration_evidence()
        {
            roots.push(native_root.clone());
        }
        roots.extend(protected.iter().cloned());
        roots.sort();
        roots.dedup();
        Ok(Self {
            site: inputs.site().clone(),
            brand: brand.id.clone(),
            configuration,
            source_entries,
            roots,
            evidence_sha256,
            retained_bytes,
            output_bytes: output.total_bytes(),
            output_entries: output.entries().len(),
            output_depth: output
                .entries()
                .iter()
                .map(|entry| entry.path.as_str().split('/').count())
                .max()
                .unwrap_or(0),
            output_file_bytes: output
                .entries()
                .iter()
                .filter_map(|entry| match entry.content {
                    Content::File { bytes, .. } => Some(bytes),
                    _ => None,
                })
                .max()
                .unwrap_or(0),
            sources,
            canonical_base: repository.public_base_url.clone(),
            output: output.identity(),
            incomplete_roots: catalog.incomplete_roots().clone(),
            entries,
        })
    }
    pub(super) fn charge(
        &self,
        output: &mut gazette_core::source::Limits,
        catalog_bytes: &mut usize,
    ) -> Result<(), DocumentationError> {
        if self.output_depth > output.depth || self.output_file_bytes > output.file_bytes {
            return Err(invalid("documentation target per-entry bound exceeded"));
        }
        output.total_bytes = output
            .total_bytes
            .checked_sub(self.output_bytes)
            .ok_or_else(|| invalid("documentation target byte bound exceeded"))?;
        output.entries = output
            .entries
            .checked_sub(self.output_entries)
            .ok_or_else(|| invalid("documentation target entry bound exceeded"))?;
        *catalog_bytes = catalog_bytes
            .checked_sub(self.retained_bytes)
            .ok_or_else(|| invalid("documentation catalog aggregate byte bound exceeded"))?;
        Ok(())
    }
    /// Bounded identity of admitted semantics, ownership, canonical address and captured sources.
    pub fn evidence_sha256(&self) -> Sha256Digest {
        self.evidence_sha256
    }
    /// Original independent documentation capture identities, including documentation-only sources.
    pub fn source_identities(&self) -> &BTreeMap<Key, TreeIdentity> {
        &self.sources
    }
    /// Independent source, configuration and generated-stage roots protected by this retained evidence.
    pub fn protected_roots(&self) -> &[PathBuf] {
        &self.roots
    }
    /// Site whose configured documentation repository owns these pages.
    pub fn site(&self) -> &Key {
        &self.site
    }
    /// Brand owning that independent documentation repository.
    pub fn brand(&self) -> &Key {
        &self.brand
    }
    /// Configured canonical documentation base; absent remains unavailable.
    pub fn canonical_base(&self) -> Option<&PublicBaseUrl> {
        self.canonical_base.as_ref()
    }
    /// Complete validated reading entries, one per provider page.
    pub fn entries(&self) -> &[DocumentationCatalogEntry] {
        &self.entries
    }
    /// Exact provider tree that proves these entries.
    pub fn output_identity(&self) -> TreeIdentity {
        self.output
    }
    /// Whether every selected generator root supplied usable retained evidence.
    pub fn complete(&self) -> bool {
        self.incomplete_roots.is_empty()
    }
    /// Selected generator roots without usable retained provider evidence.
    pub fn incomplete_roots(&self) -> &BTreeSet<EntryPath> {
        &self.incomplete_roots
    }
    /// Resolve a provider route beneath an explicit serving base, or its canonical base.
    /// The optional base belongs to the documentation server, not outbound link rewriting.
    ///
    /// # Errors
    /// Rejects an entry outside this catalog or an invalid provider route.
    pub fn page_url(
        &self,
        entry: &DocumentationCatalogEntry,
        serving_base: Option<&PublicBaseUrl>,
    ) -> Result<Option<String>, DocumentationError> {
        if !self
            .entries
            .binary_search_by(|candidate| candidate.page.route.cmp(&entry.page.route))
            .ok()
            .is_some_and(|index| std::ptr::eq(&self.entries[index], entry))
        {
            return Err(invalid("entry does not belong to this catalog"));
        }
        let route = "/"
            .parse::<RoutePath>()
            .map_err(|_| invalid("invalid documentation route root"))?
            .join_file(&entry.page.route)
            .map_err(|_| invalid("provider route cannot form a reading address"))?;
        Ok(serving_base
            .or(self.canonical_base.as_ref())
            .map(|base| base.resolve(&route)))
    }
    /// Canonical address remains independent of local preview serving context.
    ///
    /// # Errors
    /// Rejects an entry outside this catalog or an invalid provider route.
    pub fn canonical_url(
        &self,
        entry: &DocumentationCatalogEntry,
    ) -> Result<Option<String>, DocumentationError> {
        self.page_url(entry, None)
    }
}
impl DocumentationAssembly<'_> {
    /// Optional validated discovery evidence; unsupported adapters leave it absent.
    pub fn catalog(&self) -> Option<&DocumentationCatalog> {
        self.catalog.as_deref()
    }
    /// Share the immutable validated catalog with a separately acquired publication.
    pub fn catalog_capture(&self) -> Option<Arc<DocumentationCatalog>> {
        self.catalog.clone()
    }
    /// Provider reading routes with known fragment IDs, keyed by extensionless path.
    /// Absent adapters and unqualified anchors stay distinct from empty inventories.
    pub fn reading_routes(&self) -> Option<BTreeMap<&EntryPath, Option<&BTreeSet<String>>>> {
        self.catalog().map(|catalog| {
            catalog
                .entries()
                .iter()
                .map(|entry| (&entry.page.route, entry.page.anchors.as_ref()))
                .collect()
        })
    }
}
impl AcquiredInputs {
    /// Retained documentation pages explicitly installed for this publication.
    pub fn documentation_catalog(&self) -> Option<&DocumentationCatalog> {
        self.documentation_catalog.as_deref()
    }
    /// Share the exact installed immutable evidence for bounded native preview reuse.
    pub fn documentation_catalog_capture(&self) -> Option<Arc<DocumentationCatalog>> {
        self.documentation_catalog.clone()
    }
    /// Attach a validated catalog without rereading sources or invoking a provider.
    ///
    /// # Errors
    /// Rejects replacement, another owner/configuration or a changed overlapping source capture.
    pub fn install_documentation_catalog(
        &mut self,
        catalog: Arc<DocumentationCatalog>,
    ) -> Result<(), DocumentationError> {
        self.check_documentation_catalog(&catalog)?;
        self.documentation_catalog = Some(catalog);
        Ok(())
    }
    pub(crate) fn check_documentation_catalog(
        &self,
        catalog: &DocumentationCatalog,
    ) -> Result<(), DocumentationError> {
        if self.documentation_catalog.is_some()
            || catalog.site != *self.site()
            || catalog.configuration != configuration(self)?
        {
            return Err(invalid(
                "catalog owner or configuration does not match this acquisition",
            ));
        }
        for (key, input) in self.sources() {
            if let Some(expected) = catalog.source_entries.get(key) {
                let observed = input.inventory().entries();
                for (path, content) in expected {
                    let index = observed
                        .binary_search_by(|entry| entry.path.cmp(path))
                        .map_err(|_| {
                            invalid("overlapping publication capture lacks a documentation entry")
                        })?;
                    if observed[index].content != *content {
                        return Err(invalid("overlapping documentation source entry changed"));
                    }
                }
            }
        }
        Ok(())
    }
}

mod evidence;

#[cfg(test)]
#[path = "../../../tests/internal/documentation_catalog.rs"]
mod tests;
