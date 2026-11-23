//! Process-owned release-session custody shared by CLI and MCP projections.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, TryLockError},
};

use gazette_sdk::{release::session, store::JournalInspection};

use super::{Command, Kernel};

const CAPACITY: usize = 4;
type HeldSession = Arc<Mutex<session::Session>>;
type SessionRegistry = BTreeMap<String, HeldSession>;

#[derive(Default)]
pub(super) struct Sessions {
    /// Short-lived registry custody; each retained session serializes only its own effects.
    entries: Mutex<SessionRegistry>,
}

/// Registry or response-reservation failure before an operation starts.
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
    /// The transport could not reserve its complete custody response.
    Reservation(E),
}

/// Result of attempting to acquire one release writer.
#[derive(Debug)]
pub struct OpenReport {
    /// Opaque process-local handle selected before native work.
    pub session: String,
    /// Whether the kernel retained native custody for the handle.
    pub retained: bool,
    /// Actual native readiness or native open failure.
    pub outcome: OpenOutcome,
}

/// Native release-session open result.
#[derive(Debug)]
pub enum OpenOutcome {
    /// The held writer returned independently inspected readiness.
    Ready {
        /// Exact native journal inspection.
        inspection: JournalInspection,
    },
    /// Native acquisition or readiness failed without an active handle.
    Failed {
        /// Existing SDK-owned open failure.
        error: session::OpenError,
    },
}

/// Result of one operation under already-held release custody.
#[derive(Debug)]
pub struct OperationReport {
    /// Opaque process-local handle.
    pub session: String,
    /// Whether the kernel retained native custody after the operation.
    pub retained: bool,
    /// Existing SDK-owned operation result.
    pub outcome: session::Outcome,
}

/// Acquire one release writer after the caller reserves response custody.
pub struct Open<R> {
    /// Opaque handle chosen before native work.
    pub handle: String,
    /// Complete native acquisition selection.
    pub request: session::OpenRequest,
    /// Pre-effect response reservation under the registry lock.
    pub reserve: R,
}

impl<R, E> Command for Open<R>
where
    R: FnOnce(&str, &session::OpenRequest) -> Result<(), E>,
{
    type Outcome = OpenReport;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let mut entries = lock(&kernel.release_session)?;
        if entries.len() >= CAPACITY {
            return Err(Failure::Capacity);
        }
        if entries.contains_key(&self.handle) {
            return Err(Failure::HandleCollision);
        }
        (self.reserve)(&self.handle, &self.request).map_err(Failure::Reservation)?;
        match session::Session::open(self.request) {
            Ok((held, inspection)) => {
                entries.insert(self.handle.clone(), Arc::new(Mutex::new(held)));
                Ok(OpenReport {
                    session: self.handle,
                    retained: true,
                    outcome: OpenOutcome::Ready { inspection },
                })
            }
            Err(error) => Ok(OpenReport {
                session: self.handle,
                retained: false,
                outcome: OpenOutcome::Failed { error },
            }),
        }
    }
}

/// Execute one operation after reserving its exact held-custody fallback.
pub struct Operate<R> {
    /// Existing opaque process-local handle.
    pub handle: String,
    /// Exact SDK operation; no operation is inferred or retried.
    pub operation: session::Operation,
    /// Pre-effect response reservation over the retained original selection.
    pub reserve: R,
}

impl<R, E> Command for Operate<R>
where
    R: FnOnce(&str, &session::OpenRequest) -> Result<(), E>,
{
    type Outcome = OperationReport;
    type Failure = Failure<E>;

    fn execute(self, kernel: &Kernel) -> Result<Self::Outcome, Self::Failure> {
        let selected = {
            let entries = lock(&kernel.release_session)?;
            Arc::clone(entries.get(&self.handle).ok_or(Failure::Unknown)?)
        };
        let mut held = match selected.try_lock() {
            Ok(held) => held,
            Err(TryLockError::WouldBlock) => return Err(Failure::Busy),
            Err(TryLockError::Poisoned(_)) => return Err(Failure::Unavailable),
        };
        (self.reserve)(&self.handle, held.selection()).map_err(Failure::Reservation)?;
        let outcome = held.execute(&self.operation);
        let retained = !held.is_closed();
        drop(held);
        if !retained {
            let mut entries = lock(&kernel.release_session)?;
            if entries
                .get(&self.handle)
                .is_some_and(|current| Arc::ptr_eq(current, &selected))
            {
                entries.remove(&self.handle);
            }
        }
        Ok(OperationReport {
            session: self.handle,
            retained,
            outcome,
        })
    }
}

fn lock<E>(sessions: &Sessions) -> Result<std::sync::MutexGuard<'_, SessionRegistry>, Failure<E>> {
    match sessions.entries.try_lock() {
        Ok(entries) => Ok(entries),
        Err(TryLockError::WouldBlock) => Err(Failure::Busy),
        Err(TryLockError::Poisoned(_)) => Err(Failure::Unavailable),
    }
}
