//! Production initialization and writer acquisition, including actual process boundaries.

use gazette_sdk::store::{
    self, GenerationEffect, InitializeOptions, JournalHeader, ReadOptions, SessionOptions,
    WriterOptions,
};
use rusqlite::Connection;
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[path = "support/session.rs"]
mod session;
use session::Running;

fn options() -> WriterOptions {
    WriterOptions {
        allow_generation_advance: true,
        session: SessionOptions {
            allow_local_reservation: true,
            read: ReadOptions {
                allow_coordination_files: true,
                ..ReadOptions::default()
            },
        },
    }
}

fn fixture() -> (TempDir, JournalHeader) {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private disposable journal");
    let initial = store::initialize(
        root.path(),
        InitializeOptions {
            allow_create: true,
            session: options().session,
            ..InitializeOptions::default()
        },
    )
    .expect("production initialization");
    (root, initial.header)
}

fn header(root: &Path) -> JournalHeader {
    store::inspect_journal(&root.join("store.sqlite"), None, options().session.read)
        .expect("native read-back")
        .header
}

fn command(root: &Path, expected: JournalHeader) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_gazette"));
    cmd.arg("acquire-writer")
        .arg(root)
        .arg("--expected-header")
        .arg(serde_json::to_string(&expected).expect("typed coordinates"))
        .args([
            "--advance-writer-generation",
            "--reserve-local-coordination",
            "--allow-coordination-files",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn refused(root: &Path, expected: JournalHeader, selected: WriterOptions, code: &str) {
    let error = match store::acquire_writer(root, expected, selected) {
        Ok(_) => panic!("unexpected writer acquisition: {code}"),
        Err(error) => error,
    };
    assert_eq!(error.cause.code(), code, "{error}");
    assert_eq!(error.generation, GenerationEffect::NotAttempted);
}

#[test]
fn sdk_cas_requires_exact_previous_coordinates_and_never_resets_on_close_or_drop() {
    let (root, initial) = fixture();
    let mut writer = store::acquire_writer(root.path(), initial, options()).expect("acquire");
    let expected = JournalHeader {
        writer_fence: 1,
        ..initial
    };
    assert_eq!(
        writer.inspect().expect("acquired read-back").header,
        expected
    );
    assert_eq!(header(root.path()), expected);
    refused(root.path(), expected, options(), "store_reservation_busy");
    writer.close().expect("close");
    let bytes = fs::read(root.path().join("store.sqlite")).expect("committed bytes");
    for wrong in [
        initial,
        JournalHeader {
            journal_id: [0; 32],
            ..expected
        },
        JournalHeader {
            journal_epoch: 2,
            ..expected
        },
        JournalHeader {
            schema_version: 2,
            ..expected
        },
    ] {
        refused(root.path(), wrong, options(), "store_journal_mismatch");
    }
    assert_eq!(
        fs::read(root.path().join("store.sqlite")).expect("unchanged"),
        bytes
    );
    drop(
        store::acquire_writer(root.path(), expected, options()).expect("explicit next generation"),
    );
    assert_eq!(
        header(root.path()),
        JournalHeader {
            writer_fence: 2,
            ..initial
        }
    );
    assert_eq!(
        fs::read_dir(root.path()).expect("no marker files").count(),
        1
    );
}

#[test]
fn executable_readiness_proves_committed_generation_and_holds_local_coordination() {
    let (root, initial) = fixture();
    let mut first = Running::start(command(root.path(), initial));
    let ready = first.event();
    assert_eq!(ready["event"], "acquired");
    assert_eq!(ready["inspection"]["header"]["writer_fence"], 1);
    assert_eq!(ready["inspection"]["verifier"]["sql_read_only"], true);
    let expected = JournalHeader {
        writer_fence: 1,
        ..initial
    };
    assert_eq!(header(root.path()), expected);
    let mut second = Running::start(command(root.path(), expected));
    assert_eq!(second.wait().code(), Some(1));
    assert!(second.stderr().contains("store_reservation_busy"));
    first
        .child
        .stdin
        .as_mut()
        .expect("control input")
        .write_all(b"\n")
        .expect("explicit close");
    assert_eq!(first.event()["event"], "closed");
    assert!(first.wait().success());
    assert!(first.stderr().is_empty());
    assert_eq!(header(root.path()), expected);
    let mut next = Running::start(command(root.path(), expected));
    assert_eq!(next.event()["inspection"]["header"]["writer_fence"], 2);
    drop(next.child.stdin.take());
    assert_eq!(next.event()["event"], "closed");
    assert!(next.wait().success());
}

#[test]
fn process_death_releases_lock_but_cannot_revive_previous_generation() {
    let (root, initial) = fixture();
    let mut child = Running::start(command(root.path(), initial));
    assert_eq!(child.event()["event"], "acquired");
    child.child.kill().expect("kill exact disposable child");
    assert!(!child.wait().success());
    let expected = JournalHeader {
        writer_fence: 1,
        ..initial
    };
    assert_eq!(header(root.path()), expected);
    refused(root.path(), initial, options(), "store_journal_mismatch");
    store::acquire_writer(root.path(), expected, options())
        .expect("explicit current generation after inspection")
        .close()
        .expect("close");
    assert_eq!(header(root.path()).writer_fence, 2);
}

#[test]
fn lost_ready_output_is_failure_even_though_generation_is_committed() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};
    let (root, initial) = fixture();
    let (reader, output) = UnixStream::pair().expect("disconnected output fixture");
    drop(reader);
    let child = command(root.path(), initial)
        .stdout(Stdio::from(OwnedFd::from(output)))
        .spawn()
        .expect("Gazette");
    let (_send, events) = mpsc::channel();
    let mut running = Running { child, events };
    assert_eq!(running.wait().code(), Some(1));
    assert!(running.stderr().contains("generation was advanced"));
    assert_eq!(
        header(root.path()),
        JournalHeader {
            writer_fence: 1,
            ..initial
        }
    );
    refused(root.path(), initial, options(), "store_journal_mismatch");
}

#[test]
fn failed_commit_retains_unverified_attempt_evidence_and_requires_read_back() {
    let (root, initial) = fixture();
    // A different process performs the update, so closing its read preflight
    // cannot release this process's POSIX database read locks.
    let reader =
        Connection::open(root.path().join("store.sqlite")).expect("external fixture reader");
    reader
        .execute_batch("BEGIN;")
        .expect("held read transaction");
    let _: i64 = reader
        .query_row("SELECT writer_fence FROM gazette_journal", [], |row| {
            row.get(0)
        })
        .expect("shared lock");
    let mut child = Running::start({
        let mut cmd = command(root.path(), initial);
        cmd.args(["--busy-timeout-ms", "10"]);
        cmd
    });
    assert_eq!(child.wait().code(), Some(1));
    let error = child.stderr();
    assert!(error.contains("store_busy"), "{error}");
    assert!(error.contains("generation effect Unverified"), "{error}");
    reader
        .execute_batch("ROLLBACK;")
        .expect("release external read lock");
    drop(reader);
    // Read-back, not the failed transport, establishes that this fixture rolled back.
    assert_eq!(header(root.path()), initial);
    store::acquire_writer(root.path(), initial, options())
        .expect("new explicit attempt after read-back")
        .close()
        .expect("close");
}

#[test]
fn killed_precommit_writer_preserves_sidecar_and_cannot_silently_reacquire() {
    use std::os::unix::process::ExitStatusExt;

    let (root, initial) = fixture();
    let path = root.path().join("store.sqlite");
    let sidecar = root.path().join("store.sqlite-journal");
    let before = fs::read(&path).expect("committed baseline bytes");
    let reader = Connection::open(&path).expect("independent blocking reader");
    reader.execute_batch("BEGIN;").expect("hold read snapshot");
    let observed: i64 = reader
        .query_row("SELECT writer_fence FROM gazette_journal", [], |row| {
            row.get(0)
        })
        .expect("shared lock");
    assert_eq!(observed, 0);
    let mut child = Running::start({
        let mut cmd = command(root.path(), initial);
        cmd.args(["--busy-timeout-ms", "60000"]);
        cmd
    });
    // The external shared lock prevents COMMIT's exclusive lock. Wait for the
    // real UPDATE to create its rollback journal, not an elapsed-time guess.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            child.child.try_wait().expect("live writer").is_none(),
            "writer exited before the transaction checkpoint"
        );
        if fs::metadata(&sidecar).is_ok_and(|metadata| metadata.len() > 512) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native write checkpoint did not arrive"
        );
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        matches!(child.events.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "no acquisition readiness before COMMIT"
    );
    child
        .child
        .kill()
        .expect("kill this exact precommit writer");
    assert_eq!(child.wait().signal(), Some(9));
    assert!(
        child.stderr().is_empty(),
        "process death is not an acquisition error receipt"
    );
    assert!(
        matches!(
            child.events.recv_timeout(Duration::from_secs(10)),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ),
        "no late acquisition or close event"
    );
    reader
        .execute_batch("ROLLBACK;")
        .expect("release independent shared lock");
    drop(reader);

    let residue = fs::read(&sidecar).expect("interrupted rollback evidence retained");
    assert!(residue.len() > 512);
    assert_eq!(
        &residue[..8],
        &[0; 8],
        "pinned provider's pre-exclusive-lock journal is not hot"
    );
    assert_eq!(
        fs::read(&path).expect("no committed database update"),
        before
    );
    for _ in 0..2 {
        // A cold journal permits observation of committed state, but does not
        // authorize deleting evidence or starting another generation mutation.
        assert_eq!(header(root.path()), initial);
        refused(root.path(), initial, options(), "store_changed");
        assert_eq!(fs::read(&path).expect("main preserved"), before);
        assert_eq!(fs::read(&sidecar).expect("sidecar preserved"), residue);
    }
    assert_eq!(
        fs::read_dir(root.path())
            .expect("exact retained inventory")
            .count(),
        2
    );
}

#[test]
fn generation_or_native_path_change_permanently_invalidates_the_held_lifetime() {
    let (root, initial) = fixture();
    let path = root.path().join("store.sqlite");
    let mut writer =
        store::acquire_writer(root.path(), initial, options()).expect("held generation");
    let external = Connection::open(&path).expect("non-cooperating native writer");
    external
        .execute_batch("UPDATE gazette_journal SET writer_fence=2;")
        .expect("external change");
    assert!(matches!(
        writer.inspect(),
        Err(store::Error::JournalMismatch)
    ));
    external
        .execute_batch("UPDATE gazette_journal SET writer_fence=1;")
        .expect("restore numeric value");
    drop(external);
    assert!(matches!(writer.inspect(), Err(store::Error::Changed)));
    assert!(matches!(writer.close(), Err(store::Error::Changed)));

    let mut writer = store::acquire_writer(root.path(), header(root.path()), options())
        .expect("new explicit lifetime");
    let saved = tempfile::tempdir().expect("preserved fixture directory");
    let preserved = saved.path().join("original.sqlite");
    fs::rename(&path, &preserved).expect("preserve original inode");
    fs::copy(&preserved, &path).expect("same bytes different inode");
    assert!(matches!(writer.inspect(), Err(store::Error::Changed)));
    assert!(writer.close().is_err());
    assert_eq!(
        fs::read(&preserved).expect("original"),
        fs::read(&path).expect("replacement")
    );
}

#[test]
fn permissions_unsafe_namespaces_and_unsupported_storage_fail_before_advancement() {
    let (root, initial) = fixture();
    let path = root.path().join("store.sqlite");
    let before = fs::read(&path).expect("original bytes");
    refused(
        root.path(),
        initial,
        WriterOptions::default(),
        "store_writer_permission",
    );
    let mut selected = options();
    selected.session.allow_local_reservation = false;
    refused(
        root.path(),
        initial,
        selected,
        "store_reservation_permission",
    );
    selected = options();
    selected.session.read.allow_coordination_files = false;
    refused(
        root.path(),
        initial,
        selected,
        "store_coordination_permission",
    );
    selected = options();
    selected.session.read.vm_steps = 0;
    refused(root.path(), initial, selected, "store_invalid_limits");
    selected = options();
    selected.session.read.vm_steps = 1;
    refused(root.path(), initial, selected, "store_interrupted");
    selected = options();
    selected.session.read.storage_bytes = 1;
    refused(root.path(), initial, selected, "store_storage_limit");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("nonprivate database");
    refused(root.path(), initial, options(), "store_unsafe_path");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restore mode");
    let aliases = tempfile::tempdir().expect("aliases");
    let hardlink = aliases.path().join("alias.sqlite");
    fs::hard_link(&path, &hardlink).expect("duplicate local namespace");
    refused(root.path(), initial, options(), "store_unsafe_path");
    fs::remove_file(&hardlink).expect("remove only test-created alias");
    let alias = aliases.path().join("namespace");
    symlink(root.path(), &alias).expect("symlink namespace");
    assert!(store::acquire_writer(&alias, initial, options()).is_err());
    for name in [
        "store.sqlite-journal",
        "store.sqlite-wal",
        "store.sqlite-shm",
        "foreign",
    ] {
        let extra = root.path().join(name);
        fs::write(&extra, b"preserve recovery evidence").expect("extra fixture");
        refused(root.path(), initial, options(), "store_changed");
        assert_eq!(
            fs::read(&extra).expect("preserved sidecar"),
            b"preserve recovery evidence"
        );
        fs::remove_file(extra).expect("remove only fixture-created extra");
    }
    assert_eq!(fs::read(&path).expect("no writes"), before);
    let external = Connection::open(&path).expect("native fixture");
    external
        .execute_batch("CREATE TABLE foreign_table (value TEXT);")
        .expect("unsupported schema");
    drop(external);
    let contaminated = fs::read(&path).expect("foreign bytes");
    refused(root.path(), initial, options(), "store_journal_schema");
    assert_eq!(fs::read(&path).expect("no repair"), contaminated);
}

#[test]
fn exhausted_counter_and_wal_are_not_reset_converted_or_recovered() {
    let (root, initial) = fixture();
    let path = root.path().join("store.sqlite");
    let external = Connection::open(&path).expect("native fixture");
    external
        .execute_batch("UPDATE gazette_journal SET writer_fence=9223372036854775807;")
        .expect("exhausted counter");
    drop(external);
    let expected = JournalHeader {
        writer_fence: i64::MAX as u64,
        ..initial
    };
    refused(root.path(), expected, options(), "store_writer_exhausted");
    assert_eq!(header(root.path()), expected);

    let (wal, initial) = fixture();
    let path = wal.path().join("store.sqlite");
    let external = Connection::open(&path).expect("native WAL fixture");
    external
        .execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL mode");
    drop(external); // Clean close leaves WAL mode in the header, without sidecars.
    let before = fs::read(&path).expect("WAL-mode bytes");
    let error = match store::acquire_writer(wal.path(), initial, options()) {
        Ok(_) => panic!("WAL not qualified"),
        Err(error) => error,
    };
    assert_eq!(error.generation, GenerationEffect::NotAttempted);
    assert!(matches!(
        error.cause,
        store::Error::RecoveryRequired | store::Error::Changed
    ));
    assert_eq!(fs::read(&path).expect("no checkpoint conversion"), before);
}

#[test]
fn copied_coordinates_are_not_claimed_as_cross_host_or_restore_freshness() {
    let (root, initial) = fixture();
    let mut writer =
        store::acquire_writer(root.path(), initial, options()).expect("original writer");
    let expected = writer.inspect().expect("original generation").header;
    let copied = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("copied namespace");
    fs::copy(
        root.path().join("store.sqlite"),
        copied.path().join("store.sqlite"),
    )
    .expect("test-only quiescent byte copy");
    let mut other = store::acquire_writer(copied.path(), expected, options())
        .expect("copy has independent local reservation");
    assert_eq!(
        other.inspect().expect("copy advanced").header.writer_fence,
        2
    );
    assert_eq!(
        writer
            .inspect()
            .expect("original unchanged")
            .header
            .writer_fence,
        1
    );
    other.close().expect("copy close");
    writer.close().expect("original close");
}

#[test]
fn help_and_argument_errors_disclose_effects_without_opening_a_store() {
    let help = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args(["acquire-writer", "--help"])
        .output()
        .expect("help");
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).expect("help text");
    assert!(text.contains("--advance-writer-generation"));
    assert!(text.contains("interruption/lost output"));
    let missing = tempfile::tempdir()
        .expect("unused parent")
        .path()
        .join("missing");
    for invalid in [None, Some("{}"), Some("not-json")] {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_gazette"));
        cmd.arg("acquire-writer").arg(&missing);
        if let Some(value) = invalid {
            cmd.args(["--expected-header", value]);
        }
        assert_eq!(
            cmd.output().expect("invalid invocation").status.code(),
            Some(2)
        );
    }
    assert!(!missing.exists());
}
