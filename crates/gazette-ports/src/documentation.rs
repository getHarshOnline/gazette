//! Documentation targets consume retained manuals and generator artifacts without native effects.

use gazette_core::{
    documentation::{Error, Plan, Request},
    source::{CapturedSource, Limits},
};

/// Adapt a complete selected publication without acquiring inputs or choosing its destination.
pub trait DocumentationTarget {
    /// Explicit preview-only manual hyperlink bases, retained by the application
    /// to prevent local addresses entering durable preparation or maintained output.
    fn preview_link_bases(&self) -> Option<&gazette_core::publication::routing::PreviewLinkBases> {
        None
    }

    /// Validate the complete byte-free target shape before any configured generator effect.
    ///
    /// # Errors
    /// Reject unsupported presentation, namespaces or declared formats. This method must not
    /// read artifact bytes, invoke native tools, access a network or create target output.
    fn preflight(&self, plan: Plan<'_>) -> Result<(), Error>;

    /// Return one complete local tree, not a hosted build or publication receipt.
    ///
    /// # Errors
    /// Reject unsupported inputs, missing references, malformed artifacts and output limits.
    /// No native reads, generator process, network fallback or partial success is permitted.
    fn assemble(&self, request: Request<'_>, limits: Limits) -> Result<CapturedSource, Error>;

    /// Assemble output and optional validated discovery facts through the same target owner.
    /// Older adapters explicitly report unavailable catalog support rather than an empty catalog.
    /// # Errors
    /// Preserves target errors and rejects exhausted output or catalog bounds.
    fn assemble_with_catalog(
        &self,
        request: Request<'_>,
        limits: Limits,
        _catalog_limits: gazette_core::documentation::CatalogLimits,
    ) -> Result<gazette_core::documentation::TargetAssembly, Error> {
        self.assemble(request, limits)
            .map(|output| gazette_core::documentation::TargetAssembly {
                output,
                catalog: None,
            })
    }
}

/// Explicit native generation into separately retained work directories, outside compilation.
pub trait Generator {
    /// Verify inputs, invoke the exact selected tool and retain the complete generated tree.
    /// This does not create stages, clean up, retry, validate a hosting target or grant publication.
    ///
    /// # Errors
    /// Preserves original job locators, completed phase evidence and process custody uncertainty.
    fn generate(
        &self,
        request: gazette_core::documentation::generator::Request<'_>,
    ) -> Result<
        gazette_core::documentation::generator::Generated,
        gazette_core::documentation::generator::Error,
    >;
}
