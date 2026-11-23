//! One native read-snapshot lifecycle for short operations and retained sessions.

use crate::{check_files, native_error, open_existing, regular_file};
use gazette_core::store::{Error, ReadOptions};
use rusqlite::Connection;
use std::{
    fs::Metadata,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

pub(super) struct ReadStore {
    connection: Connection,
    path: PathBuf,
    identity: Metadata,
    options: ReadOptions,
}

impl ReadStore {
    pub(super) fn open(path: &Path, options: ReadOptions) -> Result<Self, Error> {
        options.validate()?;
        // Literal absolute filenames cannot select a URI or in-memory database.
        let path = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir().map_err(Error::Io)?.join(path)
        };
        let identity = check_files(&path, options.storage_bytes)?;
        let connection = open_existing(&path, options)?;
        let store = Self {
            connection,
            path,
            identity,
            options,
        };
        store.verify()?;
        Ok(store)
    }

    fn verify(&self) -> Result<(), Error> {
        let current = check_files(&self.path, self.options.storage_bytes)?;
        if (self.identity.dev(), self.identity.ino()) != (current.dev(), current.ino()) {
            return Err(Error::Changed);
        }
        Ok(())
    }

    pub(super) fn read<T>(
        &mut self,
        read: impl FnOnce(&Connection) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.verify()?;
        let transaction = self.connection.transaction().map_err(native_error)?;
        let result = read(&transaction);
        transaction.rollback().map_err(native_error)?;
        self.verify()?;
        result
    }

    pub(super) fn close(self) -> Result<(), Error> {
        self.connection
            .close()
            .map_err(|(_, error)| native_error(error))?;
        let current = regular_file(&self.path)?;
        if (self.identity.dev(), self.identity.ino()) != (current.dev(), current.ino()) {
            return Err(Error::Changed);
        }
        Ok(())
    }
}
