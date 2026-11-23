//! Fresh journal creation through the real CLI/SDK; no fixture-seeded Gazette schema.

use gazette_sdk::store::{self, InitializeOptions, ReadOptions, SessionOptions};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use std::{
    fs,
    os::{
        fd::OwnedFd,
        unix::{
            ffi::OsStrExt,
            fs::{PermissionsExt, symlink},
            net::UnixStream,
        },
    },
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

fn private_directory() -> TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private fixture")
}

fn options() -> InitializeOptions {
    InitializeOptions {
        allow_create: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: ReadOptions {
                allow_coordination_files: true,
                ..ReadOptions::default()
            },
        },
        ..InitializeOptions::default()
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

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command.arg("initialize-store").arg(root).args([
        "--create-store",
        "--reserve-local-coordination",
        "--allow-coordination-files",
    ]);
    command
}

fn reject(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let failure: Value = serde_json::from_slice(&output.stdout).expect("complete native failure");
    assert_eq!(failure["code"], code);
    assert!(failure.get("directory_unix_bytes").is_some());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

#[test]
fn committed_origin_survives_process_exit_and_is_read_back_independently() {
    let root = private_directory();
    let output = command(root.path()).output().expect("initialize process");
    assert!(output.status.success(), "{output:?}");
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("receipt");
    let header: store::JournalHeader =
        serde_json::from_value(receipt["header"].clone()).expect("typed header");
    assert_eq!(header.schema_version, 6);
    assert_eq!(header.journal_epoch, 1);
    assert_eq!(header.writer_fence, 0);
    assert_ne!(header.journal_id, [0; 32]);
    assert_eq!(receipt["verifier"]["sql_read_only"], true);
    let path = root.path().join("store.sqlite");
    assert_eq!(fs::read_dir(root.path()).expect("inventory").count(), 1);
    assert_eq!(
        fs::metadata(&path)
            .expect("private file")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let native = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("independent reader");
    let observed: ([u8; 32], i64, i64) = native
        .query_row(
            "SELECT journal_id, journal_epoch, writer_fence FROM gazette_journal WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("persisted coordinates");
    assert_eq!(observed, (header.journal_id, 1, 0));
    assert_eq!(
        native
            .query_row("PRAGMA integrity_check", [], |row| row.get::<_, String>(0))
            .expect("integrity"),
        "ok"
    );
    drop(native);
    let mut session =
        store::connect(&path, root.path(), options().session).expect("direct existing connection");
    assert_eq!(session.observe().expect("metadata").user_version, 6);
    session.close().expect("explicit close");
    let second = private_directory();
    let second = store::initialize(second.path(), options()).expect("SDK origin");
    assert_ne!(second.header.journal_id, header.journal_id);
}

#[test]
fn permissions_are_checked_before_creating_any_output() {
    let root = private_directory();
    for (args, code) in [
        (vec![], "store_initialize_permission"),
        (vec!["--create-store"], "store_reservation_permission"),
        (
            vec!["--create-store", "--reserve-local-coordination"],
            "store_coordination_permission",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("initialize-store")
            .arg(root.path())
            .args(args)
            .output()
            .expect("permission check");
        reject(output, code);
        assert_eq!(fs::read_dir(root.path()).expect("untouched").count(), 0);
    }
    let invalid = InitializeOptions {
        database_bytes: 0,
        ..options()
    };
    let error = store::initialize(root.path(), invalid).expect_err("invalid limit");
    assert!(error.directory.is_none());
    assert_eq!(error.cause.code(), "store_invalid_limits");
    assert_eq!(fs::read_dir(root.path()).expect("untouched").count(), 0);
}

#[test]
fn existing_foreign_partial_and_initialized_state_are_never_reused() {
    let root = private_directory();
    let foreign = root.path().join("owner.txt");
    fs::write(&foreign, "keep").expect("foreign fixture");
    reject(
        command(root.path()).output().expect("foreign refusal"),
        "store_exists",
    );
    assert_eq!(fs::read(&foreign).expect("preserved"), b"keep");
    let partial = private_directory();
    fs::write(partial.path().join("store.sqlite"), []).expect("partial fixture");
    reject(
        command(partial.path()).output().expect("partial refusal"),
        "store_exists",
    );
    assert_eq!(
        fs::metadata(partial.path().join("store.sqlite"))
            .expect("retained")
            .len(),
        0
    );
    let initialized = private_directory();
    let first = store::initialize(initialized.path(), options()).expect("fresh");
    let bytes = fs::read(&first.database).expect("prior bytes");
    reject(
        command(initialized.path()).output().expect("no overwrite"),
        "store_exists",
    );
    assert_eq!(fs::read(&first.database).expect("preserved"), bytes);
}

#[test]
fn unsafe_missing_and_contended_namespaces_fail_without_creation() {
    let parent = private_directory();
    let missing = parent.path().join("missing");
    reject(
        command(&missing).output().expect("missing refusal"),
        "store_io",
    );
    assert!(!missing.exists());
    let root = private_directory();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755))
        .expect("nonprivate fixture");
    reject(
        command(root.path()).output().expect("private required"),
        "store_unsafe_path",
    );
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
        .expect("restore fixture mode");
    let alias = parent.path().join("alias");
    symlink(root.path(), &alias).expect("symlink fixture");
    assert!(
        !command(&alias)
            .output()
            .expect("symlink refusal")
            .status
            .success()
    );
    assert_eq!(fs::read_dir(root.path()).expect("untouched").count(), 0);
    let source = parent.path().join("source.sqlite");
    Connection::open(&source)
        .expect("foreign test DB")
        .execute_batch("CREATE TABLE fixture(x)")
        .expect("fixture schema");
    let session =
        store::connect(&source, root.path(), options().session).expect("hold reservation");
    reject(
        command(root.path())
            .output()
            .expect("cross-process contention"),
        "store_reservation_busy",
    );
    assert_eq!(fs::read_dir(root.path()).expect("untouched").count(), 0);
    session.close().expect("release");
    store::initialize(root.path(), options()).expect("explicit retry after release");
}

#[test]
fn interrupted_and_space_limited_initializations_retain_unverified_output() {
    for args in [["--max-vm-steps", "1"], ["--max-database-bytes", "4096"]] {
        let root = private_directory();
        let output = command(root.path())
            .args(args)
            .output()
            .expect("bounded failure");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        let report: Value = serde_json::from_slice(&output.stdout).expect("failed output report");
        assert_eq!(
            report["directory_unix_bytes"],
            serde_json::json!(
                root.path()
                    .canonicalize()
                    .expect("canonical root")
                    .as_os_str()
                    .as_bytes()
            )
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("retained at"));
        let path = root.path().join("store.sqlite");
        assert!(path.is_file());
        let before = fs::read(&path).expect("residue");
        let native = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("inspect failed initialization without recovery writes");
        assert_eq!(
            native
                .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
                .expect("no committed version"),
            0
        );
        assert_eq!(
            native
                .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row
                    .get::<_, i64>(0))
                .expect("no partial schema"),
            0
        );
        drop(native);
        reject(
            command(root.path()).output().expect("no silent repair"),
            "store_exists",
        );
        assert_eq!(fs::read(path).expect("unchanged residue"), before);
    }
}

#[test]
fn failed_post_commit_read_back_preserves_committed_origin_without_success() {
    let root = private_directory();
    // The creation page ceiling is unchanged. This bound applies when the
    // independent verifier opens the already committed database.
    reject(
        command(root.path())
            .args(["--max-storage-bytes", "1"])
            .output()
            .expect("post-commit refusal"),
        "store_storage_limit",
    );
    let path = root.path().join("store.sqlite");
    let before = fs::read(&path).expect("committed output retained");
    let native = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("independent native observation");
    assert_eq!(
        native
            .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
            .expect("committed version"),
        6
    );
    assert_eq!(
        native
            .query_row("SELECT count(*) FROM gazette_journal", [], |row| row
                .get::<_, i64>(0))
            .expect("committed origin"),
        1
    );
    drop(native);
    reject(
        command(root.path())
            .output()
            .expect("no automatic reinitialization"),
        "store_exists",
    );
    assert_eq!(fs::read(path).expect("unchanged committed output"), before);
}

#[test]
fn detached_backup_preserves_coordinates_without_minting_a_new_origin() {
    let root = private_directory();
    let initial = store::initialize(root.path(), options()).expect("initial store");
    let parent = private_directory();
    let backup = store::backup(store::BackupRequest {
        source: &initial.database,
        output_parent: parent.path(),
        protected: protected(root.path()),
        options: store::BackupOptions {
            allow_create: true,
            read: options().session.read,
            ..store::BackupOptions::default()
        },
    })
    .expect("production consistent backup");
    let verified = store::verify_backup(
        &backup.directory,
        backup.manifest_sha256,
        store::BackupVerifyOptions {
            read: options().session.read,
            ..store::BackupVerifyOptions::default()
        },
    )
    .expect("independently pinned verification");
    assert_eq!(verified.snapshot.source.user_version, 6);
    let native = Connection::open_with_flags(
        backup.directory.join("store.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("independent copy read");
    let copied: [u8; 32] = native
        .query_row("SELECT journal_id FROM gazette_journal", [], |row| {
            row.get(0)
        })
        .expect("copied identity");
    assert_eq!(
        copied, initial.header.journal_id,
        "copy identity is not anti-rollback or a new journal origin"
    );
}

#[test]
fn lost_receipt_delivery_never_reports_success_or_reinitializes() {
    let root = private_directory();
    let (reader, writer) = UnixStream::pair().expect("controlled lost output");
    drop(reader);
    let descriptor: OwnedFd = writer.into();
    let output = command(root.path())
        .stdout(Stdio::from(descriptor))
        .output()
        .expect("lost receipt process");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("receipt delivery failed"));
    let retained: Value = serde_json::from_slice(
        output
            .stderr
            .split(|&byte| byte == b'\n')
            .next()
            .expect("original native report"),
    )
    .expect("surviving stderr retains complete receipt");
    let retained_header: store::JournalHeader =
        serde_json::from_value(retained["header"].clone()).expect("independent original header");
    let path = root.path().join("store.sqlite");
    store::inspect_journal(&path, Some(&retained_header), options().session.read)
        .expect("original delivered coordinates match native output");
    let report = store::observe(&path, options().session.read).expect("store exists after failure");
    assert_eq!(report.user_version, 6);
    reject(
        command(root.path()).output().expect("no repeat origin"),
        "store_exists",
    );
}

#[test]
fn initial_schema_rejects_invalid_coordinate_rows() {
    let root = private_directory();
    let receipt = store::initialize(root.path(), options()).expect("real production schema");
    let connection = Connection::open(receipt.database).expect("test-only native writer");
    for sql in [
        "INSERT INTO gazette_journal VALUES(2, randomblob(32), 1, 0)",
        "UPDATE gazette_journal SET journal_id=x'00'",
        "UPDATE gazette_journal SET journal_epoch=0",
        "UPDATE gazette_journal SET writer_fence=-1",
        "UPDATE gazette_journal SET writer_fence='arbitrary'",
    ] {
        assert!(connection.execute_batch(sql).is_err(), "must reject {sql}");
    }
    let row: (i64, i64) = connection
        .query_row(
            "SELECT journal_epoch, writer_fence FROM gazette_journal",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("unchanged origin");
    assert_eq!(row, (1, 0));
}

#[test]
fn response_reservation_refuses_before_creation_and_overflow_retains_native_header() {
    let root = private_directory();
    let refused = command(root.path())
        .args(["--max-output-bytes", "1"])
        .output()
        .expect("bounded refusal");
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no native creation")
            .count(),
        0
    );
    let selected = root
        .path()
        .join("\u{1}".repeat(200))
        .join("\u{2}".repeat(200));
    fs::create_dir_all(&selected).expect("escape-heavy valid output root");
    fs::set_permissions(&selected, fs::Permissions::from_mode(0o700)).expect("private root");
    let limits = store::OutputLimits {
        directory_bytes: selected
            .canonicalize()
            .expect("native directory")
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
    let output = command(&selected)
        .args([
            "--max-output-directory-bytes",
            &directory_limit,
            "--max-output-bytes",
            &response_limit,
        ])
        .output()
        .expect("native creation with lossless fallback");
    assert!(
        !output.status.success(),
        "fallback is not complete receipt delivery"
    );
    assert!(output.stdout.len() <= maximum);
    let fallback: Value = serde_json::from_slice(&output.stdout).expect("lossless fallback");
    assert_eq!(fallback["code"], "store_output_response_unavailable");
    assert_eq!(
        fallback["directory_unix_bytes"],
        serde_json::json!(
            selected
                .canonicalize()
                .expect("native directory")
                .as_os_str()
                .as_bytes()
        )
    );
    assert_eq!(fallback["evidence"]["kind"], "initialized");
    let header: store::JournalHeader =
        serde_json::from_value(fallback["evidence"]["header"].clone())
            .expect("original returned header");
    let observed = store::inspect_journal(
        &selected.join("store.sqlite"),
        Some(&header),
        options().session.read,
    )
    .expect("independent read-back uses fallback header");
    assert_eq!(observed.header, header);
    assert_eq!(
        fs::read_dir(&selected)
            .expect("single native output")
            .count(),
        1
    );
}
