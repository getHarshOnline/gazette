//! Shared source capture for exact strict or development publication sets.

use super::{
    AcquiredInputs, AcquiredPublicationSet, PublicationSetAcquisitionLimits, PublicationSetError,
    composition, publication_inputs,
};
use gazette_compiler::{toml, workspace};
use gazette_core::publication::Key;
use gazette_ports::source::{RevisionReader, SourceCapture};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Compile once and capture the union of selected presentation and all configured content.
///
/// Every source key is read once under one aggregate budget. Site views retain the
/// same captured bytes and compilation; other sites' presentation sources are not read.
/// Complete content sources acquire first. Their failure stops the shared prerequisite;
/// presentation-only failures remain exact source/site outcomes while other sites continue.
/// A key serving both roles is required content, not a degradable presentation input.
/// No source is retried and no stale fallback is allowed.
///
/// # Errors
/// Rejects empty/duplicate/over-limit selection before compilation, then absent sites,
/// missing bindings and shared content failures before returning a set.
/// Presentation-only native/bound failures are retained in the returned set.
pub fn acquire_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &str,
    sites: &[Key],
    syntax: toml::Limits,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let requested = sites;
    let sites = validate_site_selection(sites, limits)?;
    let configuration = crate::configuration::resolve(source, syntax)
        .map_err(PublicationSetError::Configuration)?;
    acquire_validated_set(
        reader,
        revisions,
        configuration,
        sites,
        requested,
        limits,
        crate::source::AcquisitionMode::Strict,
    )
}

/// Capture a strict publication set from one already-resolved workspace.
///
/// # Errors
/// Refuses invalid site selection, missing bindings and incomplete strict source capture.
pub fn acquire_resolved_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    sites: &[Key],
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let requested = sites;
    let sites = validate_site_selection(sites, limits)?;
    acquire_validated_set(
        reader,
        revisions,
        configuration,
        sites,
        requested,
        limits,
        crate::source::AcquisitionMode::Strict,
    )
}

/// Acquire the exact workspace default set when no invocation override was selected.
///
/// # Errors
/// Refuses absent defaults and the same bounds, source and presentation failures as
/// explicit acquisition. Configured defaults never grant native-read permission.
pub fn acquire_configured_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let selection = configuration
        .compilation()
        .workspace
        .build_selection(None)
        .map_err(PublicationSetError::Selection)?;
    acquire_selected_publications(reader, revisions, configuration, &selection, limits)
}

/// Capture one already-resolved build intent without relabelling its owner or order.
///
/// # Errors
/// Refuses forged or stale selection evidence, invalid bounds and incomplete capture.
/// The selection must exactly match this retained workspace and its declared draft policy.
pub fn acquire_selected_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    selection: &workspace::BuildSelection,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    acquire_selected_set(
        reader,
        revisions,
        configuration,
        selection,
        limits,
        crate::source::AcquisitionMode::Strict,
    )
}

/// Capture development bytes while preserving the same resolved build intent.
///
/// # Errors
/// Refuses stale selection evidence and unsafe capture; no configured pin is changed.
pub fn acquire_development_selected_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    selection: &workspace::BuildSelection,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    acquire_selected_set(
        reader,
        revisions,
        configuration,
        selection,
        limits,
        crate::source::AcquisitionMode::Development,
    )
}

fn acquire_selected_set(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    selection: &workspace::BuildSelection,
    limits: PublicationSetAcquisitionLimits,
    mode: crate::source::AcquisitionMode,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let invocation = match selection.origin {
        workspace::BuildOrigin::Workspace => None,
        workspace::BuildOrigin::Invocation => Some(selection.sites.as_slice()),
    };
    let expected = configuration
        .compilation()
        .workspace
        .build_selection(invocation)
        .map_err(PublicationSetError::Selection)?;
    if &expected != selection {
        return Err(PublicationSetError::Selection(
            "build selection does not match its retained workspace",
        ));
    }
    let sites = validate_site_selection(&selection.sites, limits)?;
    acquire_set_with_selection(
        reader,
        revisions,
        configuration,
        sites,
        selection.clone(),
        limits,
        mode,
    )
}

/// Acquire explicitly selected development working bytes with the configured Git baseline.
///
/// Shares strict acquisition's configuration, source selection, confinement and budgets.
/// Every retained input is marked development, even when its bytes match the pin.
/// No baseline pin is advanced and no verified materialization is established.
///
/// # Errors
/// Rejects the same malformed selections and unsafe, over-limit or wrongly checked-out
/// sources as strict acquisition; only working-byte equality with the baseline is relaxed.
pub fn acquire_development_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    source: &str,
    sites: &[Key],
    syntax: toml::Limits,
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let requested = sites;
    let sites = validate_site_selection(sites, limits)?;
    let configuration = crate::configuration::resolve(source, syntax)
        .map_err(PublicationSetError::Configuration)?;
    acquire_validated_set(
        reader,
        revisions,
        configuration,
        sites,
        requested,
        limits,
        crate::source::AcquisitionMode::Development,
    )
}

/// Capture development bytes from one already-resolved workspace.
///
/// # Errors
/// Refuses invalid site selection, missing bindings and unsafe or incomplete source capture.
pub fn acquire_development_resolved_publications(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    sites: &[Key],
    limits: PublicationSetAcquisitionLimits,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let requested = sites;
    let sites = validate_site_selection(sites, limits)?;
    acquire_validated_set(
        reader,
        revisions,
        configuration,
        sites,
        requested,
        limits,
        crate::source::AcquisitionMode::Development,
    )
}

fn acquire_validated_set(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    sites: BTreeSet<&Key>,
    requested: &[Key],
    limits: PublicationSetAcquisitionLimits,
    mode: crate::source::AcquisitionMode,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let selection = configuration
        .compilation()
        .workspace
        .build_selection(Some(requested))
        .map_err(PublicationSetError::Selection)?;
    acquire_set_with_selection(
        reader,
        revisions,
        configuration,
        sites,
        selection,
        limits,
        mode,
    )
}

fn acquire_set_with_selection(
    reader: &impl SourceCapture,
    revisions: &impl RevisionReader,
    configuration: crate::configuration::ResolvedWorkspace,
    sites: BTreeSet<&Key>,
    build_selection: workspace::BuildSelection,
    limits: PublicationSetAcquisitionLimits,
    mode: crate::source::AcquisitionMode,
) -> Result<AcquiredPublicationSet, PublicationSetError> {
    let compilation = configuration.compilation();
    let mut presentations = BTreeMap::new();
    let mut selected = BTreeSet::new();
    for site in sites {
        let packages = publication_inputs::package_selection(&compilation.workspace, site)
            .map_err(PublicationSetError::Acquisition)?;
        selected.extend(packages.sources.iter().cloned());
        presentations.insert(site.clone(), packages);
    }
    let required = publication_inputs::required_sources(&compilation.workspace);
    selected.extend(required.iter().cloned());
    let captured = composition::capture_partitioned(
        reader,
        revisions,
        configuration,
        &selected,
        &required,
        &mut presentations,
        composition::CapturePlan {
            limits: limits.sources,
            mode,
            strict_presentations: false,
        },
    )
    .map_err(PublicationSetError::Acquisition)?;
    let shared = captured.inputs;
    // Required content was captured independently of presentation success. This
    // view borrows that same retained set; it never grants a failed site a Theme.
    let content = AcquiredInputs::from_capture(
        Arc::clone(&shared),
        presentations
            .first_key_value()
            .ok_or(PublicationSetError::Selection("empty acquired site set"))?
            .0
            .clone(),
        BTreeSet::new(),
        BTreeSet::new(),
    );
    Ok(AcquiredPublicationSet {
        content_sources: required,
        presentation_sources: presentations
            .iter()
            .map(|(site, packages)| (site.clone(), packages.sources.clone()))
            .collect(),
        build_selection,
        content,
        sites: presentations
            .into_iter()
            .map(|(site, packages)| {
                let inputs = match packages
                    .sources
                    .iter()
                    .find_map(|key| captured.failures.get(key))
                {
                    Some(error) => Err(Arc::clone(error)),
                    None => Ok(AcquiredInputs::from_capture(
                        Arc::clone(&shared),
                        site.clone(),
                        packages.themes,
                        packages.plugins,
                    )),
                };
                (site, inputs)
            })
            .collect(),
        shared,
        source_failures: captured.failures,
    })
}

fn validate_site_selection(
    sites: &[Key],
    limits: PublicationSetAcquisitionLimits,
) -> Result<BTreeSet<&Key>, PublicationSetError> {
    if sites.is_empty() || sites.len() > limits.sites {
        return Err(PublicationSetError::Selection(
            "site count is empty or exceeds its limit",
        ));
    }
    let distinct: BTreeSet<_> = sites.iter().collect();
    if distinct.len() != sites.len() {
        return Err(PublicationSetError::Selection("duplicate requested site"));
    }
    Ok(distinct)
}
