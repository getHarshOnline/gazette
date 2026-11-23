//! Existing documentation captures and assembly joined to a publication selection.

use super::{
    AcquiredInputs, DocumentationCatalog, DocumentationError, acquire_resolved_documentation,
};
use crate::build::{AcquiredPublicationSet, PublicationSetError};
use crate::source::PreparationLimits;
use gazette_compiler::workspace::Key;
use gazette_core::source::{self, TreeIdentity};
use gazette_ports::{
    documentation::DocumentationTarget,
    markdown::Markdown,
    source::{RevisionReader, SourceCapture},
};
use std::{collections::BTreeMap, sync::Arc};

/// Existing native documentation acquisitions kept until their one target assembly.
/// These are independently scoped captures, not publication content nodes.
#[derive(Debug)]
pub struct DocumentationInputs {
    inputs: BTreeMap<Key, AcquiredInputs>,
}
fn failure(site: &Key, error: DocumentationError) -> PublicationSetError {
    PublicationSetError::Documentation {
        site: site.clone(),
        error: Box::new(error),
    }
}

/// Capture configured documentation for every successfully acquired selected publication.
/// Source bounds cover the existing publication captures and these additional observations.
/// No generator, target stage or provider process runs here.
///
/// # Errors
/// Refuses incomplete native captures or exhausted complete-set bounds without a partial result.
pub fn acquire_publication_documentation(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    markdown: &dyn Markdown,
    publications: &AcquiredPublicationSet,
    mut limits: PreparationLimits,
) -> Result<DocumentationInputs, PublicationSetError> {
    let charge = |limits: &mut PreparationLimits, input: &crate::source::AcquiredInput| {
        limits.sources = limits.sources.checked_sub(1).ok_or_else(|| {
            super::discovery::invalid("documentation source count bound exceeded")
        })?;
        limits.entries = limits
            .entries
            .checked_sub(input.inventory().entries().len())
            .ok_or_else(|| {
                super::discovery::invalid("documentation source entry bound exceeded")
            })?;
        limits.bytes = limits
            .bytes
            .checked_sub(input.inventory().total_bytes())
            .ok_or_else(|| super::discovery::invalid("documentation source byte bound exceeded"))?;
        Ok::<_, DocumentationError>(())
    };
    let mut inputs = BTreeMap::new();
    for (site, publication) in publications.sites() {
        if publication.is_err() {
            continue;
        }
        let configured = publications.compilation().workspace.brands().any(|brand| {
            brand.publications.iter().any(|publication| {
                &publication.site == site
                    && publication
                        .documentation_repository
                        .as_ref()
                        .is_some_and(|repository| repository.build.is_some())
            })
        });
        if !configured {
            continue;
        }
        if inputs.is_empty() {
            for input in publications.sources().values() {
                charge(&mut limits, input).map_err(|error| failure(site, error))?;
            }
        }
        let acquired = acquire_resolved_documentation(
            reader,
            revisions,
            markdown,
            publications.configuration().clone(),
            site,
            limits,
        )
        .map_err(|error| failure(site, error))?;
        for input in acquired.sources().values() {
            charge(&mut limits, input).map_err(|error| failure(site, error))?;
        }
        inputs.insert(site.clone(), acquired);
    }
    Ok(DocumentationInputs { inputs })
}
impl DocumentationInputs {
    /// Exact independently acquired owners, borrowed for the existing native documentation operation.
    pub fn sites(&self) -> &BTreeMap<Key, AcquiredInputs> {
        &self.inputs
    }
    /// Complete independent documentation/configuration roots retained before any stage effect.
    ///
    /// # Errors
    /// Refuses a missing native source binding rather than dropping its protection.
    pub fn source_roots(&self) -> Result<Vec<std::path::PathBuf>, PublicationSetError> {
        let mut roots = Vec::new();
        for inputs in self.inputs.values() {
            roots.extend(
                inputs
                    .source_roots()
                    .map_err(PublicationSetError::Acquisition)?,
            );
            if let crate::configuration::Evidence::Documents { native_root, .. } =
                inputs.configuration_evidence()
            {
                roots.push(native_root.clone());
            }
        }
        roots.sort();
        roots.dedup();
        Ok(roots)
    }
    /// Whether no selected publication configures a documentation build.
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
    /// Raw capture identities for watch comparisons before any target assembly or generation.
    pub fn source_identities(&self) -> BTreeMap<Key, BTreeMap<Key, TreeIdentity>> {
        self.inputs
            .iter()
            .map(|(site, inputs)| {
                (
                    site.clone(),
                    inputs
                        .sources()
                        .iter()
                        .map(|(key, input)| (key.clone(), input.inventory().identity()))
                        .collect(),
                )
            })
            .collect()
    }
    /// Validate supplied owners and exact current captures without installing or assembling anything.
    /// Returns target capacity remaining after the supplied catalogs.
    ///
    /// # Errors
    /// Refuses stale, incomplete, conflicting or oversized supplied evidence before native effects.
    pub fn check_supplied(
        &self,
        publications: &AcquiredPublicationSet,
        supplied: &BTreeMap<Key, Arc<DocumentationCatalog>>,
        mut output: source::Limits,
    ) -> Result<source::Limits, PublicationSetError> {
        let mut catalog_bytes = gazette_core::documentation::CatalogLimits::default().bytes;
        for (site, catalog) in supplied {
            let inputs = self.inputs.get(site).ok_or_else(|| {
                failure(
                    site,
                    super::discovery::invalid(
                        "supplied documentation catalog has no selected configured owner",
                    ),
                )
            })?;
            let identities: BTreeMap<_, _> = inputs
                .sources()
                .iter()
                .map(|(key, input)| (key.clone(), input.inventory().identity()))
                .collect();
            if catalog.source_identities() != &identities {
                return Err(failure(
                    site,
                    super::discovery::invalid(
                        "supplied documentation catalog source capture changed",
                    ),
                ));
            }
            if !catalog.complete() {
                return Err(failure(
                    site,
                    super::discovery::invalid(
                        "configured documentation catalog has incomplete generated roots",
                    ),
                ));
            }
            publications
                .sites()
                .get(site)
                .and_then(|result| result.as_ref().ok())
                .ok_or_else(|| {
                    failure(
                        site,
                        super::discovery::invalid(
                            "documentation owner is not an acquired publication",
                        ),
                    )
                })?
                .check_documentation_catalog(catalog)
                .map_err(|error| failure(site, error))?;
            catalog
                .charge(&mut output, &mut catalog_bytes)
                .map_err(|error| failure(site, error))?;
        }
        Ok(output)
    }
    /// Assemble imported inputs once, or reuse explicitly supplied validated native results.
    /// Supplied results must still match the freshly captured documentation inputs exactly.
    /// Configured generation without a supplied result preserves `GenerationRequired`.
    ///
    /// # Errors
    /// Rejects absent/incomplete catalog support, stale supplied results, missing generated output,
    /// another selected owner, or exhausted aggregate target bounds.
    pub fn assemble_into(
        &self,
        target: &(impl DocumentationTarget + ?Sized),
        publications: &mut AcquiredPublicationSet,
        supplied: &BTreeMap<Key, Arc<DocumentationCatalog>>,
        mut output: source::Limits,
    ) -> Result<(), PublicationSetError> {
        for site in supplied.keys() {
            if !self.inputs.contains_key(site) {
                return Err(failure(
                    site,
                    super::discovery::invalid(
                        "supplied documentation catalog has no selected configured owner",
                    ),
                ));
            }
        }
        let mut selected = Vec::new();
        let mut catalog_bytes = gazette_core::documentation::CatalogLimits::default().bytes;
        for (site, inputs) in &self.inputs {
            let catalog = if let Some(catalog) = supplied.get(site) {
                let identities: BTreeMap<_, _> = inputs
                    .sources()
                    .iter()
                    .map(|(key, input)| (key.clone(), input.inventory().identity()))
                    .collect();
                if catalog.source_identities() != &identities {
                    return Err(failure(
                        site,
                        super::discovery::invalid(
                            "supplied documentation catalog source capture changed",
                        ),
                    ));
                }
                Arc::clone(catalog)
            } else {
                let assembled = inputs
                    .assemble_documentation(target, output)
                    .map_err(|error| failure(site, error))?;
                assembled.catalog_capture().ok_or_else(|| {
                    failure(
                        site,
                        super::discovery::invalid(
                            "configured documentation target supplied no validated catalog",
                        ),
                    )
                })?
            };
            if !catalog.complete() {
                return Err(failure(
                    site,
                    super::discovery::invalid(
                        "configured documentation catalog has incomplete generated roots",
                    ),
                ));
            }
            catalog
                .charge(&mut output, &mut catalog_bytes)
                .map_err(|error| failure(site, error))?;
            publications
                .sites()
                .get(site)
                .and_then(|input| input.as_ref().ok())
                .ok_or_else(|| {
                    failure(
                        site,
                        super::discovery::invalid(
                            "documentation owner is not an acquired publication",
                        ),
                    )
                })?
                .check_documentation_catalog(&catalog)
                .map_err(|error| failure(site, error))?;
            selected.push((site.clone(), catalog));
        }
        for (site, catalog) in selected {
            publications
                .install_documentation_catalog(catalog)
                .map_err(|error| failure(&site, error))?;
        }
        Ok(())
    }
}
