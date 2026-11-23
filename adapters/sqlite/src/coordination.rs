//! Lifetime-held local directory reservation, shared by connection and initialization.

use gazette_core::store::Error;
use rustix::fs::{FlockOperation, Mode, OFlags, flock, open};
use std::{
    fs::{self, File, Metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) struct Reservation {
    // Explicit drop unlocks even with an incidental pre-exec duplicate. Kernel
    // descriptor cleanup also releases locks after process death; never trust a PID.
    file: File,
    path: PathBuf,
    identity: Metadata,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        // CLOEXEC does not prevent brief inheritance across fork. Closing only
        // our descriptor can leave that incidental duplicate holding the lock.
        // No reservation transfer is supported: its owning Rust lifetime ends here.
        let _ = flock(&self.file, FlockOperation::Unlock);
    }
}

#[cfg(test)]
#[path = "../tests/internal/coordination.rs"]
mod tests;

impl Reservation {
    pub(super) fn acquire(path: &Path) -> Result<Self, Error> {
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir().map_err(Error::Io)?.join(path)
        };
        // Resolve trusted ancestors, not the final component, including trailing-slash aliases.
        let path = absolute
            .parent()
            .ok_or(Error::UnsafePath)?
            .canonicalize()
            .map_err(Error::Io)?
            .join(absolute.file_name().ok_or(Error::UnsafePath)?);
        let descriptor = open(
            &path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| Error::Io(error.into()))?;
        let file = File::from(descriptor);
        let identity = file.metadata().map_err(Error::Io)?;
        if identity.mode() & 0o077 != 0 {
            return Err(Error::UnsafePath);
        }
        flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|error| {
            if error == rustix::io::Errno::WOULDBLOCK {
                Error::ReservationBusy
            } else {
                Error::Io(error.into())
            }
        })?;
        let reservation = Self {
            file,
            path,
            identity,
        };
        reservation.verify()?;
        Ok(reservation)
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn verify(&self) -> Result<(), Error> {
        let current = fs::symlink_metadata(&self.path).map_err(Error::Io)?;
        if !current.is_dir()
            || current.mode() & 0o077 != 0
            || (self.identity.dev(), self.identity.ino()) != (current.dev(), current.ino())
        {
            return Err(Error::Changed);
        }
        Ok(())
    }

    pub(super) fn flush(&self) -> Result<(), Error> {
        self.verify()?;
        self.file.sync_all().map_err(Error::Io)?;
        self.verify()
    }
}
