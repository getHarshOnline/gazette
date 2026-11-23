//! Process-owned custody over the SDK's editor-safe selected-buffer semantics.

use std::{
    collections::BTreeMap,
    sync::{Mutex, TryLockError},
    time::{Duration, Instant},
};

use gazette_sdk::authoring::session;

use super::{Command, Kernel};

mod editor_context;
mod selection;
pub use editor_context::{EditorContext, EditorContextInput, ImportEditorContext, MAX_FRAME_BYTES};
pub use selection::Selection;

const CAPACITY: usize = 4;
const LIFETIME: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(super) struct Sessions {
    entries: Mutex<BTreeMap<String, Held>>,
}

struct Held {
    session: session::Session,
    expires: Instant,
}

/// Stable session custody or native authoring failure before transport projection.
#[derive(Debug)]
pub enum Failure {
    /// Another operation currently owns the process-local registry.
    Busy,
    /// A poisoned registry cannot provide trustworthy retained state.
    Unavailable,
    /// Four live sessions already occupy the fixed registry.
    Capacity,
    /// The opaque handle is unknown, closed, or expired.
    Unknown,
    /// The supplied generation no longer names current selected buffers.
    StaleGeneration,
    /// A caller-supplied opaque handle collided with retained custody.
    HandleCollision,
    /// The shared SDK refused the requested selected-buffer operation.
    Native(session::Error),
}

impl Failure {
    /// Stable redacted failure class shared by transports.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Busy => "authoring_busy",
            Self::Unavailable => "authoring_unavailable",
            Self::Capacity => "authoring_capacity",
            Self::Unknown => "authoring_unknown_session",
            Self::StaleGeneration => "authoring_stale_generation",
            Self::HandleCollision => "authoring_handle_collision",
            Self::Native(error) => error.code(),
        }
    }

    /// Stable safe explanation without retained source bytes.
    #[must_use]
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Busy => "Another authoring operation is active; retry.",
            Self::Unavailable => {
                "Authoring state failed; restart the service and reselect exact buffers."
            }
            Self::Capacity => {
                "Close an authoring session or wait for its absolute five-minute expiry."
            }
            Self::Unknown => {
                "Unknown, closed or expired session; reselect explicit baseline inputs."
            }
            Self::StaleGeneration => {
                "Inspect current session state before issuing a new operation; nothing changed."
            }
            Self::HandleCollision => "Authoring handle collision; no session created",
            Self::Native(error) => error.reason(),
        }
    }
}

/// Successful session mutation, independent of interface encoding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Acknowledgement {
    /// Opaque process-local custody handle.
    pub session: String,
    /// Exact current selected-buffer generation.
    pub generation: u64,
    /// Whether the requested operation changed retained state.
    pub changed: bool,
}

/// Start one bounded selected-buffer session under caller-created opaque custody.
pub struct Start {
    /// Fixed-width opaque identifier created before any retained state is admitted.
    pub handle: String,
    /// Exact supplied workspace baseline; never a filesystem locator.
    pub workspace: session::Source,
    /// Exact non-root workspace documents when the selected workspace is modular.
    /// `None` selects the distinct single supplied-buffer authoring mode.
    pub workspace_documents: Option<Vec<session::Source>>,
    /// Exact supplied document baselines; never implicit native reads.
    pub documents: Vec<session::Source>,
    /// Exact authored-manual baselines. They receive shared Markdown editor analysis
    /// without entering publication-content resolution or Theme component binding.
    pub manuals: Vec<session::ManualSource>,
    /// Exact opaque retained inputs used only by native page-package resolution.
    pub page_inputs: Vec<session::PageInputSource>,
    /// Explicitly keyed Theme or plugin package manifest baselines.
    /// Workspace bindings remain the sole source and manifest-path owner.
    pub package_baselines: Vec<session::PackageSource>,
    /// SDK authoring limits shared with LSP.
    pub limits: session::Limits,
}

impl Command for Start {
    type Outcome = Acknowledgement;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.authoring)?;
        expire(&mut entries);
        check_capacity(&entries, &self.handle)?;
        let handle = self.handle.clone();
        let session = self.into_session().map_err(Failure::Native)?;
        Ok(retain(&mut entries, handle, session))
    }
}

impl Start {
    fn into_session(self) -> Result<session::Session, session::Error> {
        match self.workspace_documents {
            None => session::Session::new_with_manuals_page_inputs_and_package_baselines(
                self.workspace,
                self.documents,
                self.manuals,
                self.page_inputs,
                self.package_baselines,
                self.limits,
            ),
            Some(workspace_documents) => {
                session::Session::new_with_workspace_documents_manuals_page_inputs(
                    self.workspace,
                    workspace_documents,
                    self.documents,
                    self.manuals,
                    self.page_inputs,
                    self.package_baselines,
                    self.limits,
                )
            }
        }
    }
}

/// Start the existing supplied-buffer authoring journey from its shared request contract.
///
/// # Errors
/// Refuses invalid supplied sources or unavailable process-local custody.
pub fn start_selection(
    kernel: &Kernel,
    handle: String,
    selection: Selection,
) -> Result<Acknowledgement, Failure> {
    kernel.execute(
        selection
            .into_start(handle, Default::default())
            .map_err(Failure::Native)?,
    )
}

fn check_capacity(entries: &BTreeMap<String, Held>, handle: &str) -> Result<(), Failure> {
    if entries.len() >= CAPACITY {
        return Err(Failure::Capacity);
    }
    if entries.contains_key(handle) {
        return Err(Failure::HandleCollision);
    }
    Ok(())
}

fn retain(
    entries: &mut BTreeMap<String, Held>,
    handle: String,
    session: session::Session,
) -> Acknowledgement {
    let generation = session.generation();
    entries.insert(
        handle.clone(),
        Held {
            session,
            expires: Instant::now() + LIFETIME,
        },
    );
    Acknowledgement {
        session: handle,
        generation,
        changed: true,
    }
}

/// Run one read-only SDK projection while the exact session and generation are held.
pub struct Inspect<F> {
    /// Opaque process-local custody handle.
    pub handle: String,
    /// Exact generation when the operation is generation-bound.
    pub generation: Option<u64>,
    /// Interface-owned projection over the shared SDK session.
    pub inspect: F,
}

/// Distinguishes custody failures from interface-owned projection failures.
#[derive(Debug)]
pub enum InspectionFailure<E> {
    /// Process-local custody or generation failure.
    Session(Failure),
    /// Failure returned by the interface-owned projection.
    Projection(E),
}

impl<F, T, E> Command for Inspect<F>
where
    F: FnOnce(&session::Session) -> Result<T, E>,
{
    type Outcome = T;
    type Failure = InspectionFailure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.authoring).map_err(InspectionFailure::Session)?;
        expire(&mut entries);
        let held = entries
            .get(&self.handle)
            .ok_or(InspectionFailure::Session(Failure::Unknown))?;
        if self
            .generation
            .is_some_and(|expected| expected != held.session.generation())
        {
            return Err(InspectionFailure::Session(Failure::StaleGeneration));
        }
        (self.inspect)(&held.session).map_err(InspectionFailure::Projection)
    }
}

/// One full-text overlay mutation over already selected source identity.
pub enum Mutation {
    /// Open a full-text overlay.
    Open {
        /// Selected document identifier.
        document: String,
        /// Editor version.
        version: i32,
        /// Complete current buffer text.
        text: String,
    },
    /// Replace an already open full-text overlay.
    Change {
        /// Selected document identifier.
        document: String,
        /// Strictly increasing editor version.
        version: i32,
        /// Complete current buffer text.
        text: String,
    },
    /// Restore the retained baseline without rereading disk.
    Close {
        /// Selected document identifier.
        document: String,
    },
}

/// Apply one exact-generation selected-buffer mutation.
pub struct Mutate {
    /// Opaque process-local custody handle.
    pub handle: String,
    /// Exact current generation.
    pub generation: u64,
    /// Requested full-buffer operation.
    pub mutation: Mutation,
}

impl Command for Mutate {
    type Outcome = Acknowledgement;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.authoring)?;
        expire(&mut entries);
        let held = entries.get_mut(&self.handle).ok_or(Failure::Unknown)?;
        if self.generation != held.session.generation() {
            return Err(Failure::StaleGeneration);
        }
        let changed = match self.mutation {
            Mutation::Open {
                document,
                version,
                text,
            } => held.session.open(&document, version, text),
            Mutation::Change {
                document,
                version,
                text,
            } => held.session.change(&document, version, text),
            Mutation::Close { document } => held.session.close(&document),
        }
        .map_err(Failure::Native)?;
        Ok(Acknowledgement {
            session: self.handle,
            generation: held.session.generation(),
            changed,
        })
    }
}

/// Release one exact-generation selected-buffer session.
pub struct End {
    /// Opaque process-local custody handle.
    pub handle: String,
    /// Exact current generation.
    pub generation: u64,
}

impl Command for End {
    type Outcome = Acknowledgement;
    type Failure = Failure;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.authoring)?;
        expire(&mut entries);
        let held = entries.get(&self.handle).ok_or(Failure::Unknown)?;
        if self.generation != held.session.generation() {
            return Err(Failure::StaleGeneration);
        }
        let generation = held.session.generation();
        entries.remove(&self.handle);
        Ok(Acknowledgement {
            session: self.handle,
            generation,
            changed: true,
        })
    }
}

fn lock(sessions: &Sessions) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, Held>>, Failure> {
    match sessions.entries.try_lock() {
        Ok(entries) => Ok(entries),
        Err(TryLockError::WouldBlock) => Err(Failure::Busy),
        Err(TryLockError::Poisoned(_)) => Err(Failure::Unavailable),
    }
}

fn expire(entries: &mut BTreeMap<String, Held>) {
    let now = Instant::now();
    entries.retain(|_, held| held.expires > now);
}
