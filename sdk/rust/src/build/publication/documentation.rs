//! Local publication rendering with explicit native documentation delivery custody.

use super::*;
use crate::build::{
    self, DocumentationPublicationExecution, DocumentationPublicationFailure, DocumentationReceipt,
};

/// One complete local rendering result, preserving earlier documentation effects on every failure.
pub enum DocumentationRenderExecution<'a> {
    /// The same complete publication renderer produced its normal result.
    Rendered {
        /// Original complete set; site-local errors remain in the ordinary result.
        rendered: &'a build::RenderedPublicationSet<'a>,
        /// Original native documentation receipts, empty when no generators ran.
        receipts: &'a [DocumentationReceipt],
    },
    /// Acquisition, catalog attachment or later rendering failed.
    Refused {
        /// Every completed documentation operation preceding this failure.
        receipts: &'a [DocumentationReceipt],
        /// Original failure, never a fabricated successful subset.
        error: DocumentationPublicationFailure,
    },
    /// Failed native delivery remains inside the existing documentation operation callback.
    Native {
        /// Exact documentation owner.
        site: &'a Key,
        /// Earlier completed deliveries.
        receipts: &'a [DocumentationReceipt],
        /// Original current delivery, including borrowed exceptional Contract custody.
        execution: build::documentation_output::Execution<'a>,
    },
}
impl ResolvedLocalPublicationRecipe {
    /// Explicit staging/execution request after invocation permission is resolved.
    pub fn documentation_generation(&self) -> Option<&build::DocumentationGenerationRequest> {
        self.publication.documentation_generation()
    }
    /// Narrow native receipt capacity and invocation permission before acquisition or effects.
    /// A transport cannot enlarge the recipe's selected bounds or grant execution here.
    ///
    /// # Errors
    /// Refuses selected generation when no custody response capacity remains.
    pub fn restrict_documentation_delivery(
        &mut self,
        maximum: usize,
        allow_execute: bool,
    ) -> Result<(), &'static str> {
        self.publication
            .restrict_documentation_delivery(maximum, allow_execute)
    }
    /// Render with an immediate consumer for complete native generation receipts and failures.
    /// Source/configuration acquisition remains immutable and generation never repeats candidate checks.
    ///
    /// # Errors
    /// Returns only failures before documentation effects; later failures reach the callback with custody.
    pub fn with_rendered_documentation<T>(
        &self,
        acquisition: build::PublicationSetAcquisitionLimits,
        output: source::Limits,
        render: build::LocalRenderLimits,
        consume: impl FnOnce(DocumentationRenderExecution<'_>) -> T,
    ) -> Result<T, build::PublicationSetError> {
        let render = self.render_limits(render);
        let mut options = self.options(output);
        if self.addressing == PublicationAddressing::Local {
            options.preview_urls = Some(
                self.configuration
                    .compilation()
                    .workspace
                    .local_urls(&self.build_selection.sites)
                    .map_err(build::PublicationSetError::Selection)?,
            );
        }
        let mut inputs = build::acquire_selected_publications(
            self.configuration.clone(),
            &self.build_selection,
            acquisition,
        )?;
        Ok(build::documentation_generation::generate(
            &mut inputs,
            acquisition.sources,
            output,
            self.documentation_catalogs(),
            self.documentation_generation(),
            self.publication.documentation_preview_links(),
            |execution| match execution {
                DocumentationPublicationExecution::Ready { inputs, receipts } => {
                    let render_result = (|| {
                        self.publication
                            .acquire_math(inputs, render.publication.corpus)
                            .map_err(|error| build::PublicationSetError::Math(Box::new(error)))?;
                        self.publication
                            .acquire_diagrams(inputs, render.publication.corpus)
                            .map_err(|error| {
                                build::PublicationSetError::Diagram(Box::new(error))
                            })?;
                        build::render_publications_with_generations(
                            inputs,
                            options,
                            render,
                            self.social_card_generation(),
                        )
                    })();
                    match render_result {
                        Ok(rendered) => consume(DocumentationRenderExecution::Rendered {
                            rendered: &rendered,
                            receipts,
                        }),
                        Err(error) => consume(DocumentationRenderExecution::Refused {
                            receipts,
                            error: DocumentationPublicationFailure::Publication(error),
                        }),
                    }
                }
                DocumentationPublicationExecution::Refused { receipts, error } => {
                    consume(DocumentationRenderExecution::Refused { receipts, error })
                }
                DocumentationPublicationExecution::Native {
                    site,
                    receipts,
                    execution,
                } => consume(DocumentationRenderExecution::Native {
                    site,
                    receipts,
                    execution,
                }),
            },
        ))
    }
}
