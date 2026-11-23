//! Bounded background ownership of the shared preview runner, with replayable observations.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt, io,
    net::SocketAddr,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{runner, watch};
use crate::{build::LocalPublicationRecipe, render::LimitedWriter, workspace::Key};

/// Finite event history; the latest state and terminal outcome survive prefix eviction.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Maximum retained event records, from 1 through 1024.
    #[schemars(range(min = 1, max = 1024))]
    pub events: usize,
    /// Maximum serialized retained record bytes, at most 16 MiB.
    #[schemars(range(min = 1, max = 16777216))]
    pub bytes: usize,
    /// Maximum serialized individual record, at most 1 MiB and no more than bytes.
    #[schemars(range(min = 1, max = 1048576))]
    pub event_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            events: 128,
            bytes: 4 << 20,
            event_bytes: 256 << 10,
        }
    }
}

impl Limits {
    /// Check history allocation and per-record bounds before spawning a worker.
    ///
    /// # Errors
    /// Refuses zero, unsupported ceilings or an event larger than the whole history.
    pub fn validate(self) -> Result<(), Error> {
        if !(1..=1024).contains(&self.events)
            || !(1..=16 << 20).contains(&self.bytes)
            || !(1..=1 << 20).contains(&self.event_bytes)
            || self.event_bytes > self.bytes
        {
            return Err(Error::Options(
                "preview history bounds are zero or unsupported",
            ));
        }
        Ok(())
    }
}

/// One immutable observation; a cursor names observation order, not artifact identity.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Record {
    /// One-based monotonic coordinate within this worker.
    pub sequence: u64,
    /// Exact owned runner observation.
    pub event: runner::Event,
}

/// The runner has returned and its listener scope is gone.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Terminal {
    /// Explicit stop completed normally.
    Closed,
    /// Startup, runtime or delivery failed, without automatic retry.
    Failed {
        /// Redacted native failure class; private source and panic bytes are not exposed.
        code: &'static str,
    },
}

/// Latest bounded state, independent of whether callers have polled old events.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Snapshot {
    /// Stop was requested; this alone does not prove listener closure.
    pub stop_requested: bool,
    /// The entire native worker has finished, not merely emitted Closed.
    pub finished: bool,
    /// Oldest retained event coordinate, if any.
    pub first_available: Option<u64>,
    /// Last committed observation coordinate; zero before the first event.
    pub last_sequence: u64,
    /// Last complete generation, with stale status updated by rebuild failures.
    pub latest: Option<runner::State>,
    /// Final runner outcome, retained until explicit handle disposal.
    pub terminal: Option<Terminal>,
}

/// The requested cursor precedes the retained prefix; no history is fabricated.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Gap {
    /// Caller-supplied last-seen sequence.
    pub after: u64,
    /// First event still available.
    pub first_available: u64,
}

/// Non-consuming cursor result. Repeating a poll never acknowledges or deletes records.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct Observations {
    /// Current state even when no event follows the cursor.
    pub snapshot: Snapshot,
    /// Explicit overwritten prefix, if the requested next event is unavailable.
    pub gap: Option<Gap>,
    /// Bounded retained prefix after the requested cursor.
    pub events: Vec<Record>,
    /// Last returned sequence, or the original cursor when nothing was returned.
    pub next: u64,
}

/// Pre-effect validation, worker creation or local control failure.
#[derive(Debug)]
pub enum Error {
    /// Invalid finite history or query bounds.
    Options(&'static str),
    /// Background delivery cannot own exceptional documentation custody; no worker is started.
    DocumentationCustodyUnavailable,
    /// Native runner preflight rejected the request before spawning.
    Runner(runner::Error),
    /// The worker could not be spawned; no runner acquired inputs or listeners.
    Spawn(io::Error),
    /// An interrupted state lock cannot be treated as a trustworthy observation.
    Poisoned,
    /// The requested cursor is beyond this worker's native observations.
    Cursor,
}

impl Error {
    /// Stable redacted class; native runtime outcomes instead remain in the session.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Options(_) => "preview_session_options",
            Self::DocumentationCustodyUnavailable => "preview_documentation_custody_unavailable",
            Self::Runner(error) => error.code(),
            Self::Spawn(_) => "preview_session_spawn",
            Self::Poisoned => "preview_session_poisoned",
            Self::Cursor => "preview_session_cursor",
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Options(reason) => f.write_str(reason),
            Self::DocumentationCustodyUnavailable => f.write_str("background documentation generation requires an owned exceptional-custody carrier; use synchronous preview with native delivery"),
            Self::Runner(error) => error.fmt(f),
            Self::Spawn(error) => write!(f, "preview worker could not start: {error}"),
            Self::Poisoned => f.write_str("preview observation state is poisoned"),
            Self::Cursor => f.write_str("preview cursor exceeds observed sequence"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Runner(error) => Some(error),
            Self::Spawn(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Default)]
struct History {
    records: VecDeque<(Record, usize)>,
    bytes: usize,
    sequence: u64,
    latest: Option<runner::State>,
    terminal: Option<Terminal>,
}

struct Shared {
    history: Mutex<History>,
    stop: AtomicBool,
}

/// Owns one worker and its native lifecycle. Drop requests stop; it is not a closed receipt.
pub struct Session {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    limits: Limits,
}

impl Session {
    /// Validate then start one fixed request. Success means starting, not ready.
    /// Native startup failures remain inspectable terminal results, never auto-retried.
    ///
    /// # Errors
    /// Returns validation/spawn failure before the worker performs any native operation.
    pub fn start(
        recipe: LocalPublicationRecipe,
        addresses: BTreeMap<Key, SocketAddr>,
        options: runner::Options,
        limits: Limits,
    ) -> Result<Self, Error> {
        if recipe.documentation_generation().is_some() {
            return Err(Error::DocumentationCustodyUnavailable);
        }
        limits.validate()?;
        runner::validate(&recipe, &addresses, &options).map_err(Error::Runner)?;
        let shared = Arc::new(Shared {
            history: Mutex::new(History::default()),
            stop: AtomicBool::new(false),
        });
        let owned = Arc::clone(&shared);
        let worker = thread::Builder::new()
            .name("gazette-preview".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    runner::run(
                        &recipe,
                        addresses,
                        options,
                        |event| record(&owned, limits, event),
                        || {
                            Ok(if owned.stop.load(Ordering::Acquire) {
                                watch::Control::Stop
                            } else {
                                watch::Control::Continue
                            })
                        },
                    )
                }));
                let terminal = match result {
                    Ok(Ok(())) => Terminal::Closed,
                    Ok(Err(error)) => Terminal::Failed { code: error.code() },
                    Err(_) => Terminal::Failed {
                        code: "preview_session_panicked",
                    },
                };
                // This executes only after run returned or unwound, releasing native listeners.
                if let Ok(mut history) = owned.history.lock() {
                    history.terminal = Some(terminal);
                }
            })
            .map_err(Error::Spawn)?;
        Ok(Self {
            shared,
            worker: Some(worker),
            limits,
        })
    }

    /// Read current bounded state without consuming events or performing native reacquisition.
    ///
    /// # Errors
    /// A poisoned observation lock is not silently accepted as current state.
    pub fn inspect(&self) -> Result<Snapshot, Error> {
        let history = self.shared.history.lock().map_err(|_| Error::Poisoned)?;
        Ok(self.snapshot(&history))
    }

    /// Read a retained event prefix without acknowledgement or deletion.
    ///
    /// # Errors
    /// Rejects future cursors, zero/excess counts and poisoned state.
    pub fn poll(&self, after: u64, maximum: usize) -> Result<Observations, Error> {
        if maximum == 0 || maximum > self.limits.events {
            return Err(Error::Options(
                "preview poll count exceeds retained history bound",
            ));
        }
        let history = self.shared.history.lock().map_err(|_| Error::Poisoned)?;
        if after > history.sequence {
            return Err(Error::Cursor);
        }
        let first = history.records.front().map(|(record, _)| record.sequence);
        let gap = first
            .filter(|first| after.saturating_add(1) < *first)
            .map(|first_available| Gap {
                after,
                first_available,
            });
        let events: Vec<_> = history
            .records
            .iter()
            .filter(|(record, _)| record.sequence > after)
            .take(maximum)
            .map(|(record, _)| record.clone())
            .collect();
        let next = events.last().map_or(after, |record| record.sequence);
        Ok(Observations {
            snapshot: self.snapshot(&history),
            gap,
            events,
            next,
        })
    }

    /// Request closure at the next native checkpoint; inspect completion separately.
    ///
    /// # Errors
    /// Preserves a poisoned observation error even though the stop request was recorded.
    pub fn request_stop(&self) -> Result<Snapshot, Error> {
        self.shared.stop.store(true, Ordering::Release);
        self.inspect()
    }

    /// Join only a worker already known finished. `None` means still active.
    /// No event or terminal evidence is removed by this operation.
    /// Inaccessible history yields a failed terminal, permitting deliberate disposal.
    pub fn try_finish(&mut self) -> Option<Terminal> {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            return None;
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            return Some(Terminal::Failed {
                code: "preview_session_panicked",
            });
        }
        Some(match self.shared.history.lock() {
            Ok(history) => history.terminal.clone().unwrap_or(Terminal::Failed {
                code: "preview_session_poisoned",
            }),
            Err(_) => Terminal::Failed {
                code: "preview_session_poisoned",
            },
        })
    }

    fn snapshot(&self, history: &History) -> Snapshot {
        Snapshot {
            stop_requested: self.shared.stop.load(Ordering::Acquire),
            finished: self.worker.as_ref().is_none_or(JoinHandle::is_finished),
            first_available: history.records.front().map(|(record, _)| record.sequence),
            last_sequence: history.sequence,
            latest: history.latest.clone(),
            terminal: history.terminal.clone(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

fn record(shared: &Shared, limits: Limits, event: runner::Event) -> io::Result<()> {
    let mut history = shared
        .history
        .lock()
        .map_err(|_| io::Error::other("preview history poisoned"))?;
    let sequence = history
        .sequence
        .checked_add(1)
        .ok_or_else(|| io::Error::other("preview observation sequence exhausted"))?;
    let record = Record { sequence, event };
    let mut encoded = LimitedWriter::new(Vec::new(), limits.event_bytes);
    serde_json::to_writer(&mut encoded, &record).map_err(io::Error::other)?;
    if encoded.exceeded() {
        return Err(io::Error::other(
            "preview observation exceeds retained bound",
        ));
    }
    let bytes = encoded.into_inner().len();
    while history.records.len() >= limits.events || history.bytes > limits.bytes - bytes {
        if let Some((_, removed)) = history.records.pop_front() {
            history.bytes -= removed;
        }
    }
    match &record.event {
        runner::Event::Ready { state }
        | runner::Event::Changed { state }
        | runner::Event::Recovered { state } => history.latest = Some(state.clone()),
        runner::Event::RebuildFailed { .. } => {
            if let Some(state) = &mut history.latest {
                state.stale = true;
            }
        }
        runner::Event::Closed => (),
    }
    history.sequence = sequence;
    history.bytes += bytes;
    history.records.push_back((record, bytes));
    Ok(())
}
