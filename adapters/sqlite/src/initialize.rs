//! Fresh origin transaction only. Existing state is never selected for writes.

use crate::{
    Sqlite, configure_connection, coordination::Reservation, journal, native_error, regular_file,
    schema, with_source,
};
use gazette_core::store::{
    Error, InitializationReceipt, InitializeOptions, JournalHeader, OutputError,
};
use gazette_ports::store::StoreInitializer;
use rusqlite::{Connection, OpenFlags};
use std::{
    fs::{self, OpenOptions},
    os::unix::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::Path,
};

use crate::journal::DATABASE;

impl StoreInitializer for Sqlite {
    fn initialize(
        &self,
        root: &Path,
        options: InitializeOptions,
    ) -> Result<InitializationReceipt, OutputError> {
        options.validate()?;
        let reservation = Reservation::acquire(root)?;
        options
            .output
            .check_directory_bytes(reservation.path().as_os_str().as_bytes().len())?;
        if fs::read_dir(reservation.path())
            .map_err(Error::Io)?
            .next()
            .transpose()
            .map_err(Error::Io)?
            .is_some()
        {
            return Err(Error::StoreExists.into());
        }
        let path = reservation.path().join(DATABASE);
        // Keep this descriptor until every SQLite connection is closed: closing
        // another descriptor to the same inode can invalidate POSIX database locks.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(Error::Io)?;
        let result = (|| {
            let identity = file.metadata().map_err(Error::Io)?;
            reservation.verify()?;
            let expected = create(&path, options)?;
            reservation.verify()?;
            // A new connection verifies committed state, not the writer's uncommitted view.
            let inspected = with_source(&path, options.session.read, journal::inspect)?;
            if inspected.header != expected || inspected.verifier.journal_mode != "delete" {
                return Err(Error::JournalVerification);
            }
            let current = regular_file(&path)?;
            if (identity.dev(), identity.ino()) != (current.dev(), current.ino())
                || current.mode() & 0o077 != 0
            {
                return Err(Error::Changed);
            }
            if current.len() > options.database_bytes {
                return Err(Error::StorageLimit);
            }
            let mut entries = fs::read_dir(reservation.path()).map_err(Error::Io)?;
            if entries
                .next()
                .transpose()
                .map_err(Error::Io)?
                .is_none_or(|entry| entry.file_name() != DATABASE)
                || entries.next().transpose().map_err(Error::Io)?.is_some()
            {
                return Err(Error::Changed);
            }
            file.sync_all().map_err(Error::Io)?;
            reservation.flush()?;
            Ok(InitializationReceipt {
                directory: reservation.path().to_owned(),
                database: path.clone(),
                header: inspected.header,
                verifier: inspected.verifier,
            })
        })();
        result.map_err(|cause| OutputError {
            directory: Some(reservation.path().to_owned()),
            cause,
        })
    }
}

fn create(path: &Path, options: InitializeOptions) -> Result<JournalHeader, Error> {
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(native_error)?;
    configure_connection(&connection, options.session.read)?;
    connection
        .execute_batch("PRAGMA synchronous=FULL; PRAGMA fullfsync=ON; PRAGMA journal_mode=DELETE;")
        .map_err(native_error)?;
    let page_size: u32 = connection
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(native_error)?;
    let pages = options
        .database_bytes
        .checked_div(u64::from(page_size))
        .filter(|&pages| pages > 0)
        .ok_or(Error::StorageLimit)?;
    let pages =
        u32::try_from(pages.min(u64::from(u32::MAX - 1))).map_err(|_| Error::InvalidLimits)?;
    connection
        .pragma_update(None, "max_page_count", pages)
        .map_err(native_error)?;
    let applied: u32 = connection
        .pragma_query_value(None, "max_page_count", |row| row.get(0))
        .map_err(native_error)?;
    if applied > pages {
        return Err(Error::StorageLimit);
    }
    schema::initialize(&mut connection)?;
    let header = journal::inspect(&connection)?.header;
    connection
        .close()
        .map_err(|(_, error)| native_error(error))?;
    Ok(header)
}
