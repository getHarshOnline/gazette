//! One independently retained trusted-local anchor around the existing `SQLite` writer.
//!
//! The caller independently selects this private anchor and its principal label.
//! Neither authenticates a host/user. Same-user tampering, copied/rolled-back anchors,
//! whole-system rollback, cross-host fencing and destination leases are not provided.

use crate::{Sqlite, Writer, coordination::Reservation, journal, native_error, with_source};
use gazette_core::{
    source::Sha256Digest,
    store::{
        Error, LocalCoordinatorBinding, LocalCoordinatorSeal, LocalCoordinatorState,
        SessionOptions, WriterAcquireError, WriterOptions,
    },
};
use gazette_ports::store::{LocalCoordinatorStore, ReleaseRecoveryConnector, WriterConnector};
use std::path::{Path, PathBuf};

mod adoption;
mod anchor;
pub(super) mod snapshot;
use anchor::{Anchor, Observed};

/// Inert explicit connector. Construction performs no access and grants no authority.
/// Every acquisition verifies the independently retained binding and required continuity state.
pub struct LocalCoordinator {
    anchor_root: PathBuf,
    binding: LocalCoordinatorBinding,
    allow_anchor_write: bool,
}

impl LocalCoordinator {
    /// Select a local anchor without opening it or trusting its current contents.
    pub fn new(
        anchor_root: PathBuf,
        binding: LocalCoordinatorBinding,
        allow_anchor_write: bool,
    ) -> Self {
        Self {
            anchor_root,
            binding,
            allow_anchor_write,
        }
    }

    /// Enroll only an empty private anchor and a fresh generation-zero, unwritten journal.
    /// Existing anchors/journals are never adopted or reset. Names are caller-selected
    /// labels, not verified publisher credentials. No SQL mutation occurs.
    ///
    /// # Errors
    /// Refuses permission, unsafe/overlapping paths, prior history, limits or persistence failure.
    /// After an anchor write attempt, errors explicitly preserve uncertainty and residue.
    pub fn enroll(
        anchor_root: &Path,
        journal_root: &Path,
        coordinator_id: &str,
        principal_scope: Sha256Digest,
        options: SessionOptions,
        allow_anchor_write: bool,
    ) -> Result<LocalCoordinatorSeal, Error> {
        if !allow_anchor_write {
            return Err(Error::CoordinatorPermission);
        }
        options.validate()?;
        let anchor = Anchor::acquire(anchor_root, options.read)?;
        anchor.validate_coordinator_id(coordinator_id)?;
        anchor.empty()?;
        let reservation = reserve_journal(&anchor, journal_root)?;
        let database = reservation.path().join(journal::DATABASE);
        let header = with_source(&database, options.read, |connection| {
            let inspection = journal::inspect(connection)?;
            if inspection.header.writer_fence != 0
                || inspection.header.journal_epoch != u64::from(journal::INITIAL_EPOCH)
                || inspection.verifier.journal_mode != "delete"
            {
                return Err(Error::CoordinatorState);
            }
            // Native schema verification above establishes the exact trusted table names.
            // Derive this inventory from that owner, never maintain a second history list.
            let mut statement = connection.prepare(
                "SELECT name FROM sqlite_schema WHERE type='table' AND name != 'gazette_journal' ORDER BY name"
            ).map_err(native_error)?;
            let names = statement
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(native_error)?;
            for name in names {
                let name = name.map_err(native_error)?;
                let occupied: bool = connection
                    .query_row(
                        &format!(
                            "SELECT EXISTS(SELECT 1 FROM \"{}\")",
                            name.replace('"', "\"\"")
                        ),
                        [],
                        |row| row.get(0),
                    )
                    .map_err(native_error)?;
                if occupied {
                    return Err(Error::CoordinatorState);
                }
            }
            Ok(inspection.header)
        })?;
        let binding = LocalCoordinatorBinding {
            coordinator_id: coordinator_id.to_owned(),
            principal_scope,
            journal_root: reservation.path().to_owned(),
            journal_id: header.journal_id,
            journal_epoch: header.journal_epoch,
        };
        let writer = Writer::reserved(reservation, header, options.read)?;
        let seal = writer.coordinator_seal(&binding, true)?;
        anchor.write(
            LocalCoordinatorState::Clean {
                seal: seal.clone(),
                adoption: None,
            },
            None,
        )?;
        // No generation was acquired; dropping releases only the retained reservations.
        Ok(seal)
    }

    /// Read bounded anchor state against an independently retained binding, without repair.
    /// This observes anchor history only, not current database bytes or prior fsync success.
    ///
    /// # Errors
    /// Refuses permission, malformed/unsafe state, bounds or exact binding mismatch.
    pub fn inspect(
        anchor_root: &Path,
        expected: &LocalCoordinatorBinding,
        options: SessionOptions,
    ) -> Result<LocalCoordinatorState, Error> {
        options.validate()?;
        let anchor = Anchor::acquire(anchor_root, options.read)?;
        anchor.validate_coordinator_id(&expected.coordinator_id)?;
        let observed = anchor.read()?;
        if state_seal(&observed.state).binding != *expected {
            return Err(Error::CoordinatorMismatch);
        }
        Ok(observed.state)
    }
}

impl LocalCoordinatorStore for Sqlite {
    fn enroll_local_coordinator(
        &self,
        anchor_root: &Path,
        journal_root: &Path,
        coordinator_id: &str,
        principal_scope: Sha256Digest,
        options: SessionOptions,
        allow_anchor_write: bool,
    ) -> Result<LocalCoordinatorSeal, Error> {
        LocalCoordinator::enroll(
            anchor_root,
            journal_root,
            coordinator_id,
            principal_scope,
            options,
            allow_anchor_write,
        )
    }

    fn inspect_local_coordinator(
        &self,
        anchor_root: &Path,
        expected: &LocalCoordinatorBinding,
        options: SessionOptions,
    ) -> Result<LocalCoordinatorState, Error> {
        LocalCoordinator::inspect(anchor_root, expected, options)
    }
}

impl WriterConnector for LocalCoordinator {
    type Writer = Writer;

    fn acquire_writer(
        &self,
        root: &Path,
        expected: gazette_core::store::JournalHeader,
        options: WriterOptions,
    ) -> Result<Writer, WriterAcquireError> {
        permission(options, self.allow_anchor_write)?;
        let anchor = Anchor::acquire(&self.anchor_root, options.session.read)?;
        let observed = anchor.read()?;
        let LocalCoordinatorState::Clean { seal, .. } = &observed.state else {
            return Err(Error::CoordinatorState.into());
        };
        if seal.binding != self.binding || seal.header != expected {
            return Err(Error::CoordinatorMismatch.into());
        }
        if expected.writer_fence >= i64::MAX as u64 {
            return Err(Error::WriterExhausted.into());
        }
        let reservation = reserve_journal(&anchor, root)?;
        if reservation.path() != self.binding.journal_root {
            return Err(Error::CoordinatorMismatch.into());
        }
        let mut writer = Writer::reserved(reservation, expected, options.session.read)?;
        if writer.coordinator_seal(&self.binding, false)? != *seal {
            return Err(Error::CoordinatorMismatch.into());
        }
        let dirty = LocalCoordinatorState::InProgress {
            previous: seal.clone(),
            reserved_generation: expected.writer_fence + 1,
            transaction: None,
        };
        let observed = anchor.write(dirty, Some(&observed))?;
        writer.coordinate(Guard { anchor, observed });
        writer.start().map_err(|error| WriterAcquireError {
            cause: Error::CoordinatorUnverified(Box::new(error.cause)),
            generation: error.generation,
        })
    }
}

impl ReleaseRecoveryConnector for LocalCoordinator {
    type Reconciler = crate::Reconciler;

    fn recover_release(
        &self,
        root: &Path,
        expected: gazette_core::store::JournalHeader,
        profile: gazette_core::store::RecoveryProfile,
        options: SessionOptions,
    ) -> Result<Self::Reconciler, Error> {
        options.validate()?;
        let anchor = Anchor::acquire(&self.anchor_root, options.read)?;
        let observed = anchor.read()?;
        let LocalCoordinatorState::InProgress {
            previous,
            reserved_generation,
            transaction: Some(transaction),
        } = &observed.state
        else {
            return Err(Error::CoordinatorState);
        };
        if previous.binding != self.binding
            || transaction.header != expected
            || expected.writer_fence != *reserved_generation
        {
            return Err(Error::CoordinatorMismatch);
        }
        let reservation = reserve_journal(&anchor, root)?;
        if reservation.path() != self.binding.journal_root {
            return Err(Error::CoordinatorMismatch);
        }
        let mut writer = Writer::reserved(reservation, expected, options.read)?;
        writer.coordinate(Guard { anchor, observed });
        // Final admission verifies the retained complete SQL expectation and global
        // profile invariant under both locks. Never advance or rewrite the anchor.
        writer.into_reconciler(profile, self.allow_anchor_write)
    }
}

fn permission(options: WriterOptions, allow: bool) -> Result<(), Error> {
    if !allow {
        return Err(Error::CoordinatorPermission);
    }
    options.validate()
}

fn reserve_journal(anchor: &Anchor, root: &Path) -> Result<Reservation, Error> {
    if !root.is_absolute() {
        return Err(Error::UnsafePath);
    }
    let reservation = Reservation::acquire(root)?;
    if reservation.path().starts_with(anchor.path())
        || anchor.path().starts_with(reservation.path())
    {
        return Err(Error::UnsafePath);
    }
    Ok(reservation)
}

fn state_seal(state: &LocalCoordinatorState) -> &LocalCoordinatorSeal {
    match state {
        LocalCoordinatorState::Clean { seal, .. } => seal,
        LocalCoordinatorState::InProgress { previous, .. } => previous,
        LocalCoordinatorState::Adopting { previous, .. } => previous,
    }
}

/// Held control guard; drop never seals or repairs uncertain work.
pub(super) struct Guard {
    anchor: Anchor,
    observed: Observed,
}

impl Guard {
    /// Actual held administrative root, never reconstructed from caller evidence.
    pub(super) fn materialization_root(&self) -> Result<PathBuf, Error> {
        self.verify()?;
        Ok(self.anchor.path().to_path_buf())
    }

    /// Persist expected post-state before SQL commit; never infer that it committed.
    pub(super) fn expect_transaction(
        &mut self,
        transaction: gazette_core::store::LocalCoordinatorTransaction,
    ) -> Result<(), Error> {
        let LocalCoordinatorState::InProgress {
            previous,
            reserved_generation,
            ..
        } = &self.observed.state
        else {
            return Err(Error::CoordinatorState);
        };
        if transaction.header.journal_id != previous.binding.journal_id
            || transaction.header.journal_epoch != previous.binding.journal_epoch
            || transaction.header.schema_version != previous.header.schema_version
            || transaction.header.writer_fence != *reserved_generation
        {
            return Err(Error::CoordinatorMismatch);
        }
        let state = LocalCoordinatorState::InProgress {
            previous: previous.clone(),
            reserved_generation: *reserved_generation,
            transaction: Some(transaction),
        };
        // Exact replay need not rewrite the independent anchor.
        if state != self.observed.state {
            self.observed = self.anchor.write(state, Some(&self.observed))?;
        } else {
            self.verify()?;
        }
        Ok(())
    }

    pub(super) fn verify_transaction(
        &self,
        connection: &rusqlite::Connection,
        options: gazette_core::store::ReadOptions,
    ) -> Result<(), Error> {
        if let LocalCoordinatorState::InProgress {
            transaction: Some(expected),
            ..
        } = &self.observed.state
            && snapshot::capture(connection, options)? != *expected
        {
            return Err(Error::CoordinatorMismatch);
        }
        Ok(())
    }

    pub(super) fn binding(&self) -> &LocalCoordinatorBinding {
        &state_seal(&self.observed.state).binding
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        self.anchor.verify(&self.observed)
    }

    pub(super) fn finish(&self, seal: LocalCoordinatorSeal) -> Result<(), Error> {
        let LocalCoordinatorState::InProgress {
            reserved_generation,
            ..
        } = &self.observed.state
        else {
            return Err(Error::CoordinatorState);
        };
        if seal.binding != *self.binding() || seal.header.writer_fence != *reserved_generation {
            return Err(Error::CoordinatorMismatch);
        }
        self.anchor.write(
            LocalCoordinatorState::Clean {
                seal,
                adoption: None,
            },
            Some(&self.observed),
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/internal/coordinator.rs"]
mod tests;
