//! Existing-journal validation uses production initialization, then bounded native contamination.

#[path = "journal_cli/mcp.rs"]
mod mcp;
#[path = "support/mcp.rs"]
mod mcp_carrier;

use gazette_sdk::{
    source::Sha256Digest,
    store::{
        self, InitializationReceipt, InitializeOptions, JournalHeader, ReadOptions, SessionOptions,
    },
};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;

fn read_options() -> ReadOptions {
    ReadOptions {
        allow_coordination_files: true,
        ..ReadOptions::default()
    }
}

fn fixture() -> (TempDir, InitializationReceipt) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable journal");
    let receipt = store::initialize(
        root.path(),
        InitializeOptions {
            allow_create: true,
            session: SessionOptions {
                allow_local_reservation: true,
                read: read_options(),
            },
            ..InitializeOptions::default()
        },
    )
    .expect("production initialization");
    (root, receipt)
}

fn command(path: &Path, expected: Option<&JournalHeader>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-journal")
        .arg(path)
        .arg("--allow-coordination-files");
    if let Some(header) = expected {
        command
            .arg("--expected-header")
            .arg(serde_json::to_string(header).expect("typed expected header"));
    }
    command
}

fn reject(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

fn contaminate(path: &Path, sql: &str) {
    Connection::open(path)
        .expect("test-only native mutation")
        .execute_batch(sql)
        .expect("contamination fixture");
}

#[test]
fn cli_and_sdk_bind_the_same_complete_snapshot_without_changing_the_store() {
    let (root, initial) = fixture();
    let before = fs::read(&initial.database).expect("prior bytes");
    let sdk = store::inspect_journal(&initial.database, Some(&initial.header), read_options())
        .expect("SDK schema/identity read-back");
    assert_eq!(sdk.header, initial.header);
    assert!(sdk.verifier.sql_read_only);
    let expected_schema = [
        include_bytes!("../../../adapters/sqlite/schema/0001-journal.sql").as_slice(),
        include_bytes!("../../../adapters/sqlite/schema/0002-collection-purpose.sql").as_slice(),
        include_bytes!("../../../adapters/sqlite/schema/0003-collection-navigation.sql").as_slice(),
        include_bytes!("../../../adapters/sqlite/schema/0004-project-lifecycle.sql").as_slice(),
        include_bytes!("../../../adapters/sqlite/schema/0005-collection-order.sql").as_slice(),
        include_bytes!("../../../adapters/sqlite/schema/0006-project-subject.sql").as_slice(),
    ]
    .concat();
    assert_eq!(sdk.schema_sha256, Sha256Digest::of(&expected_schema));
    for expected in [None, Some(&initial.header)] {
        let output = command(&initial.database, expected)
            .output()
            .expect("CLI inspection");
        assert!(output.status.success(), "{output:?}");
        let reported: Value = serde_json::from_slice(&output.stdout).expect("JSON projection");
        assert_eq!(
            reported,
            serde_json::to_value(&sdk).expect("same SDK result")
        );
    }
    assert_eq!(fs::read(&initial.database).expect("unchanged"), before);
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no generated files")
            .count(),
        1
    );
}

#[test]
fn every_expected_coordinate_is_checked_without_rebinding_or_repair() {
    let (_root, initial) = fixture();
    let before = fs::read(&initial.database).expect("initial bytes");
    let mut other_id = initial.header;
    other_id.journal_id[0] ^= 1;
    for expected in [
        other_id,
        JournalHeader {
            schema_version: 2,
            ..initial.header
        },
        JournalHeader {
            journal_epoch: 2,
            ..initial.header
        },
        JournalHeader {
            writer_fence: 1,
            ..initial.header
        },
        JournalHeader {
            writer_fence: u64::MAX,
            ..initial.header
        },
    ] {
        let error = store::inspect_journal(&initial.database, Some(&expected), read_options())
            .expect_err("complete binding must match");
        assert_eq!(error.code(), "store_journal_mismatch");
        reject(
            command(&initial.database, Some(&expected))
                .output()
                .expect("CLI refusal"),
            "store_journal_mismatch",
        );
        assert_eq!(fs::read(&initial.database).expect("preserved"), before);
    }
}

#[test]
fn wrong_old_newer_and_partial_storage_markers_are_not_sufficient_authority() {
    for sql in [
        "PRAGMA application_id=0",
        "PRAGMA application_id=17",
        "PRAGMA user_version=0",
        "PRAGMA user_version=-1",
        "PRAGMA user_version=2",
        "DROP TABLE gazette_journal",
    ] {
        let (_root, initial) = fixture();
        contaminate(&initial.database, sql);
        let bytes = fs::read(&initial.database).expect("changed fixture bytes");
        reject(
            command(&initial.database, None)
                .output()
                .expect("unsupported schema"),
            "store_journal_schema",
        );
        assert_eq!(fs::read(&initial.database).expect("no repair"), bytes);
    }
}

#[test]
fn altered_ddl_and_every_extra_schema_object_are_rejected() {
    for sql in [
        "ALTER TABLE gazette_journal ADD COLUMN unexpected TEXT",
        "CREATE TABLE foreign_state(value INTEGER)",
        "CREATE TABLE sqliteXhidden(value INTEGER)",
        "CREATE INDEX extra_index ON gazette_journal(journal_epoch)",
        "CREATE VIEW extra_view AS SELECT * FROM gazette_journal",
        "CREATE TRIGGER extra_trigger AFTER UPDATE ON gazette_journal BEGIN SELECT 1; END",
        "ALTER TABLE gazette_journal RENAME TO old_journal; CREATE VIEW gazette_journal AS SELECT * FROM old_journal",
        "ALTER TABLE gazette_journal RENAME TO old_journal; CREATE TABLE gazette_journal(singleton INTEGER PRIMARY KEY, journal_id BLOB, journal_epoch INTEGER, writer_fence INTEGER); INSERT INTO gazette_journal SELECT * FROM old_journal; DROP TABLE old_journal",
    ] {
        let (_root, initial) = fixture();
        contaminate(&initial.database, sql);
        let bytes = fs::read(&initial.database).expect("contaminated bytes");
        reject(
            command(&initial.database, Some(&initial.header))
                .output()
                .expect("no marker-only acceptance"),
            "store_journal_schema",
        );
        assert_eq!(fs::read(&initial.database).expect("no cleanup"), bytes);
    }
}

#[test]
fn missing_or_constraint_violating_coordinates_fail_read_back() {
    for sql in [
        "DELETE FROM gazette_journal",
        "PRAGMA ignore_check_constraints=ON; UPDATE gazette_journal SET journal_epoch=0",
        "PRAGMA ignore_check_constraints=ON; UPDATE gazette_journal SET writer_fence=-1",
        "PRAGMA ignore_check_constraints=ON; UPDATE gazette_journal SET journal_id=x'00'",
        "PRAGMA ignore_check_constraints=ON; UPDATE gazette_journal SET singleton=2",
        "PRAGMA ignore_check_constraints=ON; INSERT INTO gazette_journal VALUES (2, randomblob(32), 1, 0)",
    ] {
        let (_root, initial) = fixture();
        contaminate(&initial.database, sql);
        let bytes = fs::read(&initial.database).expect("invalid data fixture");
        reject(
            command(&initial.database, None)
                .output()
                .expect("invalid coordinates"),
            "store_journal_verification",
        );
        assert_eq!(fs::read(&initial.database).expect("no data repair"), bytes);
    }
}

#[test]
fn one_wal_snapshot_does_not_mix_committed_and_uncommitted_coordinates() {
    let (_root, initial) = fixture();
    let writer = Connection::open(&initial.database).expect("independent native test writer");
    writer.execute_batch("PRAGMA journal_mode=WAL; UPDATE gazette_journal SET writer_fence=1; BEGIN IMMEDIATE; UPDATE gazette_journal SET journal_epoch=2, writer_fence=2, journal_id=randomblob(32)").expect("uncommitted next coordinates");
    let committed = JournalHeader {
        writer_fence: 1,
        ..initial.header
    };
    let read = store::inspect_journal(&initial.database, Some(&committed), read_options())
        .expect("one committed snapshot");
    assert_eq!(read.header, committed);
    assert_eq!(read.verifier.journal_mode, "wal");
    reject(
        command(&initial.database, Some(&initial.header))
            .output()
            .expect("stale generation refused"),
        "store_journal_mismatch",
    );
    writer
        .execute_batch("ROLLBACK")
        .expect("discard test-only uncommitted update");
    assert_eq!(
        store::inspect_journal(&initial.database, Some(&committed), read_options())
            .expect("same committed coordinates")
            .header,
        committed
    );
}

#[test]
fn backup_of_older_generation_is_detected_only_against_an_independent_newer_binding() {
    let (root, initial) = fixture();
    let parent = tempfile::tempdir().expect("backup fixture parent");
    let roots = vec![root.path().to_path_buf()];
    let backup = store::backup(store::BackupRequest {
        source: &initial.database,
        output_parent: parent.path(),
        protected: store::LifecycleProtectedRoots {
            authored: roots.clone(),
            staged: roots.clone(),
            published: roots.clone(),
            live_journals: roots,
        },
        options: store::BackupOptions {
            allow_create: true,
            read: read_options(),
            ..store::BackupOptions::default()
        },
    })
    .expect("production backup");
    contaminate(
        &initial.database,
        "UPDATE gazette_journal SET writer_fence=1",
    );
    let newer = JournalHeader {
        writer_fence: 1,
        ..initial.header
    };
    let copied = backup.directory.join("store.sqlite");
    reject(
        command(&copied, Some(&newer))
            .output()
            .expect("older generation refusal"),
        "store_journal_mismatch",
    );
    let old = store::inspect_journal(&copied, Some(&initial.header), read_options())
        .expect("old bytes match an old expectation; not anti-rollback");
    assert_eq!(old.header, initial.header);
    assert_eq!(
        store::inspect_journal(&initial.database, Some(&newer), read_options())
            .expect("live newer journal preserved")
            .header,
        newer
    );
}

#[test]
fn bad_expected_header_is_rejected_before_native_access() {
    let (root, initial) = fixture();
    let missing = root.path().join("does-not-exist");
    let header = serde_json::to_string(&initial.header).expect("header JSON");
    let extra = header.replacen('{', "{\"unknown\":1,", 1);
    let duplicate = header.replacen('{', "{\"schema_version\":1,", 1);
    let oversized = format!("{header}{}", " ".repeat(4096));
    for input in ["{}", "null", "[]", "broken", &extra, &duplicate, &oversized] {
        let output = command(&missing, None)
            .arg("--expected-header")
            .arg(input)
            .output()
            .expect("invalid header refusal");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(!missing.exists());
    }
}

#[test]
fn permission_bounds_missing_corrupt_and_linked_input_never_initialize() {
    let (root, initial) = fixture();
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["inspect-journal", "--help"])
        .output()
        .expect("effect help");
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).expect("help text");
    assert!(help.contains("no SQL writes"));
    assert!(help.contains("WAL/shared-memory coordination"));
    assert_eq!(
        command(&initial.database, None)
            .arg("--initialize")
            .output()
            .expect("no hidden initialization")
            .status
            .code(),
        Some(2)
    );
    let bytes = fs::read(&initial.database).expect("original bytes");
    let denied = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-journal")
        .arg(&initial.database)
        .output()
        .expect("no permission");
    reject(denied, "store_coordination_permission");
    for (args, code) in [
        (["--max-vm-steps", "1"], "store_interrupted"),
        (["--max-storage-bytes", "1"], "store_storage_limit"),
    ] {
        reject(
            command(&initial.database, None)
                .args(args)
                .output()
                .expect("bounded refusal"),
            code,
        );
    }
    let missing = root.path().join("missing");
    reject(
        command(&missing, None).output().expect("missing refusal"),
        "store_io",
    );
    assert!(!missing.exists());
    let linked = root.path().join("linked");
    symlink(&initial.database, &linked).expect("linked fixture");
    reject(
        command(&linked, None).output().expect("link refusal"),
        "store_unsafe_path",
    );
    let empty = root.path().join("empty");
    fs::write(&empty, []).expect("empty fixture");
    reject(
        command(&empty, None).output().expect("empty refusal"),
        "store_uninitialized",
    );
    let corrupt = root.path().join("corrupt");
    fs::write(&corrupt, [b'x'; 4096]).expect("corrupt fixture");
    reject(
        command(&corrupt, None).output().expect("corrupt refusal"),
        "store_provider",
    );
    assert_eq!(
        fs::read(corrupt).expect("unchanged corrupt file"),
        [b'x'; 4096]
    );
    assert_eq!(
        fs::read(&initial.database).expect("original preserved"),
        bytes
    );
}
