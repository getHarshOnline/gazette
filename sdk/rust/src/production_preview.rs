//! Production-linked, no-push preview over one explicit fresh render.

use std::{collections::BTreeMap, io, net::SocketAddr, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    build::{self, LocalPublicationRecipe},
    effect,
    preview::{self, runner, session, watch},
    source, toml,
    workspace::Key,
};

/// Registry behavior selected by this bounded first slice.
///
/// Reconciliation remains unavailable until it can consume the same retained
/// semantic model as rendering. The operation never starts a second compilation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RegistryMode {
    /// Do not read or mutate the private registry.
    Unconfigured,
}

impl FromStr for RegistryMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unconfigured" => Ok(Self::Unconfigured),
            _ => Err("production preview registry must be unconfigured"),
        }
    }
}

/// Artifact input selected before acquisition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactMode {
    /// Render once in memory from the exact pinned publication request.
    FreshRender,
}

impl FromStr for ArtifactMode {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "fresh-render" => Ok(Self::FreshRender),
            _ => Err("production preview artifact must be fresh-render"),
        }
    }
}

/// Exact production-linked request consumed by both planning and execution.
pub struct Request {
    publication: LocalPublicationRecipe,
    registry: RegistryMode,
    artifact: ArtifactMode,
    listeners: BTreeMap<Key, SocketAddr>,
}

impl Request {
    /// Bind one already decoded publication recipe to explicit no-push choices.
    #[must_use]
    pub fn new(
        publication: LocalPublicationRecipe,
        registry: RegistryMode,
        artifact: ArtifactMode,
        listeners: BTreeMap<Key, SocketAddr>,
    ) -> Self {
        Self {
            publication,
            registry,
            artifact,
            listeners,
        }
    }
}

/// Native bounds and permissions for the fresh-render operation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Explicit listener permission and HTTP limits.
    pub listeners: preview::Options,
    /// Workspace syntax ceilings.
    pub syntax: toml::Limits,
    /// Complete selected-source acquisition ceilings.
    pub acquisition: build::PublicationSetAcquisitionLimits,
    /// Per-site generated-output ceilings.
    pub output: source::Limits,
    /// Aggregate complete-set rendering ceilings.
    pub render: build::LocalRenderLimits,
}

impl Options {
    fn native(self) -> runner::Options {
        runner::Options {
            mode: runner::Mode::Pinned,
            local_urls: false,
            listeners: self.listeners,
            syntax: self.syntax,
            acquisition: self.acquisition,
            output: self.output,
            render: self.render,
        }
    }
}

/// Fixed operation profile, distinct from ordinary preview and release preparation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Configured public links, loopback serving, and no release effect.
    ProductionLinkedNoPushNoncertified,
}

/// Exact pre-execution choices and portable effect disclosure.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Plan {
    /// Fixed production-linked, no-push profile.
    pub status: Status,
    /// Registry behavior selected by the request.
    pub registry: RegistryMode,
    /// Artifact behavior selected by the request.
    pub artifact: ArtifactMode,
    /// Explicit invocation order, absent until configured workspace defaults are resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sites: Option<Vec<Key>>,
    /// Complete requested loopback bindings before port allocation.
    pub requested_listeners: BTreeMap<Key, SocketAddr>,
    /// Complete portable effect classes for execution.
    pub effects: effect::Plan,
}

/// Resolve the same exact request consumed by execution without native effects.
///
/// # Errors
/// Refuses incomplete listener selection, non-loopback bindings, missing listener
/// permission, invalid bounds, or another preview preflight failure.
pub fn plan(request: &Request, options: Options) -> Result<Plan, runner::Error> {
    let planned = runner::plan(&request.publication, &request.listeners, &options.native())?;
    Ok(Plan {
        status: Status::ProductionLinkedNoPushNoncertified,
        registry: request.registry,
        artifact: request.artifact,
        sites: planned.sites,
        requested_listeners: planned.requested_listeners,
        effects: planned.effects,
    })
}

/// Run one synchronous fresh-render lifecycle from the already planned request.
///
/// # Errors
/// Preserves acquisition, rendering, listener and observer failures without
/// adding registry, output-tree, Git, release or outbound-network effects.
pub fn run<O, C>(
    request: &Request,
    options: Options,
    observe: O,
    control: C,
) -> Result<(), runner::Error>
where
    O: FnMut(runner::Event) -> io::Result<()>,
    C: FnMut() -> io::Result<watch::Control>,
{
    plan(request, options)?;
    runner::run(
        &request.publication,
        request.listeners.clone(),
        options.native(),
        observe,
        control,
    )
}

/// Start one held fresh-render lifecycle using the existing preview-session owner.
///
/// # Errors
/// Returns only pre-effect validation or worker-spawn failures. Native startup
/// outcomes remain observable through the returned session.
pub fn start(
    request: Request,
    options: Options,
    history: session::Limits,
) -> Result<session::Session, session::Error> {
    plan(&request, options).map_err(session::Error::Runner)?;
    session::Session::start(
        request.publication,
        request.listeners,
        options.native(),
        history,
    )
}
