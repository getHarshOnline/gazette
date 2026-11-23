//! Bounded native documentation reuse within the existing development-preview lifecycle.

use super::*;
use build::{
    DocumentationCatalog, DocumentationDelivery, DocumentationPublicationExecution,
    DocumentationReceipt,
};
use std::{cell::RefCell, sync::Arc};

type Identities = BTreeMap<Key, BTreeMap<Key, source::TreeIdentity>>;
#[derive(Default)]
struct Cache {
    catalogs: BTreeMap<Key, Arc<DocumentationCatalog>>,
    receipts: BTreeMap<Key, DocumentationReceipt>,
    failed: Option<(Identities, Vec<DocumentationReceipt>)>,
    pending: Vec<DocumentationReceipt>,
    current: Vec<DocumentationReceipt>,
}

pub(super) fn watch(
    request: watch::Request<'_>,
    options: watch::Options,
    supplied: &BTreeMap<Key, Arc<DocumentationCatalog>>,
    generation: Option<&build::DocumentationGenerationRequest>,
    observe: &mut impl FnMut(Event) -> io::Result<()>,
    control: &mut impl FnMut() -> io::Result<watch::Control>,
    deliver: &mut impl FnMut(DocumentationDelivery<'_>) -> io::Result<()>,
) -> Result<(), Error> {
    let local = options.local_urls;
    if generation.is_none() {
        return watch::run_with_documentation(request, options, supplied, |event| {
            let closed = matches!(event, watch::Event::Closed);
            if let Some(event) = watch_event(event, local)? {
                observe(event)?;
            }
            if closed {
                Ok(watch::Control::Continue)
            } else {
                control()
            }
        })
        .map_err(Error::Watch);
    }
    let output = source::Limits {
        entries: options.render.entries,
        total_bytes: options.render.bytes,
        ..Default::default()
    };
    let preview_links = request.render.preview_link_bases.clone();
    let cache = RefCell::new(Cache::default());
    let fatal = RefCell::new(None::<io::Error>);
    let deliver = RefCell::new(deliver);
    let assemble = |documents: &gazette_application::build::DocumentationInputs,
                    inputs: &mut build::AcquiredPublicationSet| {
        let identities = documents.source_identities();
        if let Some((previous, receipts)) = &cache.borrow().failed
            && previous == &identities
        {
            if let Err(error) =
                deliver.borrow_mut()(DocumentationDelivery::ReplayedFailure { receipts })
            {
                *fatal.borrow_mut() = Some(error);
            }
            return Err(build::PublicationSetError::Selection(
                "unchanged documentation generation failure; original receipt retained",
            ));
        }
        let mut catalogs = supplied.clone();
        let mut reused = BTreeMap::new();
        {
            let cache = cache.borrow();
            for (site, catalog) in &cache.catalogs {
                if !catalogs.contains_key(site)
                    && identities.get(site) == Some(catalog.source_identities())
                {
                    catalogs.insert(site.clone(), Arc::clone(catalog));
                    if let Some(receipt) = cache.receipts.get(site) {
                        reused.insert(site.clone(), receipt.clone());
                    }
                }
            }
        }
        let mut remaining_generation = generation.cloned();
        if let Some(selection) = &mut remaining_generation {
            let reused_bytes = reused.values().try_fold(0usize, |total, receipt| {
                total.checked_add(receipt.bytes().len())
            });
            let remaining =
                reused_bytes.and_then(|bytes| selection.response_bytes.checked_sub(bytes));
            let Some(remaining) = remaining else {
                return Err(build::PublicationSetError::Selection(
                    "retained documentation receipts exceed the whole-set response bound",
                ));
            };
            // Pure reuse needs no reservation. One byte makes any new effect fail its native
            // reservation when previous receipts have consumed the complete response allowance.
            selection.response_bytes = remaining.max(1);
        }
        build::documentation_generation::generate_acquired(
            inputs,
            documents,
            build::documentation_generation::GenerationLimits {
                acquisition: options.acquisition.sources,
                output,
            },
            &catalogs,
            remaining_generation.as_ref(),
            preview_links.as_ref(),
            |execution| match execution {
                DocumentationPublicationExecution::Ready { inputs, receipts } => {
                    reused.extend(
                        receipts
                            .iter()
                            .map(|receipt| (receipt.site.clone(), receipt.clone())),
                    );
                    let receipts = reused.values().cloned().collect::<Vec<_>>();
                    {
                        let mut cache = cache.borrow_mut();
                        cache.catalogs = inputs
                            .sites()
                            .iter()
                            .filter_map(|(site, result)| {
                                Some((
                                    site.clone(),
                                    result.as_ref().ok()?.documentation_catalog_capture()?,
                                ))
                            })
                            .collect();
                        cache.receipts = reused;
                        cache.failed = None;
                        cache.pending = receipts.clone();
                    }
                    if !receipts.is_empty()
                        && let Err(error) = deliver.borrow_mut()(DocumentationDelivery::Complete {
                            receipts: &receipts,
                        })
                    {
                        *fatal.borrow_mut() = Some(error);
                        return Err(build::PublicationSetError::Selection(
                            "documentation receipt delivery failed",
                        ));
                    }
                    Ok(())
                }
                DocumentationPublicationExecution::Refused { receipts, error } => {
                    reused.extend(
                        receipts
                            .iter()
                            .map(|receipt| (receipt.site.clone(), receipt.clone())),
                    );
                    let receipts = reused.values().cloned().collect::<Vec<_>>();
                    cache.borrow_mut().pending = receipts.clone();
                    if let Err(error) = deliver.borrow_mut()(DocumentationDelivery::Refused {
                        receipts: &receipts,
                        error,
                    }) {
                        *fatal.borrow_mut() = Some(error);
                    }
                    Err(build::PublicationSetError::Selection(
                        "documentation attempt refused; original receipts delivered",
                    ))
                }
                DocumentationPublicationExecution::Native {
                    site,
                    receipts,
                    execution,
                } => {
                    reused.extend(
                        receipts
                            .iter()
                            .map(|receipt| (receipt.site.clone(), receipt.clone())),
                    );
                    let prior = reused.values().cloned().collect::<Vec<_>>();
                    let contract = matches!(
                        &execution,
                        build::documentation_output::Execution::Contract { .. }
                    );
                    if let build::documentation_output::Execution::Delivered { response, .. } =
                        &execution
                    {
                        let receipt = DocumentationReceipt::retained(
                            site.clone(),
                            build::documentation_output::Encoded {
                                bytes: response.bytes.clone(),
                                is_error: response.is_error,
                            },
                        );
                        let mut all = prior.clone();
                        all.push(receipt);
                        let mut cache = cache.borrow_mut();
                        cache.pending = all.clone();
                        cache.failed = Some((identities, all));
                    } else {
                        cache.borrow_mut().pending = prior.clone();
                    }
                    if let Err(error) = deliver.borrow_mut()(DocumentationDelivery::Native {
                        site,
                        receipts: &prior,
                        execution,
                    }) {
                        *fatal.borrow_mut() = Some(error);
                    }
                    if contract && fatal.borrow().is_none() {
                        *fatal.borrow_mut() = Some(io::Error::other(
                            "documentation native custody contract failed; original custody delivered; no retry",
                        ));
                    }
                    Err(build::PublicationSetError::Selection(
                        "documentation native attempt failed; original custody delivered",
                    ))
                }
            },
        )
    };
    let result = watch::run_with_documentation_assembler(
        request,
        options,
        supplied,
        Some(&assemble),
        |event| {
            if let Some(error) = fatal.borrow_mut().take() {
                return Err(error);
            }
            let closed = matches!(event, watch::Event::Closed);
            if let Some(mut event) = watch_event(event, local)? {
                let mut cache = cache.borrow_mut();
                match &mut event {
                    Event::Ready { state } | Event::Changed { state } => {
                        cache.current = cache.pending.clone();
                        state.documentation = cache.current.clone();
                    }
                    Event::Recovered { state } => state.documentation = cache.current.clone(),
                    Event::RebuildFailed { documentation, .. } => {
                        *documentation = cache.pending.clone()
                    }
                    Event::Closed => {}
                }
                drop(cache);
                observe(event)?;
            }
            if closed {
                Ok(watch::Control::Continue)
            } else {
                control()
            }
        },
    );
    if let Some(error) = fatal.borrow_mut().take() {
        return Err(Error::Observer(error));
    }
    result.map_err(Error::Watch)
}
