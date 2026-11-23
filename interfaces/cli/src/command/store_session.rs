//! Process-owned ordinary store lifetimes shared by protocol projections.

use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Mutex, TryLockError},
};

use gazette_sdk::store;

use super::{Command, Kernel};

mod materialization;
pub use materialization::{MaterializationFailure, MaterializationReport, Materialize};
mod documentation_materialization;
pub use documentation_materialization::{
    DocumentationContractError, DocumentationMaterializationFailure,
    DocumentationMaterializationOutcome, DocumentationMaterializationReport,
    MaterializeDocumentation,
};

const CAPACITY: usize = 4;

#[derive(Default)]
pub(super) struct Sessions {
    entries: Mutex<BTreeMap<String, Held>>,
}

/// One ordinary native lifetime selection.
#[derive(Clone, Debug)]
pub enum Selection {
    /// Existing read-only database session.
    Connection {
        /// Existing database; never created, migrated or adopted.
        path: PathBuf,
        /// Existing private namespace held by cooperating local callers.
        coordination_root: PathBuf,
        /// Required explicit acknowledgement of native reads.
        allow_native_reads: bool,
        /// Native lifetime connection budget and permissions.
        options: store::SessionOptions,
    },
    /// Existing private journal writer generation.
    Writer {
        /// Existing private initialized journal directory.
        root: PathBuf,
        /// Independently retained complete coordinates.
        expected_header: store::JournalHeader,
        /// Required explicit acknowledgement of native reads.
        allow_native_reads: bool,
        /// Native generation, reservation, coordination permissions and limits.
        options: store::WriterOptions,
    },
}

impl Selection {
    fn validate(&self) -> bool {
        match self {
            Self::Connection {
                path,
                coordination_root,
                allow_native_reads,
                options,
            } => {
                *allow_native_reads
                    && path.is_absolute()
                    && coordination_root.is_absolute()
                    && options.validate().is_ok()
            }
            Self::Writer {
                root,
                allow_native_reads,
                options,
                ..
            } => *allow_native_reads && root.is_absolute() && options.validate().is_ok(),
        }
    }
}

enum Native {
    Connection(Box<store::Session>),
    Writer(Box<store::Writer>),
}

/// One fresh native observation from ordinary custody.
#[derive(Debug)]
pub enum Inspection {
    /// Existing-store metadata observation.
    Connection(store::Observation),
    /// Exact held journal inspection.
    Writer(store::JournalInspection),
}

impl Inspection {
    fn header(&self) -> Option<store::JournalHeader> {
        match self {
            Self::Connection(_) => None,
            Self::Writer(inspection) => Some(inspection.header),
        }
    }
}

impl Native {
    fn inspect(&mut self) -> Result<Inspection, store::Error> {
        match self {
            Self::Connection(session) => session.observe().map(Inspection::Connection),
            Self::Writer(writer) => writer.inspect().map(Inspection::Writer),
        }
    }

    fn close(self) -> Result<(), store::Error> {
        match self {
            Self::Connection(session) => (*session).close(),
            Self::Writer(writer) => (*writer).close(),
        }
    }
}

struct Held {
    native: Native,
    selection: Selection,
    acquired_header: Option<store::JournalHeader>,
}

/// Native ordinary-store failure phase.
#[derive(Clone, Copy, Debug)]
pub enum NativeFailurePhase {
    /// Native lifetime acquisition.
    Acquisition,
    /// Fresh inspection immediately after acquisition.
    Readiness,
    /// Later fresh inspection.
    Inspection,
    /// Explicit native close.
    Close,
}

/// Effect-aware native failure evidence.
#[derive(Debug)]
pub struct NativeFailure {
    /// Native phase that failed.
    pub phase: NativeFailurePhase,
    /// Existing stable SDK error code.
    pub code: &'static str,
    /// Writer-generation effect retained only for failed acquisition.
    pub generation: Option<store::GenerationEffect>,
    /// Separate close failure after readiness failed.
    pub close_error: Option<&'static str>,
}

/// Registry, selection or reservation failure before native work starts.
#[derive(Debug)]
pub enum Failure<E> {
    /// Another operation currently owns this registry.
    Busy,
    /// Poisoned custody cannot establish native state or closure.
    Unavailable,
    /// Four retained handles already occupy the fixed registry.
    Capacity,
    /// The selected handle is unknown or explicitly closed.
    Unknown,
    /// A caller-supplied handle collided with retained custody.
    HandleCollision,
    /// The ordinary selection was invalid before native work.
    Selection,
    /// A configured-output operation requires an already-held journal writer.
    NotWriter,
    /// The transport could not reserve its complete custody response.
    Reservation(E),
}

/// Kernel-owned ordinary-store operation result.
#[derive(Debug)]
pub struct Report {
    /// Opaque process-local handle.
    pub session: String,
    /// Whether the kernel retains an actual native object.
    pub active: bool,
    /// Original successfully acquired writer header, if any.
    pub acquired_header: Option<store::JournalHeader>,
    /// Fresh native result or effect-aware failure.
    pub outcome: Outcome,
}

/// Native ordinary-store result independent of transport encoding.
#[derive(Debug)]
pub enum Outcome {
    /// A connection was acquired and freshly observed.
    Connected {
        /// Fresh native metadata observation.
        observation: store::Observation,
    },
    /// A writer was acquired and freshly inspected.
    Acquired {
        /// Fresh native journal inspection.
        inspection: store::JournalInspection,
    },
    /// An existing connection was freshly observed.
    Observed {
        /// Fresh native metadata observation.
        observation: store::Observation,
    },
    /// An existing writer was freshly inspected.
    Inspected {
        /// Fresh native journal inspection.
        inspection: store::JournalInspection,
    },
    /// Explicit native close succeeded and custody was removed.
    Closed,
    /// Native work failed with exact effect evidence.
    Failed {
        /// Existing native failure and effect status.
        failure: NativeFailure,
        /// Native acquisition succeeded before this report.
        acquired: bool,
    },
}

/// Acquire one ordinary native lifetime after response reservation.
pub struct Open<R> {
    /// Opaque handle selected before native work.
    pub handle: String,
    /// Exact ordinary native selection.
    pub selection: Selection,
    /// Pre-effect response reservation under the registry lock.
    pub reserve: R,
}

impl<R, E> Command for Open<R>
where
    R: FnOnce(&str, &Selection) -> Result<(), E>,
{
    type Outcome = Report;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        if !self.selection.validate() {
            return Err(Failure::Selection);
        }
        let mut entries = lock(&kernel.store_session)?;
        if entries.len() >= CAPACITY {
            return Err(Failure::Capacity);
        }
        if entries.contains_key(&self.handle) {
            return Err(Failure::HandleCollision);
        }
        (self.reserve)(&self.handle, &self.selection).map_err(Failure::Reservation)?;
        let acquired = match &self.selection {
            Selection::Connection {
                path,
                coordination_root,
                options,
                ..
            } => store::connect(path, coordination_root, *options)
                .map(Box::new)
                .map(Native::Connection)
                .map_err(|error| NativeFailure {
                    phase: NativeFailurePhase::Acquisition,
                    code: error.code(),
                    generation: None,
                    close_error: None,
                }),
            Selection::Writer {
                root,
                expected_header,
                options,
                ..
            } => store::acquire_writer(root, *expected_header, *options)
                .map(Box::new)
                .map(Native::Writer)
                .map_err(|error| NativeFailure {
                    phase: NativeFailurePhase::Acquisition,
                    code: error.cause.code(),
                    generation: Some(error.generation),
                    close_error: None,
                }),
        };
        let mut native = match acquired {
            Ok(native) => native,
            Err(failure) => {
                return Ok(Report {
                    session: self.handle,
                    active: false,
                    acquired_header: None,
                    outcome: Outcome::Failed {
                        failure,
                        acquired: false,
                    },
                });
            }
        };
        let inspection = match native.inspect() {
            Ok(inspection) => inspection,
            Err(error) => {
                let close_error = native.close().err().map(|error| error.code());
                return Ok(Report {
                    session: self.handle,
                    active: false,
                    acquired_header: None,
                    outcome: Outcome::Failed {
                        failure: NativeFailure {
                            phase: NativeFailurePhase::Readiness,
                            code: error.code(),
                            generation: None,
                            close_error,
                        },
                        acquired: true,
                    },
                });
            }
        };
        let acquired_header = inspection.header();
        let outcome = match inspection {
            Inspection::Connection(observation) => Outcome::Connected { observation },
            Inspection::Writer(inspection) => Outcome::Acquired { inspection },
        };
        entries.insert(
            self.handle.clone(),
            Held {
                native,
                selection: self.selection,
                acquired_header,
            },
        );
        Ok(Report {
            session: self.handle,
            active: true,
            acquired_header,
            outcome,
        })
    }
}

/// Inspect or close one already-held ordinary native lifetime.
pub struct Operate<R> {
    /// Existing opaque process-local handle.
    pub handle: String,
    /// Whether to consume custody through explicit native close.
    pub close: bool,
    /// Pre-effect response reservation over retained original coordinates.
    pub reserve: R,
}

impl<R, E> Command for Operate<R>
where
    R: FnOnce(&str, &Selection, Option<store::JournalHeader>, bool) -> Result<(), E>,
{
    type Outcome = Report;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.store_session)?;
        let held = entries.get_mut(&self.handle).ok_or(Failure::Unknown)?;
        (self.reserve)(
            &self.handle,
            &held.selection,
            held.acquired_header,
            self.close,
        )
        .map_err(Failure::Reservation)?;
        if self.close {
            let held = entries.remove(&self.handle).ok_or(Failure::Unknown)?;
            return Ok(match held.native.close() {
                Ok(()) => Report {
                    session: self.handle,
                    active: false,
                    acquired_header: held.acquired_header,
                    outcome: Outcome::Closed,
                },
                Err(error) => Report {
                    session: self.handle,
                    active: false,
                    acquired_header: held.acquired_header,
                    outcome: Outcome::Failed {
                        failure: NativeFailure {
                            phase: NativeFailurePhase::Close,
                            code: error.code(),
                            generation: None,
                            close_error: None,
                        },
                        acquired: true,
                    },
                },
            });
        }
        Ok(match held.native.inspect() {
            Ok(Inspection::Connection(observation)) => Report {
                session: self.handle,
                active: true,
                acquired_header: held.acquired_header,
                outcome: Outcome::Observed { observation },
            },
            Ok(Inspection::Writer(inspection)) => Report {
                session: self.handle,
                active: true,
                acquired_header: held.acquired_header,
                outcome: Outcome::Inspected { inspection },
            },
            Err(error) => Report {
                session: self.handle,
                active: true,
                acquired_header: held.acquired_header,
                outcome: Outcome::Failed {
                    failure: NativeFailure {
                        phase: NativeFailurePhase::Inspection,
                        code: error.code(),
                        generation: None,
                        close_error: None,
                    },
                    acquired: true,
                },
            },
        })
    }
}

fn lock<E>(
    sessions: &Sessions,
) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, Held>>, Failure<E>> {
    match sessions.entries.try_lock() {
        Ok(entries) => Ok(entries),
        Err(TryLockError::WouldBlock) => Err(Failure::Busy),
        Err(TryLockError::Poisoned(_)) => Err(Failure::Unavailable),
    }
}
