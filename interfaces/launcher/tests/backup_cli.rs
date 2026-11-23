//! Real `SQLite` copies, immutable backup evidence and direct executable behavior.

use gazette_sdk::{
    source::Sha256Digest,
    store::{self, BackupOptions, Error, ReadOptions},
};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{PermissionsExt, symlink},
    },
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::tempdir;

fn options() -> BackupOptions {
    BackupOptions {
        allow_create: true,
        read: ReadOptions {
            allow_coordination_files: true,
            ..ReadOptions::default()
        },
        ..BackupOptions::default()
    }
}

fn protected(root: &Path) -> store::LifecycleProtectedRoots {
    let roots = vec![root.to_path_buf()];
    store::LifecycleProtectedRoots {
        authored: roots.clone(),
        staged: roots.clone(),
        published: roots.clone(),
        live_journals: roots,
    }
}

fn sdk_backup(
    source: &Path,
    parent: &Path,
    options: BackupOptions,
) -> Result<store::BackupReceipt, store::OutputError> {
    store::backup(store::BackupRequest {
        source,
        output_parent: parent,
        protected: protected(source.parent().expect("selected store parent")),
        options,
    })
}

fn fixture(path: &Path) -> Connection {
    let connection = Connection::open(path).expect("disposable fixture");
    connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES ('committed'); PRAGMA application_id=17; PRAGMA user_version=77;").expect("fixture baseline");
    connection
}

fn run(source: &Path, parent: &Path, flags: &[&str]) -> Output {
    let root = source.parent().expect("selected store parent");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("backup-store")
        .arg(source)
        .arg(parent)
        .arg("--authored-root")
        .arg(root)
        .arg("--staged-root")
        .arg(root)
        .arg("--published-root")
        .arg(root)
        .arg("--live-journal-root")
        .arg(root)
        .args(flags)
        .output()
        .expect("Gazette")
}

fn check_backup(directory: &Path, snapshot: &Value, expected_version: i32, expected_row: &str) {
    let path = directory.join("store.sqlite");
    let bytes = fs::read(&path).expect("backup bytes");
    assert_eq!(snapshot["format"], "gazette.sqlite-backup.v1");
    assert_eq!(snapshot["database"], "store.sqlite");
    assert_eq!(snapshot["bytes"], bytes.len());
    assert_eq!(snapshot["sha256"], Sha256Digest::of(&bytes).to_string());
    let manifest: Value =
        serde_json::from_slice(&fs::read(directory.join("backup.json")).expect("receipt"))
            .expect("portable evidence");
    assert_eq!(&manifest, snapshot);
    assert_eq!(snapshot["source"]["user_version"], expected_version);
    assert_eq!(
        fs::metadata(directory)
            .expect("directory")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for file in ["store.sqlite", "backup.json"] {
        assert_eq!(
            fs::metadata(directory.join(file))
                .expect("private file")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::read_dir(directory).expect("standalone files").count(),
        2
    );
    let copied = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("independent native read");
    let version: i32 = copied
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("copied version");
    let value: String = copied
        .query_row("SELECT value FROM fixture", [], |row| row.get(0))
        .expect("copied row");
    assert_eq!(version, expected_version);
    assert_eq!(value, expected_row);
    let integrity: String = copied
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .expect("native integrity");
    assert_eq!(integrity, "ok");
}

#[test]
fn sdk_copies_committed_wal_not_abandoned_writes_and_never_replaces_prior_backup() {
    let root = tempdir().expect("source root");
    let backups = tempdir().expect("private backup root");
    let source = root.path().join("live.db");
    let writer = fixture(&source);
    writer
        .execute_batch(
            "BEGIN IMMEDIATE; UPDATE fixture SET value='abandoned'; PRAGMA user_version=99;",
        )
        .expect("uncommitted change");
    let main_before = fs::read(&source).expect("main");
    let wal_before = fs::read(root.path().join("live.db-wal")).expect("WAL");
    let first = sdk_backup(&source, backups.path(), options()).expect("consistent copy");
    check_backup(
        &first.directory,
        &serde_json::to_value(&first.snapshot).expect("snapshot"),
        77,
        "committed",
    );
    assert_eq!(fs::read(&source).expect("source unchanged"), main_before);
    assert_eq!(
        fs::read(root.path().join("live.db-wal")).expect("WAL unchanged"),
        wal_before
    );
    writer
        .execute_batch("ROLLBACK; UPDATE fixture SET value='later'; PRAGMA user_version=88;")
        .expect("later commit");
    let second = sdk_backup(&source, backups.path(), options()).expect("fresh later copy");
    assert_ne!(first.directory, second.directory);
    check_backup(
        &second.directory,
        &serde_json::to_value(&second.snapshot).expect("snapshot"),
        88,
        "later",
    );
    check_backup(
        &first.directory,
        &serde_json::to_value(&first.snapshot).expect("snapshot"),
        77,
        "committed",
    );
}

#[test]
fn executable_delivers_the_same_snapshot_as_its_durable_manifest() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let source = root.path().join("live.db");
    let _writer = fixture(&source);
    let output = run(
        &source,
        backups.path(),
        &["--create-backup", "--allow-coordination-files"],
    );
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).expect("complete receipt");
    assert!(output.stderr.is_empty(), "{output:?}");
    let directory = Path::new(
        result["directory"]
            .as_str()
            .expect("private output locator"),
    );
    assert!(directory.starts_with(backups.path().canonicalize().expect("backup root")));
    check_backup(directory, &result["snapshot"], 77, "committed");
    assert_eq!(
        result["manifest_sha256"],
        Sha256Digest::of(&fs::read(directory.join("backup.json")).expect("exact manifest"))
            .to_string()
    );
    let help = run(&source, backups.path(), &["--help"]);
    assert!(
        String::from_utf8_lossy(&help.stdout)
            .contains("no overwrite, migration, restore or release")
    );
}

#[test]
fn permission_invalid_source_and_preflight_bounds_create_no_backup() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let source = root.path().join("live.db");
    for flags in [
        vec![],
        vec!["--create-backup"],
        vec!["--create-backup", "--allow-coordination-files"],
    ] {
        let output = run(&source, backups.path(), &flags);
        assert_eq!(output.status.code(), Some(1));
        let report: Value =
            serde_json::from_slice(&output.stdout).expect("complete failure report");
        assert_eq!(report["directory_unix_bytes"], Value::Null);
        assert_eq!(fs::read_dir(backups.path()).expect("no output").count(), 0);
        assert!(!source.exists());
    }
    let _writer = fixture(&source);
    let err = sdk_backup(
        &source,
        backups.path(),
        BackupOptions {
            database_bytes: 1,
            ..options()
        },
    )
    .expect_err("preflight bound");
    assert!(matches!(err.cause, Error::BackupLimit));
    assert!(err.directory.is_none());
    for flag in [
        "--max-backup-bytes",
        "--pages-per-step",
        "--max-backup-steps",
    ] {
        let output = run(
            &source,
            backups.path(),
            &["--create-backup", "--allow-coordination-files", flag, "0"],
        );
        assert_eq!(output.status.code(), Some(1), "{flag}: {output:?}");
        let report: Value =
            serde_json::from_slice(&output.stdout).expect("complete failure report");
        assert_eq!(report["code"], "store_invalid_limits");
        assert_eq!(report["directory_unix_bytes"], Value::Null);
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("store_invalid_limits"), "{error}");
        assert!(error.contains("no new store output created"), "{error}");
    }
    let link = root.path().join("link.db");
    symlink(&source, &link).expect("fixture link");
    let err = sdk_backup(&link, backups.path(), options()).expect_err("source link refusal");
    assert!(matches!(err.cause, Error::UnsafePath));
    assert!(err.directory.is_none());
    assert_eq!(fs::read_dir(backups.path()).expect("no output").count(), 0);
}

#[test]
fn failed_copy_reports_residue_without_receipt_and_never_reuses_it() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let source = root.path().join("live.db");
    let writer = fixture(&source);
    writer
        .execute_batch("CREATE TABLE large(value BLOB); INSERT INTO large VALUES(zeroblob(65536));")
        .expect("multi-page fixture");
    let err = sdk_backup(
        &source,
        backups.path(),
        BackupOptions {
            pages_per_step: 1,
            steps: 1,
            ..options()
        },
    )
    .expect_err("unfinished copy");
    assert!(matches!(err.cause, Error::BackupLimit));
    let residue = err.directory.expect("reported incomplete location");
    assert!(residue.join("store.sqlite").exists());
    assert!(!residue.join("backup.json").exists());
    let retained = fs::read(residue.join("store.sqlite")).expect("residue");
    let complete =
        sdk_backup(&source, backups.path(), options()).expect("independent later backup");
    assert_ne!(complete.directory, residue);
    assert_eq!(
        fs::read(residue.join("store.sqlite")).expect("retained untouched"),
        retained
    );
    assert!(!residue.join("backup.json").exists());
}

#[test]
fn broken_receipt_pipe_is_failure_but_does_not_erase_the_completed_backup() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let source = root.path().join("live.db");
    let _writer = fixture(&source);
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("backup-store")
        .arg(&source)
        .arg(backups.path())
        .arg("--authored-root")
        .arg(root.path())
        .arg("--staged-root")
        .arg(root.path())
        .arg("--published-root")
        .arg(root.path())
        .arg("--live-journal-root")
        .arg(root.path())
        .args(["--create-backup", "--allow-coordination-files"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("terminal backup process");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("receipt delivery failed"));
    let entries: Vec<_> = fs::read_dir(backups.path())
        .expect("output")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(entries.len(), 1);
    let manifest: Value = serde_json::from_slice(
        &fs::read(entries[0].join("backup.json")).expect("completed receipt"),
    )
    .expect("manifest");
    check_backup(&entries[0], &manifest, 77, "committed");
    let delivered: Value = serde_json::from_slice(
        output
            .stderr
            .split(|&b| b == b'\n')
            .next()
            .expect("retained report"),
    )
    .expect("exact receipt on surviving stderr");
    assert_eq!(
        delivered["manifest_sha256"],
        Sha256Digest::of(&fs::read(entries[0].join("backup.json")).expect("native manifest"))
            .to_string()
    );
}

#[test]
fn bounded_backup_delivery_preserves_original_pin_and_failed_copy_locator() {
    let root = tempdir().expect("source root");
    let source = root.path().join("live.db");
    let writer = fixture(&source);
    let backups = tempdir().expect("backup parent");
    let refused = run(
        &source,
        backups.path(),
        &[
            "--create-backup",
            "--allow-coordination-files",
            "--max-output-bytes",
            "1",
        ],
    );
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert_eq!(fs::read_dir(backups.path()).expect("no output").count(), 0);
    let parent = backups
        .path()
        .join("\u{1}".repeat(200))
        .join("\u{2}".repeat(200));
    fs::create_dir_all(&parent).expect("escape-heavy valid parent");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("private parent");
    let limits = store::OutputLimits {
        directory_bytes: parent
            .canonicalize()
            .expect("native parent")
            .join("gazette-backup-XXXXXX")
            .as_os_str()
            .as_bytes()
            .len(),
    };
    let maximum = (1..8192)
        .find(|&maximum| store::output::Reservation::new(limits, maximum).is_ok())
        .expect("minimum complete fallback reservation")
        + 1;
    let directory_limit = limits.directory_bytes.to_string();
    let response_limit = maximum.to_string();
    let output = run(
        &source,
        &parent,
        &[
            "--create-backup",
            "--allow-coordination-files",
            "--max-output-directory-bytes",
            &directory_limit,
            "--max-output-bytes",
            &response_limit,
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.len() <= maximum);
    let fallback: Value = serde_json::from_slice(&output.stdout).expect("bounded fallback");
    assert_eq!(fallback["code"], "store_output_response_unavailable");
    assert_eq!(fallback["evidence"]["kind"], "backup");
    let bytes: Vec<u8> = serde_json::from_value(fallback["directory_unix_bytes"].clone())
        .expect("exact locator bytes");
    let directory = std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes));
    assert!(directory.starts_with(parent.canonicalize().expect("canonical parent")));
    let pin: Sha256Digest = serde_json::from_value(fallback["evidence"]["manifest_sha256"].clone())
        .expect("original independent pin");
    store::verify_backup(
        &directory,
        pin,
        store::BackupVerifyOptions {
            read: options().read,
            ..Default::default()
        },
    )
    .expect("fallback pin verifies exact native backup");
    assert_eq!(
        fs::read_dir(&parent).expect("one native operation").count(),
        1
    );
    writer
        .execute_batch("CREATE TABLE large(value BLOB); INSERT INTO large VALUES(zeroblob(65536));")
        .expect("multi-page database");
    let failed = run(
        &source,
        &parent,
        &[
            "--create-backup",
            "--allow-coordination-files",
            "--pages-per-step",
            "1",
            "--max-backup-steps",
            "1",
        ],
    );
    assert!(!failed.status.success());
    let report: Value = serde_json::from_slice(&failed.stdout).expect("failed native output");
    assert_eq!(report["code"], "store_backup_limit");
    let bytes: Vec<u8> = serde_json::from_value(report["directory_unix_bytes"].clone())
        .expect("failed exact locator");
    let residue = std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes));
    assert_ne!(residue, directory);
    assert!(residue.join("store.sqlite").exists());
    assert!(!residue.join("backup.json").exists());
    assert!(residue.starts_with(parent.canonicalize().expect("exact native parent")));
    assert_eq!(
        fs::read_dir(&parent)
            .expect("one failed copy, not retries")
            .count(),
        2
    );
}
