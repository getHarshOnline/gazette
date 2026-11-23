//! Real SQL transaction continuity; interrupted anchors remain unresolved, never adopted.

use gazette_core::store::LocalCoordinatorTransaction;
use rusqlite::{Connection, TransactionBehavior, params};

use super::*;

pub(super) fn captured(fixture: &Fixture) -> LocalCoordinatorTransaction {
    crate::with_source(&fixture.database(), session().read, |connection| {
        snapshot::capture(connection, session().read)
    })
    .expect("independent complete typed SQL snapshot")
}

fn retained(fixture: &Fixture) -> LocalCoordinatorState {
    // Native fixture observation only: calling the public inspector while this
    // process holds its reservation would correctly refuse the second lifetime.
    serde_json::from_slice(
        &fs::read(fixture.anchor.path().join("coordinator.json"))
            .expect("actual separately persisted anchor bytes"),
    )
    .expect("native typed anchor")
}

fn transaction(state: &LocalCoordinatorState) -> &LocalCoordinatorTransaction {
    let LocalCoordinatorState::InProgress {
        transaction: Some(transaction),
        ..
    } = state
    else {
        panic!("successful native commit must retain its expected transaction: {state:?}");
    };
    transaction
}

#[test]
fn generation_append_and_exact_replay_retain_one_current_expected_sql_state() {
    let fixture = Fixture::new();
    let original = captured(&fixture);
    let mut writer = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
        .expect("actual anchored generation transaction");
    let acquired = retained(&fixture);
    let expected = transaction(&acquired);
    assert_eq!(expected, &captured(&fixture));
    assert_eq!(
        expected.header.writer_fence,
        fixture.seal.header.writer_fence + 1
    );
    assert_ne!(expected.content_sha256, original.content_sha256);
    let first = try_append(&mut writer, b"one actual observation").expect("real append");
    let appended = retained(&fixture);
    let after = transaction(&appended);
    assert_eq!(after, &captured(&fixture));
    assert_eq!(
        after.header, expected.header,
        "same generation, different complete contents"
    );
    assert_ne!(after.content_sha256, expected.content_sha256);
    let path = fixture.anchor.path().join("coordinator.json");
    let bytes = fs::read(&path).expect("original expected transaction encoding");
    let identity = fs::metadata(&path)
        .expect("original atomic anchor inode")
        .ino();
    let replay = try_append(&mut writer, b"one actual observation").expect("exact native replay");
    assert!(first.inserted);
    assert!(!replay.inserted);
    assert_eq!(replay.record, first.record);
    assert_eq!(retained(&fixture), appended);
    assert_eq!(fs::read(&path).expect("no rewritten replay anchor"), bytes);
    assert_eq!(
        fs::metadata(&path)
            .expect("same anchor object on replay")
            .ino(),
        identity
    );
    assert_eq!(
        writer.inspect().expect("held inspection").header,
        after.header
    );
    drop(writer);
    assert_eq!(
        fixture.state(),
        appended,
        "drop does not seal or erase transaction evidence"
    );
    assert_eq!(captured(&fixture), *after);
}

#[test]
fn same_generation_in_place_rollback_is_refused_during_the_held_writer_lifetime() {
    for inspect_first in [false, true] {
        let fixture = Fixture::new();
        let mut writer = fixture
            .connector()
            .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
            .expect("held anchored writer");
        append(&mut writer, b"older committed observation");
        let older = fs::read(fixture.database()).expect("older complete native bytes");
        append(&mut writer, b"later committed observation");
        let latest = retained(&fixture);
        let expected = transaction(&latest).clone();
        let inode = fs::metadata(fixture.database())
            .expect("original database inode")
            .ino();
        fs::write(fixture.database(), &older).expect("controlled same-inode rollback while held");
        assert_eq!(
            fs::metadata(fixture.database())
                .expect("same native file")
                .ino(),
            inode
        );
        assert_eq!(
            fixture.header(),
            expected.header,
            "ordinary header inspection cannot detect this rollback"
        );
        assert_ne!(captured(&fixture).content_sha256, expected.content_sha256);
        if inspect_first {
            assert!(matches!(writer.inspect(), Err(Error::CoordinatorMismatch)));
        } else {
            let error = try_append(&mut writer, b"must not be appended")
                .expect_err("write verifies previous complete snapshot before mutation");
            assert!(matches!(error.cause, Error::CoordinatorMismatch));
        }
        assert_eq!(
            retained(&fixture),
            latest,
            "no old-state fallback or anchor overwrite"
        );
        assert_eq!(fs::read(fixture.database()).expect("no SQL repair"), older);
        assert!(
            writer.close().is_err(),
            "a refused lifetime cannot produce a clean seal"
        );
        assert_eq!(fixture.state(), latest);
        assert_eq!(
            fs::read(fixture.database()).expect("close did not adopt rollback"),
            older
        );
    }
}

#[test]
fn precommit_anchor_can_be_ahead_of_rolled_back_sql_and_never_implies_commit() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.database()).expect("original standalone database");
    let anchor =
        Anchor::acquire(fixture.anchor.path(), session().read).expect("native anchor reservation");
    let clean = anchor.read().expect("original clean anchor");
    let dirty = LocalCoordinatorState::InProgress {
        previous: fixture.seal.clone(),
        reserved_generation: fixture.seal.header.writer_fence + 1,
        transaction: None,
    };
    let observed = anchor
        .write(dirty, Some(&clean))
        .expect("native generation intent");
    let mut guard = Guard { anchor, observed };
    let mut connection =
        Connection::open(fixture.database()).expect("disposable native SQL connection");
    let sql = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .expect("actual uncommitted SQL transaction");
    journal::advance_generation(&sql, fixture.seal.header)
        .expect("actual pending generation update");
    let expected = snapshot::capture(&sql, session().read).expect("actual uncommitted poststate");
    let mut wrong = expected.clone();
    wrong.header.writer_fence += 1;
    assert!(matches!(
        guard.expect_transaction(wrong),
        Err(Error::CoordinatorMismatch)
    ));
    guard
        .expect_transaction(expected.clone())
        .expect("native durable precommit anchor");
    guard
        .verify_transaction(&sql, session().read)
        .expect("pending SQL matches the expected poststate");
    let ahead = retained(&fixture);
    assert_eq!(
        transaction(&ahead),
        &expected,
        "anchor precedes any SQL commit"
    );
    sql.rollback()
        .expect("explicit fixture rollback, not a product recovery operation");
    assert!(matches!(
        guard.verify_transaction(&connection, session().read),
        Err(Error::CoordinatorMismatch)
    ));
    connection.close().expect("release fixture SQL connection");
    assert_eq!(
        fs::read(fixture.database()).expect("rolled-back SQL bytes"),
        before
    );
    assert_eq!(retained(&fixture), ahead);
    drop(guard);
    assert_eq!(fixture.state(), ahead);
    assert!(
        fixture
            .connector()
            .recover_release(
                fixture.journal.path(),
                expected.header,
                gazette_core::store::RecoveryProfile::Unstarted,
                session()
            )
            .is_err(),
        "an ahead-of-commit anchor is not admitted as recovery custody"
    );
    let error = fixture
        .connector()
        .acquire_writer(fixture.journal.path(), fixture.header(), options())
        .err()
        .expect("in-progress anchor is not an adoption recipe");
    assert!(matches!(error.cause, Error::CoordinatorState));
    assert_eq!(
        error.generation,
        gazette_core::store::GenerationEffect::NotAttempted
    );
    assert_eq!(fixture.state(), ahead);
}

#[test]
fn typed_snapshot_ignores_insertion_order_but_preserves_null_empty_text_and_blob_bytes() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.database()).expect("disposable exact native schema");
    // These rows exercise the snapshot's SQL-value contract, not registry/release admission.
    let insert = |id: &str, name: Option<&str>| {
        connection
            .execute(
                "INSERT INTO gazette_registry_principals (id,kind,name) VALUES (?1,'person',?2)",
                params![id, name],
            )
            .expect("typed native principal row");
    };
    insert("principal:z", Some("É exact"));
    insert("principal:a", None);
    let first = snapshot::capture(&connection, session().read).expect("complete ordered snapshot");
    connection
        .execute("DELETE FROM gazette_registry_principals", [])
        .expect("fixture row reorder");
    insert("principal:a", None);
    insert("principal:z", Some("É exact"));
    assert_eq!(
        snapshot::capture(&connection, session().read).expect("same typed rows"),
        first
    );
    connection
        .execute(
            "UPDATE gazette_registry_principals SET name='' WHERE id='principal:a'",
            [],
        )
        .expect("empty text differs from SQL null");
    let empty = snapshot::capture(&connection, session().read).expect("empty text snapshot");
    assert_ne!(empty.content_sha256, first.content_sha256);
    connection
        .execute(
            "UPDATE gazette_registry_principals SET name=NULL WHERE id='principal:a'",
            [],
        )
        .expect("restore exact null");
    assert_eq!(
        snapshot::capture(&connection, session().read).expect("original typed state"),
        first
    );
    let id = Sha256Digest::of(b"opaque blob row");
    for bytes in [b"a\0b\xff".as_slice(), b"a\0b\xfe".as_slice()] {
        connection.execute("INSERT OR REPLACE INTO gazette_ref_observations (id,payload,payload_sha256) VALUES (?1,?2,?3)",
            params![id.as_bytes(), bytes, Sha256Digest::of(bytes).as_bytes()])
            .expect("exact binary SQL values, no UTF-8 repair");
        let current = snapshot::capture(&connection, session().read).expect("typed blob snapshot");
        assert_ne!(current.content_sha256, first.content_sha256);
    }
    let binary = snapshot::capture(&connection, session().read).expect("last exact blob state");
    connection
        .execute(
            "UPDATE gazette_ref_observations SET payload=?1 WHERE id=?2",
            params![b"a\0b\xff".as_slice(), id.as_bytes()],
        )
        .expect("change one retained byte independently of digest column");
    assert_ne!(
        snapshot::capture(&connection, session().read).expect("changed exact SQL state"),
        binary
    );
    assert!(
        connection
            .execute("UPDATE gazette_journal SET writer_fence=1.5", [])
            .is_err(),
        "the admitted STRICT schema has no floating-point journal scalar"
    );
    connection.close().expect("close typed fixture");
}

#[test]
fn snapshot_has_an_exact_complete_encoding_ceiling_and_rejects_changed_schema() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.database()).expect("disposable native schema");
    let expected = snapshot::capture(&connection, session().read).expect("complete baseline");
    let mut low = 0;
    let mut high = session().read.storage_bytes;
    while low < high {
        let middle = low + (high - low) / 2;
        let options = ReadOptions {
            storage_bytes: middle,
            ..session().read
        };
        match snapshot::capture(&connection, options) {
            Ok(value) => {
                assert_eq!(value, expected);
                high = middle;
            }
            Err(Error::StorageLimit) => low = middle + 1,
            Err(error) => panic!("unexpected native snapshot failure: {error:?}"),
        }
    }
    assert!(
        low > 32,
        "schema/table/value framing counts, not merely digest size"
    );
    assert_eq!(
        snapshot::capture(
            &connection,
            ReadOptions {
                storage_bytes: low,
                ..session().read
            }
        )
        .expect("exact full encoding ceiling"),
        expected
    );
    assert!(
        matches!(
            snapshot::capture(
                &connection,
                ReadOptions {
                    storage_bytes: low - 1,
                    ..session().read
                }
            ),
            Err(Error::StorageLimit)
        ),
        "no partial snapshot on one-byte shortage"
    );
    connection
        .execute_batch("CREATE TABLE unrelated (value BLOB) STRICT;")
        .expect("deliberate unsupported fixture schema");
    assert!(matches!(
        snapshot::capture(&connection, session().read),
        Err(Error::JournalSchema)
    ));
    connection
        .close()
        .expect("close changed-schema fixture without repair");
}

#[test]
fn every_trusted_snapshot_scan_uses_native_order_without_a_temporary_payload_sort() {
    let fixture = Fixture::new();
    let connection = Connection::open(fixture.database()).expect("actual native Gazette schema");
    let tables = crate::schema::tables().expect("one schema-owned table inventory");
    assert!(!tables.is_empty());
    for table in tables {
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {}", table.scan))
            .expect("the exact production scan has a native query plan");
        let details = statement
            .query_map([], |row| row.get::<_, String>(3))
            .expect("native query-plan rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("complete native query plan");
        assert!(!details.is_empty(), "{}: no plan evidence", table.name);
        assert!(
            details
                .iter()
                .all(|detail| !detail.to_ascii_uppercase().contains("TEMP B-TREE")),
            "{}: a private-payload temporary sort is not an admitted snapshot scan: {details:?}",
            table.name
        );
        assert!(
            details.iter().any(|detail| detail.starts_with("SCAN ")),
            "{}: expected complete persistent-table scan: {details:?}",
            table.name
        );
        let prepared = connection
            .prepare(&table.scan)
            .expect("same production scan");
        assert_eq!(
            prepared.column_count(),
            table.columns,
            "{}: complete row projection",
            table.name
        );
    }
    connection.close().expect("close query-plan fixture");
}

#[test]
fn interrupted_anchor_requires_transaction_field_and_exact_header_binding() {
    for malformed in ["omitted", "wrong-generation", "positional", "unknown"] {
        let fixture = Fixture::new();
        let writer = fixture
            .connector()
            .acquire_writer(fixture.journal.path(), fixture.seal.header, options())
            .expect("generation reaches complete transaction boundary");
        drop(writer);
        let path = fixture.anchor.path().join("coordinator.json");
        let mut state = fixture.state();
        let original = serde_json::to_string(&state).expect("canonical native state encoding");
        assert_eq!(
            fs::read(&path).expect("valid original anchor"),
            original.as_bytes()
        );
        let text = match malformed {
            "omitted" => {
                let (prefix, _) = original
                    .rsplit_once(",\"transaction\":")
                    .expect("mandatory final field");
                format!("{prefix}}}")
            }
            "wrong-generation" => {
                let LocalCoordinatorState::InProgress {
                    transaction: Some(ref mut expected),
                    ..
                } = state
                else {
                    panic!("original expected transaction");
                };
                expected.header.writer_fence = 0;
                // Keep canonical serialization: this must fail the header binding,
                // not an unrelated object-key ordering comparison.
                serde_json::to_string(&state).expect("canonical wrong header fixture")
            }
            "positional" => {
                let expected = transaction(&state);
                let object = serde_json::to_string(expected).expect("exact original nested object");
                let array = format!(
                    "[{},{}]",
                    serde_json::to_string(&expected.header).expect("native header"),
                    serde_json::to_string(&expected.content_sha256).expect("native digest")
                );
                original.replace(&object, &array)
            }
            "unknown" => {
                let expected = transaction(&state);
                let object = serde_json::to_string(expected).expect("exact original nested object");
                let replacement = format!(
                    "{},\"inferred_commit\":true}}",
                    object.strip_suffix('}').expect("object end")
                );
                original.replace(&object, &replacement)
            }
            _ => unreachable!("closed fixture variants"),
        };
        assert_ne!(text, original);
        let bytes = text.into_bytes();
        fs::write(&path, &bytes).expect("controlled invalid anchor");
        assert!(
            matches!(
                LocalCoordinator::inspect(fixture.anchor.path(), &fixture.seal.binding, session()),
                Err(Error::CoordinatorState)
            ),
            "{malformed}"
        );
        assert_eq!(fs::read(&path).expect("no inferred repair"), bytes);
    }
}
