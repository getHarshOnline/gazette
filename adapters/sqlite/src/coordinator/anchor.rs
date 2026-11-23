//! Fixed bounded anchor bytes with exclusive creation, atomic replacement and native sync.

use super::state_seal;
use crate::{
    backup::{open_unchanged, same_file_state},
    coordination::Reservation,
    regular_file,
};
use gazette_core::{
    render::LimitedWriter,
    store::{Error, LocalCoordinatorState, ReadOptions},
};
use rustix::fs::{Mode, OFlags, open};
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

const FILE: &str = "coordinator.json";
const NEXT: &str = "coordinator.next";
const MAXIMUM: usize = 16 * 1024;

pub(super) struct Observed {
    pub(super) state: LocalCoordinatorState,
    identity: Metadata,
}

pub(super) struct Anchor {
    reservation: Reservation,
    parent: PathBuf,
    parent_file: File,
    parent_identity: Metadata,
    maximum: usize,
}

impl Anchor {
    pub(super) fn acquire(path: &Path, options: ReadOptions) -> Result<Self, Error> {
        if !path.is_absolute() {
            return Err(Error::UnsafePath);
        }
        options.validate()?;
        let maximum = MAXIMUM
            .min(usize::try_from(options.value_bytes).map_err(|_| Error::InvalidLimits)?)
            .min(usize::try_from(options.storage_bytes).unwrap_or(usize::MAX));
        let reservation = Reservation::acquire(path)?;
        let parent = reservation
            .path()
            .parent()
            .ok_or(Error::UnsafePath)?
            .to_owned();
        let parent_file = File::from(
            open(
                &parent,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|error| Error::Io(error.into()))?,
        );
        let parent_identity = parent_file.metadata().map_err(Error::Io)?;
        Ok(Self {
            reservation,
            parent,
            parent_file,
            parent_identity,
            maximum,
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.reservation.path()
    }

    /// Reuse the complete anchor ceiling before retaining an externally supplied ID.
    /// The owner ID remains exact UTF-8; no digest conversion, trimming or case folding.
    pub(super) fn validate_coordinator_id(&self, value: &str) -> Result<(), Error> {
        if value.len() > self.maximum {
            return Err(Error::StorageLimit);
        }
        if value.is_empty() || value.trim() != value || value.chars().any(char::is_control) {
            return Err(Error::CoordinatorState);
        }
        Ok(())
    }

    fn inventory(&self, names: &[&str]) -> Result<(), Error> {
        self.reservation.verify()?;
        let mut entries = fs::read_dir(self.path()).map_err(Error::Io)?;
        let mut count = 0;
        while let Some(entry) = entries.next().transpose().map_err(Error::Io)? {
            if count >= names.len() || !names.iter().any(|name| entry.file_name() == *name) {
                return Err(Error::CoordinatorState);
            }
            count += 1;
        }
        if count != names.len() {
            return Err(Error::CoordinatorState);
        }
        Ok(())
    }

    pub(super) fn empty(&self) -> Result<(), Error> {
        self.inventory(&[])
    }

    fn encode(&self, state: &LocalCoordinatorState) -> Result<Vec<u8>, Error> {
        let mut output = LimitedWriter::new(Vec::new(), self.maximum);
        if serde_json::to_writer(&mut output, state).is_err() {
            return Err(if output.exceeded() {
                Error::StorageLimit
            } else {
                Error::CoordinatorState
            });
        }
        Ok(output.into_inner())
    }

    fn read_file(&self, name: &str) -> Result<(Metadata, Vec<u8>), Error> {
        let path = self.path().join(name);
        let identity = regular_file(&path)?;
        if identity.mode() & 0o077 != 0 || identity.nlink() != 1 {
            return Err(Error::UnsafePath);
        }
        if identity.len() > self.maximum as u64 {
            return Err(Error::StorageLimit);
        }
        let mut file = open_unchanged(&path, &identity)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take(self.maximum as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(Error::Io)?;
        if bytes.len() > self.maximum {
            return Err(Error::StorageLimit);
        }
        if identity.len() != bytes.len() as u64
            || !same_file_state(&identity, &file.metadata().map_err(Error::Io)?)
            || !same_file_state(&identity, &regular_file(&path)?)
        {
            return Err(Error::Changed);
        }
        Ok((identity, bytes))
    }

    pub(super) fn read(&self) -> Result<Observed, Error> {
        self.inventory(&[FILE])?;
        let (identity, bytes) = self.read_file(FILE)?;
        let state: LocalCoordinatorState =
            serde_json::from_slice(&bytes).map_err(|_| Error::CoordinatorState)?;
        if self.encode(&state)? != bytes {
            return Err(Error::CoordinatorState);
        }
        let seal = state_seal(&state);
        self.validate_coordinator_id(&seal.binding.coordinator_id)?;
        if !seal.binding.journal_root.is_absolute()
            || seal.header.journal_id != seal.binding.journal_id
            || seal.header.journal_epoch != seal.binding.journal_epoch
            || seal.header.schema_version != crate::schema::VERSION
            || seal.header.writer_fence > i64::MAX as u64
        {
            return Err(Error::CoordinatorState);
        }
        if let LocalCoordinatorState::InProgress {
            reserved_generation,
            transaction,
            ..
        } = &state
            && (seal.header.writer_fence.checked_add(1) != Some(*reserved_generation)
                || *reserved_generation > i64::MAX as u64
                || transaction.as_ref().is_some_and(|expected| {
                    expected.header.journal_id != seal.header.journal_id
                        || expected.header.journal_epoch != seal.header.journal_epoch
                        || expected.header.schema_version != seal.header.schema_version
                        || expected.header.writer_fence != *reserved_generation
                }))
        {
            return Err(Error::CoordinatorState);
        }
        if let LocalCoordinatorState::Adopting {
            previous,
            source,
            next,
            transaction,
        } = &state
            && (next.binding.coordinator_id != previous.binding.coordinator_id
                || next.binding.principal_scope != previous.binding.principal_scope
                || next.binding.journal_id != previous.binding.journal_id
                || next.binding.journal_epoch != previous.binding.journal_epoch
                || next.header.journal_id != previous.header.journal_id
                || next.header.journal_epoch != previous.header.journal_epoch
                || next.header.schema_version != previous.header.schema_version
                || next.header.writer_fence != previous.header.writer_fence.saturating_add(1)
                || transaction
                    .as_ref()
                    .is_some_and(|expected| expected.header != next.header)
                || source.restore_root == next.binding.journal_root)
        {
            return Err(Error::CoordinatorState);
        }
        self.inventory(&[FILE])?;
        Ok(Observed { state, identity })
    }

    pub(super) fn verify(&self, expected: &Observed) -> Result<(), Error> {
        let observed = self.read()?;
        if observed.state != expected.state
            || !same_file_state(&observed.identity, &expected.identity)
        {
            return Err(Error::CoordinatorMismatch);
        }
        Ok(())
    }

    pub(super) fn write(
        &self,
        state: LocalCoordinatorState,
        previous: Option<&Observed>,
    ) -> Result<Observed, Error> {
        let bytes = self.encode(&state)?;
        if let Some(previous) = previous {
            self.verify(previous)?;
        } else {
            self.empty()?;
        }
        // Everything after the exclusive-create attempt may leave residue or a changed anchor.
        // Never delete it, downgrade it to clean, or hide uncertainty behind generation status.
        self.persist(&bytes, previous)
            .map_err(|cause| Error::CoordinatorUnverified(Box::new(cause)))
    }

    fn persist(&self, bytes: &[u8], previous: Option<&Observed>) -> Result<Observed, Error> {
        let mut next = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(self.path().join(NEXT))
            .map_err(Error::Io)?;
        next.write_all(bytes)
            .and_then(|()| next.sync_all())
            .map_err(Error::Io)?;
        if self.read_file(NEXT)?.1 != bytes {
            return Err(Error::CoordinatorState);
        }
        self.inventory(if previous.is_some() {
            &[FILE, NEXT]
        } else {
            &[NEXT]
        })?;
        if let Some(previous) = previous {
            let (identity, previous_bytes) = self.read_file(FILE)?;
            if !same_file_state(&identity, &previous.identity)
                || self.encode(&previous.state)? != previous_bytes
            {
                return Err(Error::CoordinatorMismatch);
            }
        }
        self.reservation.verify()?;
        fs::rename(self.path().join(NEXT), self.path().join(FILE)).map_err(Error::Io)?;
        self.reservation.flush()?;
        let parent = fs::symlink_metadata(&self.parent).map_err(Error::Io)?;
        if !parent.is_dir()
            || (parent.dev(), parent.ino())
                != (self.parent_identity.dev(), self.parent_identity.ino())
        {
            return Err(Error::Changed);
        }
        self.parent_file.sync_all().map_err(Error::Io)?;
        self.reservation.verify()?;
        let observed = self.read()?;
        if self.encode(&observed.state)? != bytes {
            return Err(Error::CoordinatorState);
        }
        Ok(observed)
    }
}
