//! One owned observation contract over pinned and development preview lifecycles.

use std::{collections::BTreeMap, fmt, io, net::SocketAddr, thread, time::Duration};

use schemars::JsonSchema;
use serde::Serialize;

use super::{PreviewReservation, PreviewSession, watch};
use crate::{build, effect, source, toml, workspace::Key};

mod documentation;

/// Input acquisition remains fixed for the lifetime of the run.
#[derive(Clone, Copy, Debug)]
pub enum Mode {
    /// Verify and capture the configured immutable sources once.
    Pinned,
    /// Reinspect dirty bytes under the original configured source bindings.
    Development {
        /// Delay between bounded acquisitions, not an event-delivery timeout.
        interval: Duration,
    },
}

/// Native acquisition, output and listener bounds; no interface owns rebuild policy.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Pinned capture or fixed-binding development acquisition.
    pub mode: Mode,
    /// Project all cross-site links through the held local listeners.
    pub local_urls: bool,
    /// Explicit listener effects and bounded HTTP work.
    pub listeners: super::Options,
    /// Workspace parsing ceilings.
    pub syntax: toml::Limits,
    /// Complete selected-source acquisition ceilings.
    pub acquisition: build::PublicationSetAcquisitionLimits,
    /// Per-site generated-output ceilings.
    pub output: source::Limits,
    /// Aggregate complete-set rendering ceilings.
    pub render: build::LocalRenderLimits,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Pinned,
            local_urls: false,
            listeners: Default::default(),
            syntax: Default::default(),
            acquisition: Default::default(),
            output: Default::default(),
            render: Default::default(),
        }
    }
}

impl Options {
    /// Validate native effects and interval bounds before source acquisition.
    ///
    /// # Errors
    /// Refuses unacknowledged listeners and unsupported transport/watch bounds.
    pub fn validate(&self) -> Result<(), Error> {
        self.listeners.validate().map_err(Error::Transport)?;
        if let Mode::Development { interval } = self.mode {
            watch::Options {
                interval,
                listeners: self.listeners,
                ..Default::default()
            }
            .validate()
            .map_err(Error::Watch)?;
        }
        Ok(())
    }
}

/// Local presentation mode, never publication certification.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub enum Status {
    /// Human and machine links use held loopback endpoints.
    #[serde(rename = "local-cross-site-noncertified")]
    Local,
    /// Links retain configured public bases while serving locally.
    #[serde(rename = "local-production-linked-noncertified")]
    ProductionLinked,
}

/// The native source-capture policy used for this generation.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
pub enum InputMode {
    /// Exact configured immutable inputs.
    #[serde(rename = "pinned")]
    Pinned,
    /// Dirty bytes captured under fixed configured repository bindings.
    #[serde(rename = "dirty-content-addressed")]
    Development,
}

/// Exact preview selection and effect classes resolved before native execution.
///
/// Requested listener addresses may still contain port zero; only execution binds
/// sockets and observes effective ports. This plan contains no host filesystem paths.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Plan {
    /// Developer-local or production-linked presentation.
    pub status: Status,
    /// One pinned acquisition or repeated fixed-binding development acquisition.
    pub input_mode: InputMode,
    /// Explicit invocation order, or absent until configured defaults are resolved.
    /// Requested listeners never stand in for the workspace's selected sites.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sites: Option<Vec<Key>>,
    /// Complete requested loopback bindings, before native port allocation.
    pub requested_listeners: BTreeMap<Key, SocketAddr>,
    /// Complete portable effect classification for this operation.
    pub effects: effect::Plan,
}

/// An actual listener and the immutable output currently attached to it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Site {
    /// Actual bound loopback endpoint, including resolved ephemeral port.
    pub address: SocketAddr,
    /// Native entry URL including its publication mount.
    pub url: String,
    /// Complete generated inventory identity, not a release certificate.
    pub output_identity: source::TreeIdentity,
}

/// Complete current generation; source and artifact identities have native owners.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct State {
    /// Local projection status.
    pub status: Status,
    /// Acquisition mode, not inferred from URL presentation.
    pub input_mode: InputMode,
    /// One-based process-local generation; pinned runs have one generation.
    pub generation: u64,
    /// A failed fresh acquisition leaves the retained generation explicitly stale.
    pub stale: bool,
    /// Exact workspace and captured-source identities.
    pub input_identity: watch::InputIdentity,
    /// Effective site order, selection owner and local draft policy from retained inputs.
    pub build_selection: crate::workspace::BuildSelection,
    /// Complete selected site set, never a reduced successful subset.
    pub sites: BTreeMap<Key, Site>,
    /// Original retained documentation generation receipts for this served state.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documentation: Vec<build::DocumentationReceipt>,
}

/// Owned observations shared by synchronous CLI and background SDK consumers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// First complete generation attached to held listeners.
    Ready {
        /// Exact initial state.
        #[serde(flatten)]
        state: State,
    },
    /// One complete replacement generation installed atomically.
    Changed {
        /// Exact replacement state.
        #[serde(flatten)]
        state: State,
    },
    /// Fresh input again matches retained bytes after a stale interval.
    Recovered {
        /// Retained generation, now current again.
        #[serde(flatten)]
        state: State,
    },
    /// Failed acquisition/rendering retained the last complete generation.
    RebuildFailed {
        /// Unchanged retained generation coordinate.
        generation: u64,
        /// Always true for this native failure observation.
        stale: bool,
        /// Redacted native failure class.
        code: &'static str,
        /// Original receipts for the refused attempt; the previous successful state remains retained.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        documentation: Vec<build::DocumentationReceipt>,
    },
    /// Listeners are released; the enclosing runner has not necessarily returned yet.
    Closed,
}

/// Startup, serving or observer failure with its original native cause.
#[derive(Debug)]
pub enum Error {
    /// Exact selection or observation linkage cannot be established.
    Request(&'static str),
    /// One selected site failed before the complete set could be served.
    Site {
        /// Exact site whose acquisition or rendering failed.
        site: Key,
        /// Original native failure class.
        code: &'static str,
        /// Private native diagnostic, never public page copy.
        message: String,
    },
    /// Shared acquisition or compilation failed.
    Publication(build::PublicationSetError),
    /// Native listener failure.
    Transport(super::Error),
    /// Shared development lifecycle failed.
    Watch(watch::Error),
    /// Observer/control delivery failed; listener scope has been released.
    Observer(io::Error),
}

impl Error {
    /// Stable redacted classification for interface delivery.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Request(_) => "preview_refused",
            Self::Site { code, .. } => code,
            Self::Publication(error) => error.code(),
            Self::Transport(error) => error.code(),
            Self::Watch(error) => error.code(),
            Self::Observer(_) => "preview_delivery",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(reason) => f.write_str(reason),
            Self::Site {
                site,
                code,
                message,
            } => write!(f, "{site}: {code}: {message}"),
            Self::Publication(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
            Self::Watch(error) => error.fmt(f),
            Self::Observer(error) => write!(f, "preview observer failed: {error}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Request(_) | Self::Site { .. } => None,
            Self::Publication(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::Watch(error) => Some(error),
            Self::Observer(error) => Some(error),
        }
    }
}

/// Validate complete explicit listener selection without opening native sources.
///
/// # Errors
/// Refuses bounds, incomplete sites, non-loopback or duplicated fixed endpoints.
pub fn validate(
    recipe: &build::LocalPublicationRecipe,
    addresses: &BTreeMap<Key, SocketAddr>,
    options: &Options,
) -> Result<(), Error> {
    plan(recipe, addresses, options).map(drop)
}

/// Resolve the same complete selection and effect contract consumed by execution.
///
/// This parses no additional input and performs no source, filesystem, listener,
/// process, network, Git or release operation.
///
/// # Errors
/// Refuses the same invalid bounds, permissions or incomplete listener selection as run.
pub fn plan(
    recipe: &build::LocalPublicationRecipe,
    addresses: &BTreeMap<Key, SocketAddr>,
    options: &Options,
) -> Result<Plan, Error> {
    options.validate()?;
    if recipe.addressing() == build::PublicationAddressing::Local && !options.local_urls {
        return Err(Error::Request(
            "configured local addressing requires local-URL preview projection",
        ));
    }
    if addresses.is_empty()
        || addresses.len() > options.listeners.max_sites
        || recipe.invocation_sites().is_some_and(|sites| {
            addresses.len() != sites.len()
                || !sites.iter().all(|site| addresses.contains_key(&site.site))
        })
    {
        return Err(Error::Request(
            "listen bindings must match the exact selected site set",
        ));
    }
    let mut fixed = std::collections::BTreeSet::new();
    for address in addresses.values() {
        if !address.ip().is_loopback() || (address.port() != 0 && !fixed.insert(*address)) {
            return Err(Error::Request(
                "listeners require distinct explicit loopback endpoints",
            ));
        }
    }
    Ok(Plan {
        status: status(options.local_urls),
        input_mode: match options.mode {
            Mode::Pinned => InputMode::Pinned,
            Mode::Development { .. } => InputMode::Development,
        },
        sites: recipe
            .invocation_sites()
            .map(|sites| sites.iter().map(|site| site.site.clone()).collect()),
        requested_listeners: addresses.clone(),
        effects: effect::Plan {
            source_reads: match options.mode {
                Mode::Pinned => effect::SourceReads::Once,
                Mode::Development { .. } => effect::SourceReads::Repeated,
            },
            derived_state_writes: recipe.documentation_generation().is_some(),
            output_writes: recipe.documentation_generation().is_some(),
            listeners: true,
            network: effect::NetworkEffects {
                loopback: true,
                outbound: false,
            },
            git: effect::GitEffects {
                reads: true,
                mutation: false,
            },
            release_authority_consumption: false,
            unconfined_process_execution: recipe.allows_generation_execution()
                || recipe.math_generation().is_some()
                || recipe.diagram_generation().is_some(),
        },
    })
}

/// Run the existing native preview lifecycle with owned observations and separate control.
/// A stop request applies at a checkpoint, not midway through a native operation.
///
/// # Errors
/// Preserves native acquisition/render/listener failures. Failed observer delivery
/// releases all listeners without manufacturing a successful closed receipt.
pub fn run(
    recipe: &build::LocalPublicationRecipe,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
    observe: impl FnMut(Event) -> io::Result<()>,
    control: impl FnMut() -> io::Result<watch::Control>,
) -> Result<(), Error> {
    if recipe.documentation_generation().is_some() {
        return Err(Error::Request(
            "documentation generation requires its immediate native delivery consumer",
        ));
    }
    run_with_documentation_delivery(recipe, addresses, options, observe, control, |_| Ok(()))
}

/// Run explicit documentation generation with immediate original native custody delivery.
/// Retained stages are part of the effect plan; every attempt delivers receipts before serving.
///
/// # Errors
/// Preserves ordinary preview failures; native documentation failures first reach the delivery callback.
pub fn run_with_documentation_delivery(
    recipe: &build::LocalPublicationRecipe,
    addresses: BTreeMap<Key, SocketAddr>,
    options: Options,
    mut observe: impl FnMut(Event) -> io::Result<()>,
    mut control: impl FnMut() -> io::Result<watch::Control>,
    mut deliver: impl FnMut(build::DocumentationDelivery<'_>) -> io::Result<()>,
) -> Result<(), Error> {
    plan(recipe, &addresses, &options)?;
    let resolved = recipe.resolve(options.syntax).map_err(Error::Publication)?;
    if addresses.len() != resolved.sites().count()
        || !resolved.sites().all(|site| addresses.contains_key(site))
    {
        return Err(Error::Request(
            "listen bindings must match the exact resolved build site set",
        ));
    }
    let render = resolved.render_limits(options.render);
    let configuration = &resolved.configuration;
    if recipe.addressing() == build::PublicationAddressing::Local {
        let selected = resolved.sites().cloned().collect::<Vec<_>>();
        let urls = configuration
            .compilation()
            .workspace
            .local_urls(&selected)
            .map_err(Error::Request)?;
        for site in urls.keys() {
            let requested = addresses
                .get(site)
                .ok_or(Error::Request("missing configured local listener"))?;
            let port = configuration
                .compilation()
                .workspace
                .publication_for(site)
                .and_then(|publication| publication.local_port)
                .ok_or(Error::Request("missing configured local port"))?;
            if requested.ip() != std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
                || requested.port() != port.get()
            {
                return Err(Error::Request(
                    "preview listener does not match configured local addressing",
                ));
            }
        }
    }
    if let Mode::Development { interval } = options.mode {
        return documentation::watch(
            watch::Request {
                workspace: configuration,
                build_selection: resolved.build_selection().clone(),
                render: resolved.options(options.output),
                addresses,
            },
            watch::Options {
                local_urls: options.local_urls,
                interval,
                listeners: options.listeners,
                syntax: options.syntax,
                acquisition: options.acquisition,
                render,
                math_generation: resolved.math_generation(),
                diagram_generation: recipe.diagram_generation(),
                social_card_generation: resolved.social_card_generation(),
            },
            resolved.documentation_catalogs(),
            resolved.documentation_generation(),
            &mut observe,
            &mut control,
            &mut deliver,
        );
    }
    let mut inputs = build::acquire_selected_publications(
        configuration.clone(),
        resolved.build_selection(),
        options.acquisition,
    )
    .map_err(Error::Publication)?;
    for (site, result) in inputs.sites() {
        if let Err(error) = result {
            return Err(Error::Site {
                site: site.clone(),
                code: error.code(),
                message: error.to_string(),
            });
        }
    }
    build::documentation_generation::generate(
        &mut inputs,
        options.acquisition.sources,
        options.output,
        resolved.documentation_catalogs(),
        resolved.documentation_generation(),
        resolved.options(options.output).preview_link_bases.as_ref(),
        |execution| match execution {
            build::DocumentationPublicationExecution::Ready { inputs, receipts } => {
                if !receipts.is_empty() {
                    deliver(build::DocumentationDelivery::Complete { receipts })
                        .map_err(Error::Observer)?;
                }
                resolved
                    .acquire_math(inputs, render.publication.corpus)
                    .map_err(|error| {
                        Error::Publication(build::PublicationSetError::Math(Box::new(error)))
                    })?;
                recipe
                    .acquire_diagrams(inputs, render.publication.corpus)
                    .map_err(|error| {
                        Error::Publication(build::PublicationSetError::Diagram(Box::new(error)))
                    })?;
                let identity = watch::InputIdentity::of(inputs);
                let reserved = if options.local_urls {
                    Some(
                        super::reserve(inputs, addresses.clone(), options.listeners)
                            .map_err(Error::Transport)?,
                    )
                } else {
                    None
                };
                let mut render_options = resolved.options(options.output);
                if let Some(reserved) = &reserved {
                    render_options.preview_urls = Some(reserved.urls().clone());
                }
                let rendered = build::render_publications_with_generations(
                    inputs,
                    render_options,
                    render,
                    resolved.social_card_generation(),
                )
                .map_err(Error::Publication)?;
                for (site, result) in rendered.sites() {
                    if let Err(error) = result {
                        return Err(Error::Site {
                            site: site.clone(),
                            code: error.code(),
                            message: error.to_string(),
                        });
                    }
                }
                let mut session = if let Some(reserved) = reserved {
                    super::attach(reserved, &rendered)
                } else {
                    super::bind(&rendered, addresses, options.listeners)
                }
                .map_err(Error::Transport)?;
                let mut urls = session.urls();
                if !urls.keys().eq(session.addresses().keys()) {
                    return Err(Error::Request(
                        "bound entry URLs do not match the exact site set",
                    ));
                }
                let sites = session
                    .addresses()
                    .iter()
                    .map(|(key, address)| {
                        let rendered = rendered
                            .sites()
                            .get(key)
                            .and_then(|site| site.as_ref().ok())
                            .ok_or(Error::Request("bound site lacks complete output"))?;
                        Ok((
                            key.clone(),
                            Site {
                                address: *address,
                                url: urls
                                    .remove(key)
                                    .ok_or(Error::Request("bound site lacks entry URL"))?,
                                output_identity: rendered.site().output().inventory().identity(),
                            },
                        ))
                    })
                    .collect::<Result<_, Error>>()?;
                observe(Event::Ready {
                    state: State {
                        status: status(options.local_urls),
                        input_mode: InputMode::Pinned,
                        generation: 1,
                        stale: false,
                        input_identity: identity,
                        build_selection: inputs.build_selection().clone(),
                        sites,
                        documentation: receipts.to_vec(),
                    },
                })
                .map_err(Error::Observer)?;
                while control().map_err(Error::Observer)? != watch::Control::Stop {
                    match session.poll().map_err(Error::Transport)? {
                        super::Poll::Idle => thread::sleep(Duration::from_millis(10)),
                        super::Poll::Responded | super::Poll::Disconnected => (),
                    }
                }
                session.close();
                observe(Event::Closed).map_err(Error::Observer)
            }
            build::DocumentationPublicationExecution::Refused { receipts, error } => {
                if receipts.is_empty() && recipe.documentation_generation().is_none() {
                    if let build::DocumentationPublicationFailure::Publication(error) = error {
                        return Err(Error::Publication(error));
                    }
                    return Err(Error::Request("documentation acquisition refused"));
                }
                deliver(build::DocumentationDelivery::Refused { receipts, error })
                    .map_err(Error::Observer)?;
                Err(Error::Request(
                    "documentation operation refused; original receipt delivered",
                ))
            }
            build::DocumentationPublicationExecution::Native {
                site,
                receipts,
                execution,
            } => {
                deliver(build::DocumentationDelivery::Native {
                    site,
                    receipts,
                    execution,
                })
                .map_err(Error::Observer)?;
                Err(Error::Request(
                    "documentation operation failed; original native custody delivered",
                ))
            }
        },
    )
}

fn status(local: bool) -> Status {
    if local {
        Status::Local
    } else {
        Status::ProductionLinked
    }
}

fn watch_event(event: watch::Event<'_>, local: bool) -> io::Result<Option<Event>> {
    let (kind, generation, current, addresses, urls) = match event {
        watch::Event::Poll => return Ok(None),
        watch::Event::Closed => return Ok(Some(Event::Closed)),
        watch::Event::RebuildFailed { generation, error } => {
            return Ok(Some(Event::RebuildFailed {
                generation,
                stale: true,
                code: error.code(),
                documentation: Vec::new(),
            }));
        }
        watch::Event::Ready {
            generation,
            current,
            addresses,
            local_urls,
        } => (0, generation, current, addresses, local_urls),
        watch::Event::Rebuilt {
            generation,
            current,
            addresses,
            local_urls,
        } => (1, generation, current, addresses, local_urls),
        watch::Event::Recovered {
            generation,
            current,
            addresses,
            local_urls,
        } => (2, generation, current, addresses, local_urls),
    };
    if !current.sites().keys().eq(addresses.keys()) || !urls.keys().eq(addresses.keys()) {
        return Err(io::Error::other("watch observation lacks exact site set"));
    }
    let state = State {
        status: status(local),
        input_mode: InputMode::Development,
        generation,
        stale: false,
        input_identity: current.input_identity().clone(),
        build_selection: current.inputs().build_selection().clone(),
        documentation: Vec::new(),
        sites: current
            .sites()
            .iter()
            .map(|(key, site)| {
                (
                    key.clone(),
                    Site {
                        address: addresses[key],
                        url: urls[key].as_str().to_owned(),
                        output_identity: site.output().inventory().identity(),
                    },
                )
            })
            .collect(),
    };
    Ok(Some(match kind {
        0 => Event::Ready { state },
        1 => Event::Changed { state },
        _ => Event::Recovered { state },
    }))
}
