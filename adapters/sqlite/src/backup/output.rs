//! One fresh-output lifecycle for backups and detached recovery preparations.

use super::{DATABASE_FILE, check_directory, new_file, read};
use gazette_core::{
    source::Sha256Digest,
    store::{Error, OutputError, OutputLimits},
};
use serde::Serialize;
use std::{
    fs::{self, File, Metadata},
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, PermissionsExt},
    },
    path::{Path, PathBuf},
};

const RANDOM_CHARACTERS: usize = 6;

pub(super) struct OutputDirectory {
    directory: PathBuf,
    identity: Metadata,
    parent: PathBuf,
    parent_file: File,
    parent_identity: Metadata,
}

impl OutputDirectory {
    pub(super) fn create(
        parent: &Path,
        prefix: &str,
        limits: OutputLimits,
    ) -> Result<Self, OutputError> {
        limits.validate()?;
        let parent = parent.canonicalize().map_err(Error::Io)?;
        // The native builder appends exactly this many ASCII characters. Bound
        // the resolved path before creation; caller aliases cannot undercount it.
        let directory_bytes = parent
            .join(prefix)
            .as_os_str()
            .as_bytes()
            .len()
            .checked_add(RANDOM_CHARACTERS)
            .ok_or(Error::InvalidLimits)?;
        limits.check_directory_bytes(directory_bytes)?;
        let parent_file = open_directory(&parent)?;
        let parent_identity = parent_file.metadata().map_err(Error::Io)?;
        if !parent_identity.is_dir() {
            return Err(Error::UnsafePath.into());
        }
        let directory = tempfile::Builder::new()
            .prefix(prefix)
            .rand_bytes(RANDOM_CHARACTERS)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&parent)
            .map_err(Error::Io)?
            .keep();
        let identity = fs::symlink_metadata(&directory).map_err(|cause| OutputError {
            directory: Some(directory.clone()),
            cause: Error::Io(cause),
        })?;
        Ok(Self {
            directory,
            identity,
            parent,
            parent_file,
            parent_identity,
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.directory
    }

    pub(super) fn failure(&self, cause: Error) -> OutputError {
        OutputError {
            directory: Some(self.directory.clone()),
            cause,
        }
    }

    pub(super) fn seal(
        &self,
        filename: &str,
        payload: &impl Serialize,
    ) -> Result<Sha256Digest, Error> {
        check_directory(&self.directory, &self.identity, &[DATABASE_FILE])?;
        let bytes = serde_json::to_vec_pretty(payload)
            .map_err(|error| Error::Io(std::io::Error::other(error)))?;
        let path = self.directory.join(filename);
        let mut receipt = new_file(&path)?;
        receipt
            .write_all(&bytes)
            .and_then(|()| receipt.sync_all())
            .map_err(Error::Io)?;
        let (_, read_back) = read::read_manifest(&path, bytes.len() as u64)?;
        if read_back != bytes {
            return Err(Error::BackupVerification);
        }
        check_directory(&self.directory, &self.identity, &[DATABASE_FILE, filename])?;
        let directory_file = open_directory(&self.directory)?;
        let opened = directory_file.metadata().map_err(Error::Io)?;
        if (self.identity.dev(), self.identity.ino()) != (opened.dev(), opened.ino()) {
            return Err(Error::Changed);
        }
        directory_file.sync_all().map_err(Error::Io)?;
        let current_parent = fs::metadata(&self.parent).map_err(Error::Io)?;
        if (self.parent_identity.dev(), self.parent_identity.ino())
            != (current_parent.dev(), current_parent.ino())
        {
            return Err(Error::Changed);
        }
        self.parent_file.sync_all().map_err(Error::Io)?;
        Ok(Sha256Digest::of(&bytes))
    }
}

fn open_directory(path: &Path) -> Result<File, Error> {
    use rustix::fs::{Mode, OFlags, open};

    open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| Error::Io(error.into()))
}

#[cfg(test)]
#[path = "../../tests/internal/output.rs"]
mod tests;
