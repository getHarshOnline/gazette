//! Native adapter selection for the shared development-preview lifecycle.

pub use gazette_application::preview::watch::{
    BuildError, Control, Error, Event, Generation, InputIdentity, Options, Request,
};

/// Run a fixed development preview with native capture, compilation and HTTP.
///
/// # Errors
/// Preserves the application's startup, observer and transport failures. Later
/// rebuild failures are explicit events retaining the prior stale generation.
pub fn run(
    request: Request<'_>,
    options: Options,
    observe: impl FnMut(Event<'_>) -> std::io::Result<Control>,
) -> Result<(), Error> {
    run_with_documentation(
        request,
        options,
        &std::collections::BTreeMap::new(),
        observe,
    )
}

/// Use explicit validated documentation captures from the existing native operation.
///
/// # Errors
/// Preserves documentation/source mismatches and the ordinary preview lifecycle failures.
pub fn run_with_documentation(
    request: Request<'_>,
    options: Options,
    supplied: &std::collections::BTreeMap<
        crate::workspace::Key,
        std::sync::Arc<crate::build::DocumentationCatalog>,
    >,
    observe: impl FnMut(Event<'_>) -> std::io::Result<Control>,
) -> Result<(), Error> {
    run_with_documentation_assembler(request, options, supplied, None, observe)
}

/// Select an explicit existing documentation generation bridge within the same watch lifecycle.
///
/// # Errors
/// Preserves native capture, delivery and preview failures through their existing owners.
pub fn run_with_documentation_assembler(
    request: Request<'_>,
    options: Options,
    supplied: &std::collections::BTreeMap<
        crate::workspace::Key,
        std::sync::Arc<crate::build::DocumentationCatalog>,
    >,
    assemble: Option<&gazette_application::preview::watch::DocumentationAssembler<'_>>,
    observe: impl FnMut(Event<'_>) -> std::io::Result<Control>,
) -> Result<(), Error> {
    let resources = gazette_application::build::ResourceCache::new(
        crate::build::resources::Compiler,
        Default::default(),
    );
    let helper = if options.social_card_generation.is_some() {
        Some(
            crate::build::social_cards::helper()
                .map_err(|_| Error::Options("embedded social-card helper is invalid"))?,
        )
    } else {
        None
    };
    let preview_links = request.render.preview_link_bases.clone();
    let target = gazette_adapter_mintlify::Mintlify::new(&gazette_adapter_markdown::Comrak)
        .with_preview_link_bases(preview_links.as_ref());
    let documentation_output = gazette_core::source::Limits {
        entries: options.render.entries,
        total_bytes: options.render.bytes,
        ..Default::default()
    };
    gazette_application::preview::watch::run_with_documentation(
        gazette_application::preview::watch::Adapters {
            source: &gazette_adapter_fs::FileSystem,
            revisions: &gazette_adapter_git::Git,
            templates: &gazette_adapter_tera::Compiler,
            resources: &resources,
            raster: &crate::raster::inspect,
            markdown: &gazette_adapter_markdown::Comrak,
            host_files: Some(&gazette_integration_github::HostFiles),
            runner: &gazette_adapter_process::Runner,
            social_card_helper: helper.as_ref().map(|(tree, entry)| (tree, entry)),
            server: &gazette_adapter_http::Http,
        },
        request,
        options,
        Some(gazette_application::preview::watch::Documentation {
            assemble,
            target: &target,
            supplied,
            output: documentation_output,
        }),
        observe,
    )
}
