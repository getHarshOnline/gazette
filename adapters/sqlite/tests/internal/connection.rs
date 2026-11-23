//! Production connection qualification on disposable `SQLite` stores.

use crate::{
    Connection, DbConfig, Error, ErrorCode, ReadOptions, Sqlite, StoreReader, configure_connection,
    open_existing,
};
use std::{fs, os::unix::fs::symlink};
use tempfile::tempdir;

fn options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..ReadOptions::default()
    }
}

#[test]
fn actual_connection_is_read_only_without_schema_or_data_mutation() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    let writer = Connection::open(&path).expect("fixture creation");
    writer.execute_batch("CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES ('retained'); PRAGMA application_id=73; PRAGMA user_version=9;").expect("fixture schema");
    drop(writer);
    let before = fs::read(&path).expect("bytes");
    let reader = open_existing(&path, options()).expect("production connection");
    assert!(reader.is_readonly("main").expect("native readonly"));
    assert!(
        reader
            .db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY)
            .expect("foreign-key enforcement on production reader")
    );
    assert!(
        reader
            .db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE)
            .expect("defensive")
    );
    assert!(
        !reader
            .db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA)
            .expect("untrusted schema")
    );
    assert!(
        reader
            .db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE)
            .expect("no checkpoint")
    );
    for sql in [
        "INSERT INTO fixture VALUES ('changed')",
        "CREATE TABLE invented(x)",
        "PRAGMA user_version=10",
    ] {
        assert_eq!(
            reader
                .execute_batch(sql)
                .expect_err("mutation prohibited")
                .sqlite_error_code(),
            Some(ErrorCode::ReadOnly)
        );
    }
    assert!(
        reader
            .execute_batch("ATTACH DATABASE ':memory:' AS alternate")
            .is_err()
    );
    let value: String = reader
        .query_row("SELECT value FROM fixture", [], |r| r.get(0))
        .expect("read");
    assert_eq!(value, "retained");
    drop(reader);
    let report = Sqlite.observe(&path, options()).expect("observe");
    assert_eq!((report.application_id, report.user_version), (73, 9));
    assert_eq!(report.journal_mode, "delete");
    assert_eq!(report.provider_version, "3.51.3");
    assert_eq!(
        report.provider_source_id,
        "2026-03-13 10:38:09 737ae4a34738ffa0c3ff7f9bb18df914dd1cad163f28fd6b6e114a344fe6d618"
    );
    assert_eq!(fs::read(&path).expect("after"), before);
    assert_eq!(fs::read_dir(root.path()).expect("root").count(), 1);
}

#[test]
fn missing_permission_missing_database_empty_and_corrupt_files_never_initialize() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("missing.db");
    assert!(matches!(
        Sqlite.observe(&path, ReadOptions::default()),
        Err(Error::CoordinationPermission)
    ));
    assert!(matches!(
        Sqlite.observe(&path, options()),
        Err(Error::Io(_))
    ));
    assert!(!path.exists());
    fs::write(&path, []).expect("empty fixture");
    assert!(matches!(
        Sqlite.observe(&path, options()),
        Err(Error::Uninitialized)
    ));
    assert_eq!(fs::metadata(&path).expect("metadata").len(), 0);
    fs::write(&path, b"not a SQLite database").expect("bad fixture");
    assert!(matches!(
        Sqlite.observe(&path, options()),
        Err(Error::Provider { .. })
    ));
    assert_eq!(fs::read(path).expect("unchanged"), b"not a SQLite database");
}

#[test]
fn wal_committed_state_is_visible_and_uncommitted_state_is_not_without_checkpointing() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    let writer = Connection::open(&path).expect("fixture database");
    writer.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE fixture(x); PRAGMA user_version=37;").expect("WAL state");
    let main_before = fs::read(&path).expect("main bytes");
    let wal = root.path().join("store.db-wal");
    let wal_before = fs::read(&wal).expect("WAL bytes");
    assert!(!wal_before.is_empty());
    let report = Sqlite.observe(&path, options()).expect("WAL observation");
    assert_eq!(report.user_version, 37, "no immutable mode ignoring WAL");
    assert_eq!(report.journal_mode, "wal");
    assert_eq!(fs::read(&path).expect("main unchanged"), main_before);
    assert_eq!(fs::read(&wal).expect("WAL unchanged"), wal_before);
    writer
        .execute_batch("BEGIN IMMEDIATE; PRAGMA user_version=99;")
        .expect("uncommitted");
    assert_eq!(
        Sqlite
            .observe(&path, options())
            .expect("isolated reader")
            .user_version,
        37
    );
    writer.execute_batch("ROLLBACK").expect("fixture rollback");
}

#[test]
fn locking_and_resource_failures_are_explicit() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    let writer = Connection::open(&path).expect("fixture");
    writer
        .execute_batch("CREATE TABLE fixture(x); PRAGMA user_version=7;")
        .expect("schema");
    assert!(matches!(
        Sqlite.observe(
            &path,
            ReadOptions {
                storage_bytes: 1,
                ..options()
            }
        ),
        Err(Error::StorageLimit)
    ));
    assert!(matches!(
        Sqlite.observe(
            &path,
            ReadOptions {
                vm_steps: 1,
                ..options()
            }
        ),
        Err(Error::Interrupted)
    ));
    assert!(
        Sqlite
            .observe(
                &path,
                ReadOptions {
                    sql_bytes: 1,
                    ..options()
                }
            )
            .is_err()
    );
    assert!(
        Sqlite
            .observe(
                &path,
                ReadOptions {
                    value_bytes: 1,
                    ..options()
                }
            )
            .is_err()
    );
    writer
        .execute_batch("BEGIN EXCLUSIVE; UPDATE fixture SET x=1;")
        .expect("writer lock");
    assert!(matches!(
        Sqlite.observe(
            &path,
            ReadOptions {
                busy_timeout_ms: 0,
                ..options()
            }
        ),
        Err(Error::Busy)
    ));
    writer.execute_batch("ROLLBACK").expect("unlock");
    assert_eq!(
        Sqlite
            .observe(&path, options())
            .expect("retry")
            .user_version,
        7
    );
}

#[test]
fn main_and_sidecar_links_are_rejected_before_connection() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    let outside = root.path().join("outside.db");
    let writer = Connection::open(&outside).expect("fixture");
    writer
        .execute_batch("CREATE TABLE fixture(x)")
        .expect("schema");
    drop(writer);
    let before = fs::read(&outside).expect("bytes");
    symlink(&outside, &path).expect("main link");
    assert!(matches!(
        Sqlite.observe(&path, options()),
        Err(Error::UnsafePath)
    ));
    fs::remove_file(&path).expect("remove disposable symlink");
    fs::copy(&outside, &path).expect("closed fixture copy, not live backup");
    symlink(&outside, root.path().join("store.db-wal")).expect("sidecar link");
    assert!(matches!(
        Sqlite.observe(&path, options()),
        Err(Error::UnsafePath)
    ));
    assert_eq!(fs::read(outside).expect("unchanged"), before);
}

#[test]
fn configured_connection_enforces_references_and_cascades_without_user_triggers() {
    let connection = Connection::open_in_memory().expect("disposable provider");
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, false)
        .expect("explicit disabled precondition independent of provider defaults");
    configure_connection(&connection, options()).expect("production safeguards");
    connection
        .execute_batch(
            "CREATE TABLE parent(id TEXT PRIMARY KEY) STRICT;
         CREATE TABLE child(id TEXT PRIMARY KEY, parent TEXT NOT NULL
             REFERENCES parent(id) ON UPDATE CASCADE ON DELETE CASCADE) STRICT;
         CREATE TABLE audit(value TEXT) STRICT;
         CREATE TRIGGER child_deleted AFTER DELETE ON child BEGIN
             INSERT INTO audit VALUES (old.id);
         END;
         INSERT INTO parent VALUES ('original');
         INSERT INTO child VALUES ('retained', 'original');",
        )
        .expect("native relational fixture");
    assert_eq!(
        connection
            .execute("INSERT INTO child VALUES ('orphan', 'missing')", [])
            .expect_err("production connection must enforce the parent reference")
            .sqlite_error()
            .expect("native constraint failure")
            .extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
    );
    connection
        .execute("UPDATE parent SET id='renamed'", [])
        .expect("native update cascade with schema triggers disabled");
    assert_eq!(
        connection
            .query_row("SELECT parent FROM child", [], |row| row
                .get::<_, String>(0))
            .expect("updated child"),
        "renamed"
    );
    connection
        .execute("DELETE FROM parent", [])
        .expect("native delete cascade with schema triggers disabled");
    for table in ["parent", "child", "audit"] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, u32>(0))
                .expect("exact fixture table"),
            0,
            "{table}: cascades execute but ordinary schema triggers do not"
        );
    }
}

#[test]
fn deferred_reference_failure_rolls_back_the_complete_native_transaction() {
    let mut connection = Connection::open_in_memory().expect("disposable provider");
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, false)
        .expect("disabled precondition");
    configure_connection(&connection, options()).expect("production safeguards");
    connection
        .execute_batch(
            "CREATE TABLE parent(id TEXT PRIMARY KEY) STRICT;
         CREATE TABLE child(id TEXT PRIMARY KEY, parent TEXT NOT NULL
             REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED) STRICT;
         INSERT INTO parent VALUES ('previous');
         INSERT INTO child VALUES ('previous-child', 'previous');",
        )
        .expect("prior complete projection fixture");
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("same native transaction mode used by Gazette writes");
    transaction
        .execute_batch(
            "DELETE FROM child; DELETE FROM parent;
         INSERT INTO parent VALUES ('replacement');
         INSERT INTO child VALUES ('replacement-child', 'missing');",
        )
        .expect("deferred constraints permit staging the complete set");
    assert_eq!(
        transaction
            .commit()
            .expect_err("late reference failure must refuse commit")
            .sqlite_error()
            .expect("native constraint error")
            .extended_code,
        rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
    );
    assert!(
        connection.is_autocommit(),
        "failed transaction dropped and rolled back"
    );
    let retained: (String, String, String) = connection
        .query_row(
            "SELECT parent.id, child.id, child.parent FROM parent JOIN child ON child.parent=parent.id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("prior complete data remains usable");
    assert_eq!(
        retained,
        (
            "previous".into(),
            "previous-child".into(),
            "previous".into()
        )
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM parent", [], |row| row
                .get::<_, u32>(0))
            .expect("no replacement parent"),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM child", [], |row| row.get::<_, u32>(0))
            .expect("no replacement child"),
        1
    );
    // A complete replacement can insert a child before its parent; the constraint
    // is enforced at the transaction boundary, not by Gazette's own SQL emulation.
    let transaction = connection
        .transaction()
        .expect("next explicit fixture transaction");
    transaction
        .execute_batch(
            "DELETE FROM child; DELETE FROM parent;
         INSERT INTO child VALUES ('next-child', 'next');
         INSERT INTO parent VALUES ('next');",
        )
        .expect("order-independent complete replacement");
    transaction
        .commit()
        .expect("all references resolve at commit");
    assert_eq!(
        connection
            .query_row("SELECT parent FROM child", [], |row| row
                .get::<_, String>(0))
            .expect("replacement visible"),
        "next"
    );
}

#[test]
fn enabling_enforcement_does_not_repair_or_admit_existing_foreign_rows() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("foreign.db");
    let connection = Connection::open(&path).expect("disposable foreign database");
    connection
        .set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, false)
        .expect("create explicitly inconsistent fixture");
    connection
        .execute_batch(
            "CREATE TABLE parent(id INTEGER PRIMARY KEY);
         CREATE TABLE child(parent INTEGER REFERENCES parent(id));
         INSERT INTO child VALUES (7);",
        )
        .expect("preexisting foreign rows");
    connection.close().expect("close fixture writer");
    let before = fs::read(&path).expect("native baseline");
    let reader = open_existing(&path, options()).expect("production read-only connection");
    assert!(
        reader
            .db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY)
            .expect("enabled")
    );
    assert_eq!(
        reader
            .query_row("PRAGMA foreign_key_check", [], |row| row
                .get::<_, String>(0))
            .expect("violation remains observable"),
        "child"
    );
    reader.close().expect("close reader");
    Sqlite
        .observe(&path, options())
        .expect("raw metadata is not relational admission");
    assert_eq!(fs::read(&path).expect("readback"), before);
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no migration or recovery files")
            .count(),
        1
    );
}
