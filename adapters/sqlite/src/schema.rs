//! Gazette schema definitions and verification; migration machinery stays upstream.

use crate::{journal, native_error};
use gazette_core::{
    source::Sha256Digest,
    store::{Error, Observation},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use rusqlite_migration::{M, Migrations};
use std::sync::OnceLock;

const APPLICATION_ID: i32 = 0x475a4a4c; // GZJL: storage, not a release wire ABI.
pub(super) const VERSION: u32 = 6;
const INITIAL: &str = include_str!("../schema/0001-journal.sql");
const COLLECTION_PURPOSE: &str = include_str!("../schema/0002-collection-purpose.sql");
const COLLECTION_NAVIGATION: &str = include_str!("../schema/0003-collection-navigation.sql");
const PROJECT_LIFECYCLE: &str = include_str!("../schema/0004-project-lifecycle.sql");
const COLLECTION_ORDER: &str = include_str!("../schema/0005-collection-order.sql");
const PROJECT_SUBJECT: &str = include_str!("../schema/0006-project-subject.sql");

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up_with_hook(INITIAL, |transaction: &Transaction<'_>| {
            journal::seed(transaction)?;
            transaction.pragma_update(None, "application_id", APPLICATION_ID)?;
            Ok(())
        }),
        M::up(COLLECTION_PURPOSE),
        M::up(COLLECTION_NAVIGATION),
        M::up(PROJECT_LIFECYCLE),
        M::up(COLLECTION_ORDER),
        M::up(PROJECT_SUBJECT),
    ])
}

// Only fresh initialization calls this. Existing-store inspection must never
// call a migration API, even when a future Gazette version adds an upgrade.
pub(super) fn initialize(connection: &mut Connection) -> Result<(), Error> {
    let application_id: i32 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(native_error)?;
    let version: i32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(native_error)?;
    let occupied: bool = connection
        .query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |row| {
            row.get(0)
        })
        .map_err(native_error)?;
    if application_id != 0 || version != 0 || occupied {
        return Err(Error::JournalSchema);
    }
    // Pinned rusqlite_migration respects this setting. Its one transaction
    // covers DDL, the origin hook and user_version; there is no nested runner.
    connection.set_transaction_behavior(TransactionBehavior::Immediate);
    migrations().to_latest(connection).map_err(migration_error)
}

fn migration_error(error: rusqlite_migration::Error) -> Error {
    match error {
        rusqlite_migration::Error::RusqliteError { err, .. } => native_error(err),
        rusqlite_migration::Error::ForeignKeyCheck(_) => Error::JournalVerification,
        _ => Error::JournalSchema,
    }
}

pub(super) fn digest() -> Sha256Digest {
    Sha256Digest::of(
        [
            INITIAL.as_bytes(),
            COLLECTION_PURPOSE.as_bytes(),
            COLLECTION_NAVIGATION.as_bytes(),
            PROJECT_LIFECYCLE.as_bytes(),
            COLLECTION_ORDER.as_bytes(),
            PROJECT_SUBJECT.as_bytes(),
        ]
        .concat()
        .as_slice(),
    )
}

pub(super) fn verify(connection: &Connection, observation: &Observation) -> Result<(), Error> {
    if observation.application_id != APPLICATION_ID
        || u32::try_from(observation.user_version).ok() != Some(VERSION)
    {
        return Err(Error::JournalSchema);
    }
    let expected = expected()?;
    if catalog(connection, expected.objects.len() + 1)? != expected.objects {
        return Err(Error::JournalSchema);
    }
    Ok(())
}

type Object = (String, String, String, Option<String>);

#[derive(Clone)]
pub(super) struct Table {
    pub name: String,
    pub columns: usize,
    pub scan: String,
}

struct Expected {
    objects: Vec<Object>,
    tables: Vec<Table>,
    registry: Vec<Table>,
}

fn catalog(connection: &Connection, maximum: usize) -> Result<Vec<Object>, Error> {
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_schema ORDER BY name LIMIT ?1")
        .map_err(native_error)?;
    statement
        .query_map([maximum as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(native_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(native_error)
}

fn expected() -> Result<&'static Expected, Error> {
    static EXPECTED: OnceLock<Result<Expected, ()>> = OnceLock::new();
    EXPECTED
        .get_or_init(|| {
            let derive = || -> Result<Expected, Error> {
                let connection = Connection::open_in_memory().map_err(native_error)?;
                connection.execute_batch(INITIAL).map_err(native_error)?;
                connection
                    .execute_batch(COLLECTION_PURPOSE)
                    .map_err(native_error)?;
                connection
                    .execute_batch(COLLECTION_NAVIGATION)
                    .map_err(native_error)?;
                connection
                    .execute_batch(PROJECT_LIFECYCLE)
                    .map_err(native_error)?;
                connection
                    .execute_batch(COLLECTION_ORDER)
                    .map_err(native_error)?;
                connection
                    .execute_batch(PROJECT_SUBJECT)
                    .map_err(native_error)?;
                // The trusted DDL bounds its own object count; actual stores are read
                // only to that count plus one. Internal indexes are compared exactly.
                let objects = catalog(
                    &connection,
                    INITIAL.len()
                        + COLLECTION_PURPOSE.len()
                        + COLLECTION_NAVIGATION.len()
                        + PROJECT_LIFECYCLE.len()
                        + COLLECTION_ORDER.len()
                        + PROJECT_SUBJECT.len(),
                )?;
                let mut tables = Vec::new();
                let mut registry = Vec::new();
                for (kind, name, _, _) in &objects {
                    if kind == "table" {
                        let statement = connection
                            .prepare(&format!("SELECT * FROM \"{name}\" LIMIT 0"))
                            .map_err(native_error)?;
                        let table = Table {
                            name: name.clone(),
                            columns: statement.column_count(),
                            scan: ordered_scan(&connection, name)?,
                        };
                        if name.starts_with("gazette_registry_") && name != "gazette_registry_head"
                        {
                            registry.push(table.clone());
                        }
                        tables.push(table);
                    }
                }
                Ok(Expected {
                    objects,
                    tables,
                    registry,
                })
            };
            derive().map_err(|_| ())
        })
        .as_ref()
        .map_err(|()| Error::JournalSchema)
}

pub(super) fn registry_tables() -> Result<&'static [Table], Error> {
    Ok(&expected()?.registry)
}

pub(super) fn tables() -> Result<&'static [Table], Error> {
    Ok(&expected()?.tables)
}

// Derive the unique native traversal from trusted DDL, not from mutable store
// contents or a hand-maintained history-table catalog. Never sort large payloads.
fn ordered_scan(connection: &Connection, table: &str) -> Result<String, Error> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info(\"{table}\")"))
        .map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let mut primary = Vec::new();
    while let Some(row) = rows.next().map_err(native_error)? {
        let ordinal: u32 = row.get(5).map_err(native_error)?;
        if ordinal != 0 {
            let column: u32 = row.get(0).map_err(native_error)?;
            primary.push((ordinal, column + 1));
        }
    }
    if primary.is_empty() {
        return Err(Error::JournalSchema);
    }
    primary.sort_unstable();
    let order = primary
        .iter()
        .map(|(_, column)| column.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut statement = connection
        .prepare(&format!("PRAGMA index_list(\"{table}\")"))
        .map_err(native_error)?;
    let mut rows = statement.query([]).map_err(native_error)?;
    let mut index = None;
    while let Some(row) = rows.next().map_err(native_error)? {
        if row.get::<_, String>(3).map_err(native_error)? == "pk" {
            if index.is_some() {
                return Err(Error::JournalSchema);
            }
            index = Some(row.get::<_, String>(1).map_err(native_error)?);
        }
    }
    // An INTEGER PRIMARY KEY aliases rowid and needs no separate index. All
    // other admitted keys have their unique native primary-key index.
    let traversal = index.map_or_else(
        || "NOT INDEXED".to_owned(),
        |name| format!("INDEXED BY \"{}\"", name.replace('"', "\"\"")),
    );
    Ok(format!(
        "SELECT * FROM \"{table}\" {traversal} ORDER BY {order}"
    ))
}

// Ephemeral relational qualification uses exactly the native schema and bounds;
// it creates no journal origin and never adopts or writes a caller's store.
pub(super) fn validation_connection(
    options: gazette_core::store::ReadOptions,
) -> Result<Connection, Error> {
    let connection = Connection::open_in_memory().map_err(native_error)?;
    crate::configure_connection(&connection, options)?;
    connection.execute_batch(INITIAL).map_err(native_error)?;
    connection
        .execute_batch(COLLECTION_PURPOSE)
        .map_err(native_error)?;
    connection
        .execute_batch(COLLECTION_NAVIGATION)
        .map_err(native_error)?;
    connection
        .execute_batch(PROJECT_LIFECYCLE)
        .map_err(native_error)?;
    connection
        .execute_batch(COLLECTION_ORDER)
        .map_err(native_error)?;
    connection
        .execute_batch(PROJECT_SUBJECT)
        .map_err(native_error)?;
    Ok(connection)
}

#[cfg(test)]
#[path = "../tests/internal/schema.rs"]
mod tests;
