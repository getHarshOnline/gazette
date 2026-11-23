//! Native adapter selection for the existing documentation capture and assembly owners.

use super::{AcquiredPublicationSet, DocumentationCatalog, PublicationSetError};
use crate::{source, workspace::Key};
use std::{collections::BTreeMap, sync::Arc};

pub(super) fn acquire(
    inputs: &mut AcquiredPublicationSet,
    supplied: &BTreeMap<Key, Arc<DocumentationCatalog>>,
    acquisition: source::PreparationLimits,
    output: source::Limits,
    preview_links: Option<&gazette_core::publication::routing::PreviewLinkBases>,
) -> Result<(), PublicationSetError> {
    let documents = gazette_application::build::acquire_publication_documentation(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &gazette_adapter_markdown::Comrak,
        inputs,
        acquisition,
    )?;
    let target = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .with_preview_link_bases(preview_links);
    documents.assemble_into(&target, inputs, supplied, output)
}

/// Acquire and assemble configured imported documentation for an existing native publication set.
/// No generator or stage runs; configured generation requires an explicit supplied native result.
///
/// # Errors
/// Preserves source, provider, incomplete-catalog and complete-set bound failures.
pub fn acquire_documentation_catalogs(
    inputs: &mut AcquiredPublicationSet,
    acquisition: source::PreparationLimits,
    output: source::Limits,
) -> Result<(), PublicationSetError> {
    acquire(inputs, &BTreeMap::new(), acquisition, output, None)
}
