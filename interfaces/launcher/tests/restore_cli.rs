//! Detached recovery uses real backup bytes and never replaces the newer source store.

use gazette_sdk::{
    source::Sha256Digest,
    store::{self, BackupOptions, BackupReceipt, BackupVerifyOptions, ReadOptions, RestoreOptions},
};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::PermissionsExt,
    },
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::tempdir;

fn options() -> RestoreOptions {
    RestoreOptions {
        allow_prepare: true,
        verification: BackupVerifyOptions {
            read: ReadOptions {
                allow_coordination_files: true,
                ..ReadOptions::default()
            },
            ..BackupVerifyOptions::default()
        },
        ..RestoreOptions::default()
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

fn sdk_restore(
    backup: &BackupReceipt,
    parent: &Path,
    options: RestoreOptions,
) -> Result<store::RestoreReceipt, store::OutputError> {
    store::prepare_restore(store::PrepareRestoreRequest {
        backup: &backup.directory,
        manifest_sha256: backup.manifest_sha256,
        output_parent: parent,
        protected: protected(&backup.directory),
        options,
    })
}

fn fixture(source: &Path, backups: &Path) -> BackupReceipt {
    let writer = Connection::open(source).expect("disposable source");
    writer.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES('before'); PRAGMA user_version=7;").expect("old snapshot");
    let receipt = store::backup(store::BackupRequest {
        source,
        output_parent: backups,
        protected: protected(source.parent().expect("selected store parent")),
        options: BackupOptions {
            allow_create: true,
            read: options().verification.read,
            ..BackupOptions::default()
        },
    })
    .expect("production backup");
    writer
        .execute_batch("UPDATE fixture SET value='newer'; PRAGMA user_version=8;")
        .expect("newer source remains authoritative");
    drop(writer);
    receipt
}

fn command(backup: &Path, parent: &Path, pin: Sha256Digest) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("prepare-store-restore")
        .arg(backup)
        .arg(parent)
        .arg("--manifest-sha256")
        .arg(pin.to_string())
        .arg("--authored-root")
        .arg(backup)
        .arg("--staged-root")
        .arg(backup)
        .arg("--published-root")
        .arg(backup)
        .arg("--live-journal-root")
        .arg(backup)
        .args(["--prepare-restore", "--allow-coordination-files"]);
    command
}

fn reject(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("native failure report");
    assert_eq!(report["code"], code);
    assert!(report.get("directory_unix_bytes").is_some());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

fn check_output(directory: &Path, preparation: &Value, backup: &BackupReceipt) {
    assert_ne!(directory, backup.directory);
    let restored = fs::read(directory.join("store.sqlite")).expect("restored bytes");
    assert_eq!(
        restored,
        fs::read(backup.directory.join("store.sqlite")).expect("pinned bytes")
    );
    assert_eq!(
        preparation["format"],
        "gazette.sqlite-restore-preparation.v1"
    );
    assert_eq!(
        preparation["source_manifest_sha256"],
        backup.manifest_sha256.to_string()
    );
    assert_eq!(
        preparation["sha256"],
        Sha256Digest::of(&restored).to_string()
    );
    assert_eq!(preparation["bytes"], restored.len());
    assert_eq!(preparation["database"], "store.sqlite");
    let persisted: Value =
        serde_json::from_slice(&fs::read(directory.join("restore.json")).expect("receipt"))
            .expect("typed output");
    assert_eq!(&persisted, preparation);
    assert_eq!(fs::read_dir(directory).expect("exact inventory").count(), 2);
    assert_eq!(
        fs::metadata(directory)
            .expect("directory mode")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    for name in ["store.sqlite", "restore.json"] {
        assert_eq!(
            fs::metadata(directory.join(name))
                .expect("file mode")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let native = Connection::open_with_flags(
        directory.join("store.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("independent native connection");
    let value: String = native
        .query_row("SELECT value FROM fixture", [], |row| row.get(0))
        .expect("restored row");
    let version: i32 = native
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("restored header");
    assert_eq!(value, "before");
    assert_eq!(version, 7);
}

#[test]
fn executable_prepares_exact_backup_bytes_and_leaves_newer_source_untouched() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery outputs");
    let source = root.path().join("live.db");
    let backup = fixture(&source, backups.path());
    let before = fs::read(&source).expect("newer store");
    let original_manifest =
        fs::read(backup.directory.join("backup.json")).expect("original receipt");
    let output = command(&backup.directory, recoveries.path(), backup.manifest_sha256)
        .output()
        .expect("Gazette");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
    let directory = Path::new(receipt["directory"].as_str().expect("private locator"));
    check_output(directory, &receipt["preparation"], &backup);
    assert_eq!(
        receipt["receipt_sha256"],
        Sha256Digest::of(&fs::read(directory.join("restore.json")).expect("manifest bytes"))
            .to_string()
    );
    assert_eq!(fs::read(&source).expect("newer source unchanged"), before);
    assert_eq!(
        fs::read(backup.directory.join("backup.json")).expect("original receipt unchanged"),
        original_manifest
    );
    let current = Connection::open_with_flags(&source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("newer source read");
    let value: String = current
        .query_row("SELECT value FROM fixture", [], |row| row.get(0))
        .expect("current value");
    assert_eq!(value, "newer");
}

#[test]
fn sdk_repeat_creates_distinct_candidates_and_does_not_treat_one_as_a_backup() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery outputs");
    let backup = fixture(&root.path().join("live.db"), backups.path());
    let first = sdk_restore(&backup, recoveries.path(), options()).expect("first candidate");
    let first_bytes = fs::read(first.directory.join("store.sqlite")).expect("first copy");
    let second = sdk_restore(&backup, recoveries.path(), options()).expect("second candidate");
    assert_ne!(first.directory, second.directory);
    check_output(
        &first.directory,
        &serde_json::to_value(&first.preparation).expect("first receipt"),
        &backup,
    );
    check_output(
        &second.directory,
        &serde_json::to_value(&second.preparation).expect("second receipt"),
        &backup,
    );
    assert_eq!(
        fs::read(first.directory.join("store.sqlite")).expect("first unchanged"),
        first_bytes
    );
    assert!(
        store::verify_backup(
            &first.directory,
            first.receipt_sha256,
            options().verification
        )
        .is_err(),
        "restore evidence cannot impersonate an original backup manifest"
    );
}

#[test]
fn preconditions_fail_without_creating_or_changing_any_store() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery outputs");
    let backup = fixture(&root.path().join("live.db"), backups.path());
    for (requested, code) in [
        (RestoreOptions::default(), "store_restore_permission"),
        (
            RestoreOptions {
                allow_prepare: true,
                ..RestoreOptions::default()
            },
            "store_coordination_permission",
        ),
    ] {
        let error = sdk_restore(&backup, recoveries.path(), requested).expect_err("permission");
        assert_eq!(error.cause.code(), code);
        assert!(error.directory.is_none());
    }
    reject(
        command(
            &backup.directory,
            recoveries.path(),
            Sha256Digest::of(b"wrong pin"),
        )
        .output()
        .expect("bad pin"),
        "store_backup_verification",
    );
    reject(
        command(&backup.directory, recoveries.path(), backup.manifest_sha256)
            .args(["--max-storage-bytes", "1"])
            .output()
            .expect("bound"),
        "store_backup_limit",
    );
    let overlap =
        sdk_restore(&backup, &backup.directory, options()).expect_err("source/output overlap");
    assert_eq!(overlap.cause.code(), "store_unsafe_path");
    assert!(overlap.directory.is_none());
    assert_eq!(
        fs::read_dir(&backup.directory)
            .expect("original inventory")
            .count(),
        2
    );
    assert!(
        fs::read_dir(recoveries.path())
            .expect("no output")
            .next()
            .is_none()
    );
    fs::write(backup.directory.join("store.sqlite"), b"corrupt backup")
        .expect("fixture corruption");
    reject(
        command(&backup.directory, recoveries.path(), backup.manifest_sha256)
            .output()
            .expect("bad backup"),
        "store_backup_verification",
    );
    assert!(
        fs::read_dir(recoveries.path())
            .expect("still no output")
            .next()
            .is_none()
    );
}

#[test]
fn lost_receipt_delivery_preserves_detached_bytes_but_returns_failure() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery outputs");
    let backup = fixture(&root.path().join("live.db"), backups.path());
    let mut child = command(&backup.directory, recoveries.path(), backup.manifest_sha256)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("terminal process");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("receipt delivery failed"));
    let retained: Value = serde_json::from_slice(
        output
            .stderr
            .split(|&b| b == b'\n')
            .next()
            .expect("retained original report"),
    )
    .expect("complete original receipt on stderr");
    let directories: Vec<_> = fs::read_dir(recoveries.path())
        .expect("retained output")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert_eq!(directories.len(), 1);
    let preparation: Value =
        serde_json::from_slice(&fs::read(directories[0].join("restore.json")).expect("manifest"))
            .expect("preparation");
    check_output(&directories[0], &preparation, &backup);
    assert_eq!(
        retained["receipt_sha256"],
        Sha256Digest::of(&fs::read(directories[0].join("restore.json")).expect("native receipt"))
            .to_string()
    );
}

#[test]
fn invocation_never_accepts_live_activation_or_overwrite_flags() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery outputs");
    let backup = fixture(&root.path().join("live.db"), backups.path());
    for flag in ["--activate", "--overwrite", "--migrate"] {
        let output = command(&backup.directory, recoveries.path(), backup.manifest_sha256)
            .arg(flag)
            .output()
            .expect("usage refusal");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
    }
    let help = command(&backup.directory, recoveries.path(), backup.manifest_sha256)
        .arg("--help")
        .output()
        .expect("help");
    assert!(
        String::from_utf8_lossy(&help.stdout)
            .contains("no replacement, migration, activation or release")
    );
    assert!(
        fs::read_dir(recoveries.path())
            .expect("no output")
            .next()
            .is_none()
    );
}

#[test]
fn detached_response_budget_is_reserved_before_creation_and_fallback_preserves_receipt_pin() {
    let root = tempdir().expect("source");
    let backups = tempdir().expect("backups");
    let recoveries = tempdir().expect("recovery parent");
    let backup = fixture(&root.path().join("live.db"), backups.path());
    let refused = command(&backup.directory, recoveries.path(), backup.manifest_sha256)
        .args(["--max-output-bytes", "1"])
        .output()
        .expect("pre-effect budget refusal");
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert_eq!(
        fs::read_dir(recoveries.path())
            .expect("no copy created")
            .count(),
        0
    );
    let parent = recoveries
        .path()
        // Leave native SQLite pathname/sidecar headroom while forcing JSON escaping.
        .join("\u{1}".repeat(150))
        .join("\u{2}".repeat(150));
    fs::create_dir_all(&parent).expect("escape-heavy valid destination parent");
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("private parent");
    let limits = store::OutputLimits {
        directory_bytes: parent
            .canonicalize()
            .expect("native parent")
            .join("gazette-restore-XXXXXX")
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
    let output = command(&backup.directory, &parent, backup.manifest_sha256)
        .args([
            "--max-output-directory-bytes",
            &directory_limit,
            "--max-output-bytes",
            &response_limit,
        ])
        .output()
        .expect("single detached copy");
    assert!(!output.status.success());
    assert!(output.stdout.len() <= maximum);
    let fallback: Value = serde_json::from_slice(&output.stdout).expect("complete fallback");
    assert_eq!(fallback["code"], "store_output_response_unavailable");
    assert_eq!(fallback["evidence"]["kind"], "restore");
    let bytes: Vec<u8> = serde_json::from_value(fallback["directory_unix_bytes"].clone())
        .expect("exact native locator");
    let directory = std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes));
    assert!(directory.starts_with(parent.canonicalize().expect("canonical parent")));
    let receipt = fs::read(directory.join("restore.json")).expect("retained preparation receipt");
    assert_eq!(
        fallback["evidence"]["receipt_sha256"],
        Sha256Digest::of(&receipt).to_string()
    );
    check_output(
        &directory,
        &serde_json::from_slice(&receipt).expect("native preparation"),
        &backup,
    );
    assert_eq!(
        fs::read_dir(&parent)
            .expect("one copy, no automatic retry")
            .count(),
        1
    );
}
