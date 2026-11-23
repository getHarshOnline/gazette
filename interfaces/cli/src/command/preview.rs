//! Shared application custody for synchronous and process-held native preview lifecycles.

use std::{
    collections::BTreeMap,
    io,
    net::SocketAddr,
    sync::{Mutex, TryLockError},
};

use gazette_sdk::{
    build::LocalPublicationRecipe,
    preview::{runner, session, watch},
    production_preview,
    source::Sha256Digest,
    workspace::Key,
};

use super::{Command, Kernel};
use crate::command::publication;

const CAPACITY: usize = 4;

#[derive(Default)]
pub(super) struct Sessions {
    entries: Mutex<BTreeMap<String, Held>>,
}

struct Held {
    native: session::Session,
    digest: Sha256Digest,
}

/// Registry or pre-effect preview failure independent of transport framing.
#[derive(Debug)]
pub enum Failure {
    /// Another handle operation currently owns the registry.
    Busy,
    /// Poisoned custody cannot establish native state or closure.
    Unavailable,
    /// Four retained handles already occupy the fixed registry.
    Capacity,
    /// The opaque handle is unknown or explicitly released.
    Unknown,
    /// A caller-supplied opaque handle collided with retained custody.
    HandleCollision,
    /// Native preview options were refused before worker creation.
    Options(runner::Error),
    /// Shared publication permissions or recipe admission failed.
    Publication(publication::Failure),
}

impl Failure {
    /// Stable redacted failure class.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "preview_session_busy",
            Self::Unavailable => "preview_session_unavailable",
            Self::Capacity => "preview_session_capacity",
            Self::Unknown => "preview_session_unknown",
            Self::HandleCollision => "preview_session_handle_collision",
            Self::Options(error) => error.code(),
            Self::Publication(error) => error.code(),
        }
    }

    /// Stable safe explanation.
    #[must_use]
    pub fn reason(&self) -> &str {
        match self {
            Self::Busy => "Another handle operation is active; no new operation started",
            Self::Unavailable => "Registry failed; do not infer native closure or restart",
            Self::Capacity => {
                "Explicitly stop and release an existing handle; none expires or is evicted"
            }
            Self::Unknown => "Unknown or explicitly released preview handle; no operation started",
            Self::HandleCollision => "Handle collision; no operation started",
            Self::Options(_) => "Invalid native preview options; no operation started",
            Self::Publication(error) => error.message(),
        }
    }
}

/// Owned preview result with custody evidence preserved across protocol delivery.
#[derive(Clone, Debug)]
pub struct Report {
    /// Opaque process-local custody handle.
    pub session: String,
    /// Digest of the original start request.
    pub request_sha256: Sha256Digest,
    /// Whether registry custody remains available after this operation.
    pub retained: bool,
    /// Native observation or explicit native failure.
    pub outcome: Outcome,
}

/// Native preview observation independent of interface encoding.
#[derive(Clone, Debug)]
pub enum Outcome {
    /// Current non-consuming worker state.
    Observed(session::Snapshot),
    /// Bounded retained observations after an exact cursor.
    Polled(session::Observations),
    /// Completed worker joined and custody explicitly released.
    Released(session::Terminal),
    /// Native session operation failed without fabricating state.
    Failed(&'static str),
}

impl Report {
    /// Whether this report represents native failure evidence.
    #[must_use]
    pub fn failed(&self) -> bool {
        match &self.outcome {
            Outcome::Observed(snapshot) => snapshot_failed(snapshot),
            Outcome::Polled(observations) => snapshot_failed(&observations.snapshot),
            Outcome::Released(terminal) => matches!(terminal, session::Terminal::Failed { .. }),
            Outcome::Failed(_) => true,
        }
    }
}

/// Resolve one preview request without starting acquisition, providers or listeners.
pub struct Plan {
    /// Shared publication permission, bounds and exact recipe bytes.
    pub publication: publication::Render,
    /// Exact complete selected-site listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Native preview runner options.
    pub options: runner::Options,
}

impl Command for Plan {
    type Outcome = runner::Plan;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let parsed = self.publication.parse().map_err(Failure::Publication)?;
        runner::plan(&parsed.recipe, &self.addresses, &self.options).map_err(Failure::Options)
    }
}

/// Resolve one production-linked fresh-render preview without native effects.
pub struct ProductionPlan {
    /// Shared publication permission, bounds and exact recipe bytes.
    pub publication: publication::Render,
    /// Explicit registry behavior.
    pub registry: production_preview::RegistryMode,
    /// Explicit artifact behavior.
    pub artifact: production_preview::ArtifactMode,
    /// Exact complete selected-site listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Native production-preview bounds and permissions.
    pub options: production_preview::Options,
}

impl Command for ProductionPlan {
    type Outcome = production_preview::Plan;
    type Failure = Failure;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let parsed = self.publication.parse().map_err(Failure::Publication)?;
        let request = production_preview::Request::new(
            parsed.recipe,
            self.registry,
            self.artifact,
            self.addresses,
        );
        production_preview::plan(&request, self.options).map_err(Failure::Options)
    }
}

/// Start one process-held native preview worker.
pub struct Start {
    /// Fixed-width opaque identifier created before native work.
    pub handle: String,
    /// Original exact protocol request digest.
    pub request_sha256: Sha256Digest,
    /// Shared publication permission, bounds and exact recipe bytes.
    pub publication: publication::Render,
    /// Exact complete selected-site listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Native preview runner options.
    pub options: runner::Options,
    /// Bounded replayable observation history.
    pub history: session::Limits,
}

impl Command for Start {
    type Outcome = Report;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        self.options.validate().map_err(Failure::Options)?;
        let mut entries = lock(&kernel.preview)?;
        if entries.len() >= CAPACITY {
            return Err(Failure::Capacity);
        }
        if entries.contains_key(&self.handle) {
            return Err(Failure::HandleCollision);
        }
        let recipe = self
            .publication
            .parse()
            .map_err(Failure::Publication)?
            .recipe;
        let native =
            match session::Session::start(recipe, self.addresses, self.options, self.history) {
                Ok(native) => native,
                Err(error) => {
                    return Ok(Report {
                        session: self.handle,
                        request_sha256: self.request_sha256,
                        retained: false,
                        outcome: Outcome::Failed(error.code()),
                    });
                }
            };
        entries.insert(
            self.handle.clone(),
            Held {
                native,
                digest: self.request_sha256,
            },
        );
        observe(
            &self.handle,
            entries.get(&self.handle).expect("installed custody"),
        )
    }
}

/// Start one held production-linked fresh-render preview worker.
pub struct ProductionStart {
    /// Fixed-width opaque identifier created before native work.
    pub handle: String,
    /// Original exact protocol request digest.
    pub request_sha256: Sha256Digest,
    /// Shared publication permission, bounds and exact recipe bytes.
    pub publication: publication::Render,
    /// Explicit registry behavior.
    pub registry: production_preview::RegistryMode,
    /// Explicit artifact behavior.
    pub artifact: production_preview::ArtifactMode,
    /// Exact complete selected-site listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Native production-preview bounds and permissions.
    pub options: production_preview::Options,
    /// Bounded replayable observation history.
    pub history: session::Limits,
}

impl Command for ProductionStart {
    type Outcome = Report;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.preview)?;
        if entries.len() >= CAPACITY {
            return Err(Failure::Capacity);
        }
        if entries.contains_key(&self.handle) {
            return Err(Failure::HandleCollision);
        }
        let recipe = self
            .publication
            .parse()
            .map_err(Failure::Publication)?
            .recipe;
        let request =
            production_preview::Request::new(recipe, self.registry, self.artifact, self.addresses);
        let native = match production_preview::start(request, self.options, self.history) {
            Ok(native) => native,
            Err(error) => {
                return Ok(Report {
                    session: self.handle,
                    request_sha256: self.request_sha256,
                    retained: false,
                    outcome: Outcome::Failed(error.code()),
                });
            }
        };
        entries.insert(
            self.handle.clone(),
            Held {
                native,
                digest: self.request_sha256,
            },
        );
        observe(
            &self.handle,
            entries.get(&self.handle).expect("installed custody"),
        )
    }
}

/// Inspect one process-held worker without driving native work.
pub struct Inspect {
    /// Opaque process-local custody handle.
    pub handle: String,
}

impl Command for Inspect {
    type Outcome = Report;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let entries = lock(&kernel.preview)?;
        let held = entries.get(&self.handle).ok_or(Failure::Unknown)?;
        observe(&self.handle, held)
    }
}

/// Poll replayable retained observations after an exact cursor.
pub struct Poll {
    /// Opaque process-local custody handle.
    pub handle: String,
    /// Last observation already accepted by the caller.
    pub after: u64,
    /// Maximum observations to return.
    pub maximum: usize,
}

impl Command for Poll {
    type Outcome = Report;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let entries = lock(&kernel.preview)?;
        let held = entries.get(&self.handle).ok_or(Failure::Unknown)?;
        let outcome = match held.native.poll(self.after, self.maximum) {
            Ok(observations) => Outcome::Polled(observations),
            Err(error) => Outcome::Failed(error.code()),
        };
        Ok(Report {
            session: self.handle,
            request_sha256: held.digest,
            retained: true,
            outcome,
        })
    }
}

/// Request cooperative stop and optionally release actually finished custody.
pub struct Stop {
    /// Opaque process-local custody handle.
    pub handle: String,
    /// Remove custody only after the native worker is known finished.
    pub release_handle: bool,
}

impl Command for Stop {
    type Outcome = Report;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.preview)?;
        let held = entries.get_mut(&self.handle).ok_or(Failure::Unknown)?;
        let digest = held.digest;
        let snapshot = held.native.request_stop();
        if self.release_handle
            && let Some(terminal) = held.native.try_finish()
        {
            entries.remove(&self.handle);
            return Ok(Report {
                session: self.handle,
                request_sha256: digest,
                retained: false,
                outcome: Outcome::Released(terminal),
            });
        }
        Ok(Report {
            session: self.handle,
            request_sha256: digest,
            retained: true,
            outcome: match snapshot {
                Ok(snapshot) => Outcome::Observed(snapshot),
                Err(error) => Outcome::Failed(error.code()),
            },
        })
    }
}

/// Synchronous CLI lifecycle over the same native runner used by held sessions.
pub struct Run<O, C> {
    /// Shared publication recipe.
    pub recipe: LocalPublicationRecipe,
    /// Exact complete selected-site listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Native preview runner options.
    pub options: runner::Options,
    /// CLI-owned observation projection.
    pub observe: O,
    /// CLI-owned process control source.
    pub control: C,
}

impl<O, C> Command for Run<O, C>
where
    O: FnMut(runner::Event) -> io::Result<()>,
    C: FnMut() -> io::Result<watch::Control>,
{
    type Outcome = ();
    type Failure = runner::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        runner::run(
            &self.recipe,
            self.addresses,
            self.options,
            self.observe,
            self.control,
        )
    }
}

/// Synchronous preview with an immediate consumer for original documentation custody.
pub struct RunWithDocumentation<O, C, D> {
    /// Shared publication selection and explicit generation request.
    pub recipe: LocalPublicationRecipe,
    /// Exact selected listener bindings.
    pub addresses: BTreeMap<Key, SocketAddr>,
    /// Existing native preview limits and permissions.
    pub options: runner::Options,
    /// Ordinary owned serving observations.
    pub observe: O,
    /// Process control source.
    pub control: C,
    /// Immediate original documentation delivery, including exceptional borrowed Contract.
    pub documentation: D,
}
impl<O, C, D> Command for RunWithDocumentation<O, C, D>
where
    O: FnMut(runner::Event) -> io::Result<()>,
    C: FnMut() -> io::Result<watch::Control>,
    D: FnMut(gazette_sdk::build::DocumentationDelivery<'_>) -> io::Result<()>,
{
    type Outcome = ();
    type Failure = runner::Error;
    fn execute(self, _kernel: &Kernel) -> Result<(), Self::Failure> {
        runner::run_with_documentation_delivery(
            &self.recipe,
            self.addresses,
            self.options,
            self.observe,
            self.control,
            self.documentation,
        )
    }
}

/// Synchronous production-linked fresh-render lifecycle.
pub struct ProductionRun<O, C> {
    /// Exact decoded request shared with planning.
    pub request: production_preview::Request,
    /// Native bounds and permissions.
    pub options: production_preview::Options,
    /// CLI-owned observation projection.
    pub observe: O,
    /// CLI-owned process control source.
    pub control: C,
}

impl<O, C> Command for ProductionRun<O, C>
where
    O: FnMut(runner::Event) -> io::Result<()>,
    C: FnMut() -> io::Result<watch::Control>,
{
    type Outcome = ();
    type Failure = runner::Error;

    fn execute(self, _kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        production_preview::run(&self.request, self.options, self.observe, self.control)
    }
}

fn lock(sessions: &Sessions) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, Held>>, Failure> {
    match sessions.entries.try_lock() {
        Ok(entries) => Ok(entries),
        Err(TryLockError::WouldBlock) => Err(Failure::Busy),
        Err(TryLockError::Poisoned(_)) => Err(Failure::Unavailable),
    }
}

fn observe(handle: &str, held: &Held) -> Result<Report, Failure> {
    Ok(Report {
        session: handle.to_owned(),
        request_sha256: held.digest,
        retained: true,
        outcome: match held.native.inspect() {
            Ok(snapshot) => Outcome::Observed(snapshot),
            Err(error) => Outcome::Failed(error.code()),
        },
    })
}

fn snapshot_failed(snapshot: &session::Snapshot) -> bool {
    matches!(snapshot.terminal, Some(session::Terminal::Failed { .. }))
}
