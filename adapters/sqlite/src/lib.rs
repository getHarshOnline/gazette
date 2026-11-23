//! Keeps Gazette's release journal in `SQLite`: creating it, observing it, backing it
//! up and preparing a detached restore.
//!
//! This adapter builds on `gazette-core` and the capability traits in `gazette-ports`;
//! `gazette-sdk` builds on it. Source SQL access is read-only, but WAL coordination
//! files may be maintained. Backups write only fresh private output under separate
//! creation permission. Initialization writes only a new journal in an explicitly
//! reserved empty directory. Writer acquisition separately permits one exact expected
//! local generation advancement. Use an existing local database in a trusted private
//! directory. Pre/post checks detect ordinary path replacement; this is not a hostile
//! same-user filesystem sandbox or durable recovery qualification. Backup integrity
//! checks apply to the copied database, not Website schema or release-history
//! acceptance.

#[cfg(not(unix))]
compile_error!("Gazette's current SQLite path checks require Unix file identity");

use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::Duration;

use gazette_core::store::{Error, Observation, ReadOptions};
use gazette_ports::store::StoreReader;
use rusqlite::{Connection, ErrorCode, OpenFlags, config::DbConfig, limits::Limit};

mod attempt;
mod authorization;
mod backup;
mod cache;
mod checkpoint;
mod connection;
mod consumption;
mod coordination;
mod coordinator;
mod initialize;
mod journal;
mod materialization;
mod preparation;
mod readback;
mod registry;
mod schema;
mod session;
mod settlement;
mod writer;
pub use coordinator::LocalCoordinator;
pub use session::Session;
pub use writer::{Reconciler, Writer};

/// Pinned embedded `SQLite` adapter; no ambient store path or mutable global state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sqlite;

impl Sqlite {
    /// Resolve the adapter-owned database name within an explicitly selected journal root.
    /// This is lexical composition only, not path validation, a read or a lock. Native
    /// readers/writers retain their permission, confinement and freshness checks.
    pub fn journal_database(root: &Path) -> PathBuf {
        root.join(journal::DATABASE)
    }
}

impl StoreReader for Sqlite {
    fn observe(&self, path: &Path, options: ReadOptions) -> Result<Observation, Error> {
        with_source(path, options, read_metadata)
    }
}

fn with_source<T>(
    path: &Path,
    options: ReadOptions,
    read: impl FnOnce(&Connection) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut store = connection::ReadStore::open(path, options)?;
    let result = store.read(read);
    let closed = store.close();
    result.and_then(|value| closed.map(|()| value))
}

fn read_metadata(connection: &Connection) -> Result<Observation, Error> {
    let (provider_version, provider_source_id) = connection
        .query_row("SELECT sqlite_version(), sqlite_source_id()", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .map_err(native_error)?;
    Ok(Observation {
        provider: "sqlite".into(),
        provider_version,
        provider_source_id,
        application_id: connection
            .pragma_query_value(None, "application_id", |row| row.get(0))
            .map_err(native_error)?,
        user_version: connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(native_error)?,
        journal_mode: connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(native_error)?,
        sql_read_only: connection.is_readonly("main").map_err(native_error)?,
    })
}

fn open_existing(path: &Path, options: ReadOptions) -> Result<Connection, Error> {
    // Resolve the trusted parent (including macOS's /var alias), never the
    // database filename. NOFOLLOW still rejects a replaced final component.
    let parent = path
        .parent()
        .ok_or(Error::UnsafePath)?
        .canonicalize()
        .map_err(Error::Io)?;
    let path = parent.join(path.file_name().ok_or(Error::UnsafePath)?);
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_PRIVATE_CACHE
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = Connection::open_with_flags(&path, flags).map_err(native_error)?;
    if !connection.is_readonly("main").map_err(native_error)? {
        return Err(Error::Provider { code: None });
    }
    configure_connection(&connection, options)?;
    // This changes connection policy only, not database schema, journaling or data.
    connection
        .pragma_update(None, "query_only", true)
        .map_err(native_error)?;
    Ok(connection)
}

// Common defensive settings and lifetime work ceilings; access mode remains
// explicit at each opener. This does not authorize any SQL or filesystem effect.
fn configure_connection(connection: &Connection, options: ReadOptions) -> Result<(), Error> {
    connection
        .busy_timeout(Duration::from_millis(options.busy_timeout_ms.into()))
        .map_err(native_error)?;
    for (config, value) in [
        (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
        // Relational constraints must not depend on provider build defaults.
        // This configures enforcement; it does not validate or repair existing rows.
        (DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true),
        (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
        (DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_CREATE, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_ATTACH_WRITE, false),
    ] {
        if connection
            .set_db_config(config, value)
            .map_err(native_error)?
            != value
        {
            return Err(Error::Provider { code: None });
        }
    }
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_SQL_LENGTH, options.sql_bytes),
        (Limit::SQLITE_LIMIT_LENGTH, options.value_bytes),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_WORKER_THREADS, 0),
    ] {
        connection.set_limit(limit, value).map_err(native_error)?;
    }
    let mut remaining = options.vm_steps;
    connection
        .progress_handler(
            1,
            Some(move || {
                if remaining == 0 {
                    true
                } else {
                    remaining -= 1;
                    false
                }
            }),
        )
        .map_err(native_error)?;
    Ok(())
}

fn check_files(path: &Path, limit: u64) -> Result<Metadata, Error> {
    let main = regular_file(path)?;
    if main.len() == 0 {
        return Err(Error::Uninitialized);
    }
    let mut total = main.len();
    if total > limit {
        return Err(Error::StorageLimit);
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::symlink_metadata(PathBuf::from(name)) {
            Ok(meta) => {
                if !meta.is_file() {
                    return Err(Error::UnsafePath);
                }
                total = total
                    .checked_add(meta.len())
                    .filter(|&bytes| bytes <= limit)
                    .ok_or(Error::StorageLimit)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(Error::Io(error)),
        }
    }
    Ok(main)
}

fn regular_file(path: &Path) -> Result<Metadata, Error> {
    let metadata = fs::symlink_metadata(path).map_err(Error::Io)?;
    if !metadata.is_file() {
        return Err(Error::UnsafePath);
    }
    Ok(metadata)
}

fn native_error(error: rusqlite::Error) -> Error {
    if error.sqlite_error().is_some_and(|error| {
        matches!(
            error.extended_code,
            rusqlite::ffi::SQLITE_READONLY_RECOVERY | rusqlite::ffi::SQLITE_READONLY_ROLLBACK
        )
    }) {
        return Error::RecoveryRequired;
    }
    match error.sqlite_error_code() {
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked) => Error::Busy,
        Some(ErrorCode::OperationInterrupted) => Error::Interrupted,
        _ => Error::Provider {
            code: error.sqlite_error().map(|error| error.extended_code),
        },
    }
}

#[cfg(test)]
#[path = "../tests/internal/connection.rs"]
mod tests;
