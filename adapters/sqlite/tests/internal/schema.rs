//! Real initial migration and failure boundaries, without invented historical versions.

use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

#[test]
fn initial_migration_creates_one_origin_and_reinspection_preserves_it() {
    let mut connection = Connection::open_in_memory().expect("disposable database");
    initialize(&mut connection).expect("production initial migration");
    let first = journal::inspect(&connection).expect("production verification");
    assert_eq!(first.header.schema_version, VERSION);
    assert_eq!(first.header.writer_fence, 0);
    assert_eq!(first.header.journal_epoch, 1);
    // Inspection never routes through migration and cannot mint a
    // different journal identity. The fresh-only entry also rejects this store.
    assert!(matches!(
        initialize(&mut connection),
        Err(Error::JournalSchema)
    ));
    assert_eq!(
        journal::inspect(&connection)
            .expect("still supported")
            .header,
        first.header
    );
}

#[test]
fn native_failure_after_ddl_and_after_origin_rolls_back_the_whole_migration() {
    for reject_version in [false, true] {
        let mut connection = Connection::open_in_memory().expect("disposable database");
        connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                let reject = match context.action {
                    AuthAction::Insert { table_name } => {
                        !reject_version && table_name == "gazette_journal"
                    }
                    AuthAction::Pragma {
                        pragma_name,
                        pragma_value,
                    } => reject_version && pragma_name == "user_version" && pragma_value.is_some(),
                    _ => false,
                };
                if reject {
                    Authorization::Deny
                } else {
                    Authorization::Allow
                }
            }))
            .expect("native fault at exact migration boundary");
        assert!(matches!(
            initialize(&mut connection),
            Err(Error::Provider { .. })
        ));
        connection
            .authorizer(None::<fn(AuthContext<'_>) -> Authorization>)
            .expect("remove test fault");
        let (version, application): (i32, i32) = (
            connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .expect("uncommitted schema version"),
            connection
                .pragma_query_value(None, "application_id", |row| row.get(0))
                .expect("uncommitted application identifier"),
        );
        assert_eq!((version, application), (0, 0));
        assert_eq!(
            connection
                .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row
                    .get::<_, u32>(0))
                .expect("rolled-back schema catalog"),
            0
        );
        initialize(&mut connection).expect("explicit fresh retry after rolled-back test fault");
        journal::inspect(&connection).expect("one complete supported origin");
    }
}

#[test]
fn unversioned_foreign_and_newer_schemas_are_not_adopted_or_downgraded() {
    for sql in [
        "CREATE TABLE foreign_data(value TEXT); INSERT INTO foreign_data VALUES ('retain');",
        "PRAGMA application_id=42;",
        "PRAGMA user_version=2147483647;",
        "PRAGMA user_version=2;",
        "PRAGMA user_version=-1;",
    ] {
        let mut connection = Connection::open_in_memory().expect("disposable database");
        connection
            .execute_batch(sql)
            .expect("foreign or unsupported fixture");
        let before = crate::read_metadata(&connection).expect("metadata");
        assert!(matches!(
            initialize(&mut connection),
            Err(Error::JournalSchema)
        ));
        let after = crate::read_metadata(&connection).expect("unchanged metadata");
        assert_eq!(
            (after.user_version, after.application_id),
            (before.user_version, before.application_id)
        );
        assert!(matches!(
            journal::inspect(&connection),
            Err(Error::JournalSchema)
        ));
    }
}

#[test]
fn earlier_native_journals_are_refused_without_altering_their_schema_or_origin() {
    for version in [1, 2, 3, 4] {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(INITIAL).unwrap();
        if version >= 2 {
            connection.execute_batch(COLLECTION_PURPOSE).unwrap();
        }
        if version >= 3 {
            connection.execute_batch(COLLECTION_NAVIGATION).unwrap();
        }
        if version >= 4 {
            connection.execute_batch(PROJECT_LIFECYCLE).unwrap();
        }
        journal::seed(&connection).unwrap();
        connection
            .pragma_update(None, "application_id", APPLICATION_ID)
            .unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        let before = catalog(&connection, 1000).unwrap();
        let read_origin = |connection: &Connection| {
            connection.query_row(
                "SELECT journal_id, journal_epoch, writer_fence FROM gazette_journal WHERE singleton=1",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?)),
            ).unwrap()
        };
        let origin = read_origin(&connection);
        let metadata = crate::read_metadata(&connection).unwrap();
        assert!(matches!(
            initialize(&mut connection),
            Err(Error::JournalSchema)
        ));
        assert!(matches!(
            journal::inspect(&connection),
            Err(Error::JournalSchema)
        ));
        assert_eq!(catalog(&connection, 1000).unwrap(), before);
        assert_eq!(read_origin(&connection), origin);
        let after = crate::read_metadata(&connection).unwrap();
        assert_eq!(
            (after.application_id, after.user_version),
            (metadata.application_id, metadata.user_version)
        );
    }
}

#[test]
fn fresh_registry_accepts_only_the_five_project_states() {
    let mut connection = Connection::open_in_memory().unwrap();
    initialize(&mut connection).unwrap();
    for state in ["planned", "active", "paused", "done", "closed"] {
        connection.execute("INSERT INTO gazette_registry_projects (id,lifecycle,visibility,availability) VALUES (?1,?1,'public','gated')", [state]).unwrap();
    }
    for state in ["developing", "dormant", "historical", "retired"] {
        assert!(connection.execute("INSERT INTO gazette_registry_projects (id,lifecycle,visibility,availability) VALUES (?1,?1,'public','gated')", [state]).is_err());
    }
}
