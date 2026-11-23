//! Fixed watch requests, bounded options and honest runtime observations.

use super::Generation;
use crate::build::{
    PublicationSetAcquisitionLimits, PublicationSetError, PublicationSetRenderLimits,
    PublicationSetRenderOptions,
};
use gazette_compiler::{toml, workspace::Key};
use gazette_core::{preview, publication::routing::PublicBaseUrl};
use std::{collections::BTreeMap, fmt, io, net::SocketAddr, time::Duration};

/// Fixed workspace bytes, site recipes and listener choices for one watch run.
/// Changed files never select a new repository, commit, Theme or workspace.
pub struct Request<'a> {
    /// Complete caller-selected workspace compilation and exact configuration evidence.
    pub workspace: &'a crate::configuration::ResolvedWorkspace,
    /// Exact resolved selection owner, order and draft policy; fixed across generations.
    pub build_selection: gazette_compiler::workspace::BuildSelection,
    /// Complete site set and compiler recipes; caller-supplied preview URLs are refused.
    pub render: PublicationSetRenderOptions<'a>,
    /// Exact selected loopback bindings. Zero ports are resolved once, not per rebuild.
    pub addresses: BTreeMap<Key, SocketAddr>,
}

/// Explicit effect and resource bounds; polling does not install a filesystem watcher.
#[derive(Clone, Copy, Debug)]
pub struct Options {
    /// Use held socket URLs instead of configured public links in every generation.
    pub local_urls: bool,
    /// Delay after each inspection/build, between 100 milliseconds and one hour.
    pub interval: Duration,
    /// Listener permission and per-request limits, fixed at startup.
    pub listeners: preview::Options,
    /// Complete configuration parser ceilings.
    pub syntax: toml::Limits,
    /// One budget for fresh captures of the complete selected input universe.
    pub acquisition: PublicationSetAcquisitionLimits,
    /// Complete shared compilation and output ceilings.
    pub render: PublicationSetRenderLimits,
    /// Explicit native typesetting effects for new captured generations only.
    /// None performs no acquisition, even when the workspace selects a command.
    pub math_generation: Option<crate::build::MathGenerationOptions>,
    /// Explicit native diagram capture for new input generations; omission never executes.
    pub diagram_generation: Option<crate::build::DiagramGenerationOptions>,
    /// Explicit card capture after selecting each generation's final URL model.
    pub social_card_generation: Option<crate::build::SocialCardGenerationOptions>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            local_urls: true,
            interval: Duration::from_millis(500),
            listeners: Default::default(),
            syntax: Default::default(),
            acquisition: Default::default(),
            render: Default::default(),
            math_generation: None,
            diagram_generation: None,
            social_card_generation: None,
        }
    }
}

impl Options {
    /// Validate explicit effects before configuration or source reads.
    ///
    /// # Errors
    /// Refuses listener permission/bounds or an unbounded/overly frequent poll interval.
    pub fn validate(&self) -> Result<(), Error> {
        self.listeners.validate().map_err(Error::Transport)?;
        if !(Duration::from_millis(100)..=Duration::from_secs(3600)).contains(&self.interval) {
            return Err(Error::Options(
                "watch interval must be between 100 ms and one hour",
            ));
        }
        Ok(())
    }
}

/// Observer control applies between bounded native operations, never cancels an
/// already reported effect retroactively. Process termination also releases sockets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    /// Continue polling within the fixed effect and resource bounds.
    Continue,
    /// End the run and release its listeners before the closed observation.
    Stop,
}

pub(super) enum Notification {
    Ready,
    Rebuilt,
    Recovered,
}

/// Runtime observations, not release or certification receipts. Failure explicitly
/// makes the last successful generation stale until a successful fresh acquisition.
pub enum Event<'a> {
    /// Control checkpoint; hosts should not emit a progress message for each poll.
    Poll,
    /// The initial complete generation is attached to actual held listeners.
    Ready {
        /// One-based coordinate local to this watch run, not artifact identity.
        generation: u64,
        /// Complete immutable output and exact input evidence.
        current: &'a Generation,
        /// Actual continuously held sockets for the entire selected site set.
        addresses: &'a BTreeMap<Key, SocketAddr>,
        /// Local entry addresses, including when output keeps public links.
        local_urls: &'a BTreeMap<Key, PublicBaseUrl>,
    },
    /// All sites switch together after a complete successful new render.
    Rebuilt {
        /// Advanced only after installing one complete new generation.
        generation: u64,
        /// New complete immutable output and its exact input evidence.
        current: &'a Generation,
        /// Same sockets as startup; rebuild never probes or rebinds.
        addresses: &'a BTreeMap<Key, SocketAddr>,
        /// Same local entry addresses as startup.
        local_urls: &'a BTreeMap<Key, PublicBaseUrl>,
    },
    /// Fresh inspection matches the retained generation after an earlier failure.
    Recovered {
        /// Unchanged coordinate of the again-current retained generation.
        generation: u64,
        /// Retained output whose inputs again match fresh native acquisition.
        current: &'a Generation,
        /// Same sockets continuously held across the failed attempt.
        addresses: &'a BTreeMap<Key, SocketAddr>,
        /// Same local entry addresses; recovery does not change routing.
        local_urls: &'a BTreeMap<Key, PublicBaseUrl>,
    },
    /// No replacement occurred; the named generation is retained but stale.
    RebuildFailed {
        /// Last successful generation, now explicitly stale.
        generation: u64,
        /// Failure of the fresh attempt, not proof that older bytes disappeared.
        error: &'a BuildError,
    },
    /// Every owned listener has been released.
    Closed,
}

/// A fresh acquisition or complete compilation failed without replacing served bytes.
#[derive(Debug)]
pub enum BuildError {
    /// Native acquisition, parsing or shared compilation failure.
    Publication(PublicationSetError),
    /// Exact failed sites; successful siblings are not installed separately.
    Incomplete(BTreeMap<Key, String>),
    /// Invalid output-to-preview binding.
    Preview(preview::Error),
}

impl BuildError {
    /// Stable error class, suitable for a bounded private observer.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Publication(error) => error.code(),
            Self::Incomplete(_) => "preview_incomplete_generation",
            Self::Preview(error) => error.code(),
        }
    }
}
impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Publication(error) => error.fmt(f),
            Self::Incomplete(sites) => write!(
                f,
                "preview generation failed for {sites:?}; no site was replaced"
            ),
            Self::Preview(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Publication(error) => Some(error),
            Self::Preview(error) => Some(error),
            Self::Incomplete(_) => None,
        }
    }
}

/// A watch run could not start or continue safely. Rebuild failures are events;
/// listener/control failures terminate the run rather than look like fresh output.
#[derive(Debug)]
pub enum Error {
    /// Invalid fixed startup request.
    Options(&'static str),
    /// Initial generation never became ready.
    Build(BuildError),
    /// Listener binding, attachment or serving failed.
    Transport(preview::Error),
    /// An observer/control receipt could not be delivered.
    Observer(io::Error),
    /// Monotonic in-process generation coordinate cannot advance.
    Exhausted,
}
impl Error {
    /// Stable failure class, without copying private source payloads.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Options(_) => "preview_watch_options",
            Self::Build(error) => error.code(),
            Self::Transport(error) => error.code(),
            Self::Observer(_) => "preview_watch_observer",
            Self::Exhausted => "preview_watch_generation_exhausted",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options(reason) => f.write_str(reason),
            Self::Build(error) => error.fmt(f),
            Self::Transport(error) => error.fmt(f),
            Self::Observer(error) => error.fmt(f),
            Self::Exhausted => {
                f.write_str("preview generation counter exhausted; restart explicitly")
            }
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Build(error) => Some(error),
            Self::Transport(error) => Some(error),
            Self::Observer(error) => Some(error),
            Self::Options(_) | Self::Exhausted => None,
        }
    }
}
