//! Set orchestration around the existing native documentation operation and its delivery owner.

use super::*;
use documentation_output::{AcquiredOperation, Execution, GenerationSelection, RetentionSelection};
use gazette_compiler::workspace::DocumentationInput;
use gazette_ports::destination::RootSeparation;

pub(crate) struct GenerationLimits {
    pub(crate) acquisition: source::PreparationLimits,
    pub(crate) output: source::Limits,
}

pub(crate) fn generate<T>(
    inputs: &mut AcquiredPublicationSet,
    acquisition: source::PreparationLimits,
    output: source::Limits,
    supplied: &Catalogs,
    request: Option<&DocumentationGenerationRequest>,
    preview_links: Option<&gazette_core::publication::routing::PreviewLinkBases>,
    consume: impl FnOnce(DocumentationPublicationExecution<'_>) -> T,
) -> T {
    let documents = match gazette_application::build::acquire_publication_documentation(
        &gazette_adapter_fs::FileSystem,
        &gazette_adapter_git::Git,
        &gazette_adapter_markdown::Comrak,
        inputs,
        acquisition,
    ) {
        Ok(value) => value,
        Err(error) => {
            return consume(DocumentationPublicationExecution::Refused {
                receipts: &[],
                error: DocumentationPublicationFailure::Publication(error),
            });
        }
    };
    generate_acquired(
        inputs,
        &documents,
        GenerationLimits {
            acquisition,
            output,
        },
        supplied,
        request,
        preview_links,
        consume,
    )
}

pub(crate) fn generate_acquired<T>(
    inputs: &mut AcquiredPublicationSet,
    documents: &gazette_application::build::DocumentationInputs,
    limits: GenerationLimits,
    supplied: &Catalogs,
    request: Option<&DocumentationGenerationRequest>,
    preview_links: Option<&gazette_core::publication::routing::PreviewLinkBases>,
    consume: impl FnOnce(DocumentationPublicationExecution<'_>) -> T,
) -> T {
    let GenerationLimits {
        acquisition,
        output,
    } = limits;
    let mut receipts = Vec::new();
    let mut catalogs = supplied.clone();
    let mut consume = Some(consume);
    macro_rules! refuse {
        ($error:expr) => {{
            return consume.take().expect("one final delivery")(
                DocumentationPublicationExecution::Refused {
                    receipts: &receipts,
                    error: $error,
                },
            );
        }};
    }
    let mut target_remaining = match documents.check_supplied(inputs, supplied, output) {
        Ok(value) => value,
        Err(error) => refuse!(DocumentationPublicationFailure::Publication(error)),
    };
    if let Some(request) = request {
        if let Err(reason) = request.validate() {
            refuse!(DocumentationPublicationFailure::Selection(reason));
        }
        let mut plans = Vec::new();
        let mut job_total = 0usize;
        let mut input_entries = 0usize;
        let mut input_bytes = 0u64;
        for (site, acquired) in documents.sites() {
            if supplied.contains_key(site) {
                continue;
            }
            let Some(build) = acquired
                .compilation()
                .workspace
                .publication_for(site)
                .and_then(|publication| publication.documentation_repository.as_ref())
                .and_then(|repository| repository.build.as_ref())
            else {
                refuse!(DocumentationPublicationFailure::Selection(
                    "acquired documentation owner lost its build"
                ));
            };
            let mut count = 0usize;
            let mut entries = 0usize;
            let mut bytes = 0u64;
            for module in &build.modules {
                for (_, _, input) in module
                    .navigation
                    .iter()
                    .filter_map(gazette_compiler::workspace::DocumentationNavigation::reference)
                {
                    if let DocumentationInput::Generated { source, .. } = input {
                        count = count.saturating_add(1);
                        let Some(capture) = acquired.sources().get(source) else {
                            refuse!(DocumentationPublicationFailure::Selection(
                                "generated source is not retained"
                            ));
                        };
                        entries = entries.saturating_add(capture.inventory().entries().len());
                        bytes = bytes.saturating_add(capture.inventory().total_bytes());
                    }
                }
            }
            if count > 0 {
                plans.push((site, acquired, count, entries, bytes));
            }
            job_total = job_total.saturating_add(count);
            input_entries = input_entries.saturating_add(entries);
            input_bytes = input_bytes.saturating_add(bytes);
        }
        if job_total > request.jobs
            || input_entries > acquisition.entries
            || input_bytes > acquisition.bytes
        {
            refuse!(DocumentationPublicationFailure::Bounds {
                observed: (job_total, input_entries, input_bytes),
                maximum: (request.jobs, acquisition.entries, acquisition.bytes),
            });
        }
        let mut roots = match inputs.source_roots() {
            Ok(value) => value,
            Err(error) => refuse!(DocumentationPublicationFailure::Publication(
                PublicationSetError::Acquisition(error)
            )),
        };
        match documents.source_roots() {
            Ok(value) => roots.extend(value),
            Err(error) => refuse!(DocumentationPublicationFailure::Publication(error)),
        }
        for catalog in supplied.values() {
            roots.extend_from_slice(catalog.protected_roots());
        }
        roots.sort();
        roots.dedup();
        if !plans.is_empty()
            && let Err(error) =
                gazette_adapter_fs::FileSystem.check_root_separation(&request.parent, &roots)
        {
            refuse!(DocumentationPublicationFailure::Separation(error));
        }
        let mut generated_remaining = output;
        let mut receipt_remaining = request.response_bytes;
        let mut stage_remaining = source::Limits {
            entries: acquisition.entries,
            total_bytes: acquisition.bytes,
            ..acquisition.per_source.content
        };
        for (site, acquired, count, entries, bytes) in plans {
            let options = request.options(stage_remaining, generated_remaining, count);
            let result = AcquiredOperation {
                inputs: acquired,
                protected_roots: &roots,
                preview_link_bases: preview_links,
                output: target_remaining,
                generation: Some(GenerationSelection {
                    parent: &request.parent,
                    options,
                }),
                retention: RetentionSelection::None,
                response_bytes: receipt_remaining,
                reserve_read_only_response: true,
            }
            .execute_with_delivery(|execution| {
                match execution {
                    Execution::Delivered {
                        mut response,
                        assembly: Some(assembly),
                        jobs,
                    } if !response.is_error => {
                        let Some(catalog) = assembly.catalog_capture() else {
                            return Some(consume.take().expect("one final delivery")(
                                DocumentationPublicationExecution::Native {
                                    site,
                                    receipts: &receipts,
                                    execution: Execution::Delivered {
                                        response,
                                        assembly: Some(assembly),
                                        jobs,
                                    },
                                },
                            ));
                        };
                        let usage = assembly.generation_output_usage();
                        // Each operation received these remaining ceilings; subtraction cannot renew a budget.
                        generated_remaining.entries =
                            generated_remaining.entries.saturating_sub(usage.0);
                        generated_remaining.total_bytes =
                            generated_remaining.total_bytes.saturating_sub(usage.1);
                        target_remaining.entries = target_remaining
                            .entries
                            .saturating_sub(assembly.output().inventory().entries().len());
                        target_remaining.total_bytes = target_remaining
                            .total_bytes
                            .saturating_sub(assembly.output().inventory().total_bytes());
                        // Release the unused pre-effect reservation before retaining multiple receipts.
                        response.bytes = response.bytes.into_boxed_slice().into_vec();
                        receipt_remaining =
                            receipt_remaining.saturating_sub(response.bytes.capacity());
                        stage_remaining.entries = stage_remaining.entries.saturating_sub(entries);
                        stage_remaining.total_bytes =
                            stage_remaining.total_bytes.saturating_sub(bytes);
                        catalogs.insert(site.clone(), catalog);
                        receipts.push(DocumentationReceipt {
                            site: site.clone(),
                            is_error: response.is_error,
                            response: Arc::new(response),
                        });
                        None
                    }
                    execution => Some(consume.take().expect("one final delivery")(
                        DocumentationPublicationExecution::Native {
                            site,
                            receipts: &receipts,
                            execution,
                        },
                    )),
                }
            });
            if let Some(result) = result {
                return result;
            }
        }
    }
    let target = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .with_preview_link_bases(preview_links);
    if let Err(error) = documents.assemble_into(&target, inputs, &catalogs, output) {
        refuse!(DocumentationPublicationFailure::Publication(error));
    }
    consume.take().expect("one final delivery")(DocumentationPublicationExecution::Ready {
        inputs,
        receipts: &receipts,
    })
}
