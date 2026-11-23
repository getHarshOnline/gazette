//! Development generations over fixed bindings and continuously held listeners.
//!
//! Polling is only an acquisition trigger. A fresh confined capture and the same
//! compiler determine change; filesystem timestamps and event names are not truth.

use crate::build::{self, AcquiredPublicationSet};
use gazette_core::preview;
use gazette_ports::{
    preview::{PreviewReservation, PreviewServer, PreviewSession},
    render::TemplateCompiler,
    resource::ResourceCompiler,
    source::{RevisionReader, SourceCapture},
};
use std::{
    collections::BTreeMap,
    io, thread,
    time::{Duration, Instant},
};

mod contract;
mod generation;
mod serving;
use contract::Notification;
pub use contract::{BuildError, Control, Error, Event, Options, Request};
pub use generation::{Generation, InputIdentity};

/// Native mechanisms selected by the caller. No provider discovery occurs here.
pub struct Adapters<'a, S, V, T, R, P> {
    /// Confined capture of current native bytes.
    pub source: &'a S,
    /// Verification of the unchanged configured repository/parent pins.
    pub revisions: &'a V,
    /// The same template compiler used by ordinary publication rendering.
    pub templates: &'a T,
    /// The same resource compiler used by ordinary publication rendering.
    pub resources: &'a R,
    /// Bounded raster inspection selected by the caller.
    pub raster: &'a gazette_core::raster::Inspect<'a>,
    /// The same native Markdown parser used by ordinary publication rendering.
    pub markdown: &'a dyn gazette_ports::markdown::Markdown,
    /// Pure hosting files selected by the caller; configured hosting requires this port.
    pub host_files: Option<&'a dyn gazette_ports::hosting::HostFiles>,
    /// Explicit native capture runner, invoked only for selected acquisition effects.
    pub runner: &'a dyn gazette_ports::process::GenerationRunner,
    /// Fixed embedded helper supplied by native composition; never a workspace script.
    pub social_card_helper: Option<(
        &'a gazette_core::source::CapturedSource,
        &'a gazette_core::source::EntryPath,
    )>,
    /// Loopback transport; reservations remain held across generation replacement.
    pub server: &'a P,
}

/// Optional explicit native documentation assembly selected outside the application loop.
pub type DocumentationAssembler<'a> = dyn Fn(
        &build::DocumentationInputs,
        &mut AcquiredPublicationSet,
    ) -> Result<(), build::PublicationSetError>
    + 'a;

/// Existing provider mechanism and explicitly supplied immutable documentation results.
pub struct Documentation<'a> {
    /// Explicit existing generation/delivery bridge; absent keeps imported-only assembly.
    pub assemble: Option<&'a DocumentationAssembler<'a>>,
    /// The existing target adapter; no provider discovery or alternate parser.
    pub target: &'a dyn gazette_ports::documentation::DocumentationTarget,
    /// Results from an earlier explicitly authorized native documentation operation.
    pub supplied: &'a BTreeMap<
        gazette_compiler::workspace::Key,
        std::sync::Arc<crate::build::DocumentationCatalog>,
    >,
    /// Whole target-set assembly bounds.
    pub output: gazette_core::source::Limits,
}

/// Run explicit dirty/content-addressed development preview without changing pins,
/// source files, configuration or persistent state. Configuration is the fixed
/// request, not a watched file; changing its bindings requires a new run.
///
/// Each interval captures the complete selected universe through the existing
/// confined reader. Different retained bytes trigger the shared compiler once.
/// Polling catches additions, deletion, rename and atomic saves without trusting
/// event delivery or mtime. Same-content bursts coalesce. A scoped transport worker
/// serves the retained output during bounded capture/build; no worker escapes the
/// operation, and only a complete successful generation replaces those bytes.
/// Listener reservations must support thread transfer; source readers and compilers
/// remain on the calling thread and need no additional concurrency bounds.
/// No browser auto-refresh script is injected into authored/Theme output.
///
/// # Errors
/// Startup failure yields no ready set. Observer or transport failure terminates
/// and releases listeners. Later source/render failures instead emit an explicit
/// stale-generation event and keep the prior complete output until recovery.
pub fn run<S, V, T, R, P>(
    adapters: Adapters<'_, S, V, T, R, P>,
    request: Request<'_>,
    options: Options,
    observe: impl FnMut(Event<'_>) -> io::Result<Control>,
) -> Result<(), Error>
where
    S: SourceCapture,
    V: RevisionReader,
    T: TemplateCompiler,
    R: ResourceCompiler,
    P: PreviewServer,
    P::Reservation: Send,
{
    run_with_documentation(adapters, request, options, None, observe)
}

/// Run the same watch lifecycle with configured documentation capture and assembly.
///
/// # Errors
/// Preserves native source, documentation, rendering and transport failures without replacing a good generation.
pub fn run_with_documentation<S, V, T, R, P>(
    adapters: Adapters<'_, S, V, T, R, P>,
    request: Request<'_>,
    options: Options,
    documentation: Option<Documentation<'_>>,
    mut observe: impl FnMut(Event<'_>) -> io::Result<Control>,
) -> Result<(), Error>
where
    S: SourceCapture,
    V: RevisionReader,
    T: TemplateCompiler,
    R: ResourceCompiler,
    P: PreviewServer,
    P::Reservation: Send,
{
    options.validate()?;
    if request.render.preview_urls.is_some()
        || !request.render.sites.keys().eq(request.addresses.keys())
        || request
            .build_selection
            .sites
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            != request.render.sites.keys().collect()
    {
        return Err(Error::Options(
            "watch requires exact site bindings and listener-owned preview URLs",
        ));
    }
    let sites = &request.build_selection.sites;
    let acquire = || {
        let inputs = build::acquire_development_selected_publications(
            adapters.source,
            adapters.revisions,
            request.workspace.clone(),
            &request.build_selection,
            options.acquisition,
        )
        .map_err(BuildError::Publication)?;
        let failed: BTreeMap<_, _> = inputs
            .sites()
            .iter()
            .filter_map(|(key, result)| {
                result
                    .as_ref()
                    .err()
                    .map(|error| (key.clone(), error.to_string()))
            })
            .collect();
        if failed.is_empty() {
            let documents = build::acquire_publication_documentation(
                adapters.source,
                adapters.revisions,
                adapters.markdown,
                &inputs,
                options.acquisition.sources,
            )
            .map_err(BuildError::Publication)?;
            Ok((inputs, documents))
        } else {
            Err(BuildError::Incomplete(failed))
        }
    };
    let generate =
        |(mut inputs, documents): (AcquiredPublicationSet, build::DocumentationInputs)| {
            if let Some(documentation) = &documentation {
                if let Some(assemble) = documentation.assemble {
                    assemble(&documents, &mut inputs).map_err(BuildError::Publication)?;
                } else {
                    documents
                        .assemble_into(
                            documentation.target,
                            &mut inputs,
                            documentation.supplied,
                            documentation.output,
                        )
                        .map_err(BuildError::Publication)?;
                }
            } else if !documents.is_empty() {
                return Err(BuildError::Publication(
                    build::PublicationSetError::Selection(
                        "configured documentation requires its target adapter",
                    ),
                ));
            }
            if let Some(generation) = options.math_generation {
                inputs
                    .acquire_math(
                        adapters.runner,
                        options.render.publication.corpus,
                        adapters.markdown,
                        generation,
                    )
                    .map_err(|error| {
                        BuildError::Publication(build::PublicationSetError::Math(Box::new(error)))
                    })?;
            }
            if let Some(generation) = options.diagram_generation {
                inputs
                    .acquire_diagrams(
                        adapters.runner,
                        adapters.raster,
                        options.render.publication.corpus,
                        adapters.markdown,
                        generation,
                    )
                    .map_err(|error| {
                        BuildError::Publication(build::PublicationSetError::Diagram(Box::new(
                            error,
                        )))
                    })?;
            }
            Ok(inputs)
        };
    let initial = acquire().and_then(generate).map_err(Error::Build)?;
    let mut reserved = super::reserve(
        adapters.server,
        &initial,
        request.addresses,
        options.listeners,
    )
    .map_err(Error::Transport)?;
    let local_urls = reserved.urls().clone();
    let addresses = reserved.addresses().clone();
    let mut recipes = request.render;
    if options.local_urls {
        recipes.preview_urls = Some(local_urls.clone());
    }
    let render = |inputs: AcquiredPublicationSet| {
        let identity = InputIdentity::of(&inputs);
        let rendered = if let Some(generation) = options.social_card_generation {
            let (helper, entrypoint) = adapters.social_card_helper.ok_or_else(|| {
                BuildError::Publication(crate::build::PublicationSetError::Selection(
                    "social card helper must be supplied for explicit capture",
                ))
            })?;
            inputs.render_local_with_generations(
                adapters.templates,
                adapters.resources,
                adapters.raster,
                recipes.clone(),
                options.render,
                adapters.markdown,
                adapters.host_files,
                crate::build::SocialCardGeneration {
                    runner: adapters.runner,
                    helper,
                    entrypoint,
                    options: generation,
                },
            )
        } else {
            inputs.render_local_with_host_files(
                adapters.templates,
                adapters.resources,
                adapters.raster,
                recipes.clone(),
                options.render,
                adapters.markdown,
                adapters.host_files,
            )
        }
        .map_err(BuildError::Publication)?;
        let failed: BTreeMap<_, _> = rendered
            .sites()
            .iter()
            .filter_map(|(key, result)| {
                result
                    .as_ref()
                    .err()
                    .map(|error| (key.clone(), error.to_string()))
            })
            .collect();
        if !failed.is_empty() {
            return Err(BuildError::Incomplete(failed));
        }
        let urls = sites
            .iter()
            .map(|key| {
                let base = recipes
                    .preview_urls
                    .as_ref()
                    .and_then(|urls| urls.get(key))
                    .or_else(|| inputs.compilation().workspace.public_base_url(key))
                    .ok_or(BuildError::Preview(preview::Error::Refused(
                        "preview site lacks its configured base",
                    )))?;
                Ok((key.clone(), base.clone()))
            })
            .collect::<Result<_, BuildError>>()?;
        let sites = rendered.into_preview_sites().map_err(BuildError::Preview)?;
        Ok(Generation {
            inputs,
            sites,
            urls,
            identity,
        })
    };
    let mut current = render(initial).map_err(Error::Build)?;
    let mut generation = 1u64;
    let mut notification = Some(Notification::Ready);
    let mut stale = false;
    let mut last_failure = None;
    // A selected typesetting generation is not retried merely because polling repeats,
    // including when acquisition succeeded but later pure rendering failed.
    let mut failed_generation: Option<(InputIdentity, BuildError)> = None;
    loop {
        let mut session = reserved
            .attach(current.output())
            .map_err(Error::Transport)?;
        let event = notification.take().map(|kind| match kind {
            Notification::Ready => Event::Ready {
                generation,
                current: &current,
                addresses: &addresses,
                local_urls: &local_urls,
            },
            Notification::Rebuilt => Event::Rebuilt {
                generation,
                current: &current,
                addresses: &addresses,
                local_urls: &local_urls,
            },
            Notification::Recovered => Event::Recovered {
                generation,
                current: &current,
                addresses: &addresses,
                local_urls: &local_urls,
            },
        });
        let mut stop = if let Some(event) = event {
            observe(event).map_err(Error::Observer)? == Control::Stop
        } else {
            false
        };
        let until = Instant::now() + options.interval;
        while !stop && Instant::now() < until {
            stop = observe(Event::Poll).map_err(Error::Observer)? == Control::Stop;
            if stop {
                break;
            }
            match session.poll().map_err(Error::Transport)? {
                preview::Poll::Idle => thread::sleep(Duration::from_millis(10)),
                preview::Poll::Responded | preview::Poll::Disconnected => (),
            }
        }
        reserved = session.detach();
        if stop {
            break;
        }
        let mut repeated_failure = false;
        let mut attempted_identity = None;
        let (reservation, candidate) = serving::during(reserved, current.output(), || {
            let (inputs, documents) = acquire()?;
            let mut identity = InputIdentity::of(&inputs);
            identity.documentation = documents.source_identities();
            if failed_generation
                .as_ref()
                .is_some_and(|(failed, _)| failed == &identity)
            {
                repeated_failure = true;
                return Ok(None);
            }
            if identity == current.identity {
                Ok(None)
            } else {
                attempted_identity = Some(identity);
                generate((inputs, documents)).and_then(&render).map(Some)
            }
        })?;
        reserved = reservation;
        match candidate {
            Ok(Some(next)) => {
                last_failure = None;
                failed_generation = None;
                generation = generation.checked_add(1).ok_or(Error::Exhausted)?;
                current = next;
                notification = Some(Notification::Rebuilt);
                stale = false;
            }
            Ok(None) => {
                if repeated_failure {
                    if !stale {
                        stale = true;
                        let error = &failed_generation.as_ref().ok_or(Error::Exhausted)?.1;
                        if observe(Event::RebuildFailed { generation, error })
                            .map_err(Error::Observer)?
                            == Control::Stop
                        {
                            break;
                        }
                    }
                    continue;
                }
                if stale {
                    notification = Some(Notification::Recovered);
                }
                stale = false;
                last_failure = None;
            }
            Err(error) => {
                stale = true;
                let notice = (
                    attempted_identity.clone(),
                    gazette_core::source::Sha256Digest::of(
                        format!("{}\0{error}", error.code()).as_bytes(),
                    ),
                );
                let changed_failure = last_failure.as_ref() != Some(&notice);
                last_failure = Some(notice);
                let error = if (options.math_generation.is_some()
                    || options.diagram_generation.is_some())
                    && let Some(identity) = attempted_identity
                {
                    failed_generation = Some((identity, error));
                    &failed_generation.as_ref().ok_or(Error::Exhausted)?.1
                } else {
                    &error
                };
                if changed_failure
                    && observe(Event::RebuildFailed { generation, error })
                        .map_err(Error::Observer)?
                        == Control::Stop
                {
                    break;
                }
            }
        }
    }
    drop(reserved);
    observe(Event::Closed).map_err(Error::Observer)?;
    Ok(())
}
