//! Real local connection lifetime, contention, interruption and stale-path evidence.

use gazette_sdk::store::{self, ReadOptions, SessionOptions};
use rusqlite::Connection;
use std::{
    fs,
    io::Write,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
};
use tempfile::TempDir;

#[path = "support/session.rs"]
mod session;
use session::Running;

fn private_directory() -> std::io::Result<TempDir> {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir()
}

fn command(path: &Path, root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("connect-store")
        .arg(path)
        .arg(root)
        .args(["--reserve-local-coordination", "--allow-coordination-files"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn options() -> SessionOptions {
    SessionOptions {
        allow_local_reservation: true,
        read: ReadOptions {
            allow_coordination_files: true,
            ..ReadOptions::default()
        },
    }
}

fn fixture(path: &Path, value: i32) {
    let writer = Connection::open(path).expect("disposable store");
    writer
        .execute_batch("CREATE TABLE fixture(value TEXT); INSERT INTO fixture VALUES('original');")
        .expect("fixture data");
    writer
        .pragma_update(None, "user_version", value)
        .expect("fixture version");
}

#[test]
fn eof_and_sdk_drop_release_only_the_owned_local_session() {
    let source = private_directory().expect("source");
    let root = private_directory().expect("namespace");
    let path = source.path().join("store.db");
    fixture(&path, 3);
    let mut child = Running::start(command(&path, root.path()));
    assert_eq!(child.event()["event"], "connected");
    drop(child.child.stdin.take());
    assert_eq!(child.event()["event"], "closed");
    assert!(child.wait().success());
    let session = store::connect(&path, root.path(), options()).expect("released after EOF");
    drop(session);
    store::connect(&path, root.path(), options())
        .expect("released after Drop")
        .close()
        .expect("close");
}

#[test]
fn failed_readiness_delivery_ends_the_session_and_releases_its_reservation() {
    use std::os::{fd::OwnedFd, unix::net::UnixStream};

    let source = private_directory().expect("source");
    let root = private_directory().expect("namespace");
    let path = source.path().join("store.db");
    fixture(&path, 4);
    // Close the reader before spawning so delivery failure has no scheduling race.
    let (reader, writer) = UnixStream::pair().expect("local disconnected output");
    drop(reader);
    let child = command(&path, root.path())
        .stdout(Stdio::from(OwnedFd::from(writer)))
        .spawn()
        .expect("Gazette");
    let (_send, events) = mpsc::channel();
    let mut running = Running { child, events };
    assert_eq!(running.wait().code(), Some(1));
    assert!(
        running
            .stderr()
            .contains("without successful close evidence")
    );
    store::connect(&path, root.path(), options())
        .expect("failed delivery released reservation")
        .close()
        .expect("close");
}

#[test]
fn executable_reserves_until_explicit_close_and_never_creates_a_lock_marker() {
    let source = private_directory().expect("source");
    let root = private_directory().expect("coordination");
    let path = source.path().join("store.db");
    fixture(&path, 7);
    let before = fs::read(&path).expect("before");
    let mut first = Running::start(command(&path, root.path()));
    let ready = first.event();
    assert_eq!(ready["event"], "connected");
    assert_eq!(ready["observation"]["user_version"], 7);
    assert_eq!(ready["observation"]["sql_read_only"], true);
    assert!(first.child.try_wait().expect("still connected").is_none());
    let mut second = Running::start(command(&path, root.path()));
    assert_eq!(second.wait().code(), Some(1));
    assert!(second.stderr().contains("store_reservation_busy"));
    assert!(matches!(
        store::connect(&path, root.path(), options()),
        Err(store::Error::ReservationBusy)
    ));
    first
        .child
        .stdin
        .as_mut()
        .expect("control input")
        .write_all(b"\n")
        .expect("close request");
    assert_eq!(first.event()["event"], "closed");
    assert!(first.wait().success());
    assert!(first.stderr().is_empty());
    store::connect(&path, root.path(), options())
        .expect("released")
        .close()
        .expect("clean close");
    assert_eq!(fs::read_dir(root.path()).expect("namespace").count(), 0);
    assert_eq!(fs::read(&path).expect("unchanged source"), before);
}

#[test]
fn process_death_releases_native_reservation_without_pid_repair() {
    let source = private_directory().expect("source");
    let root = private_directory().expect("coordination");
    let path = source.path().join("store.db");
    fixture(&path, 11);
    let mut child = Running::start(command(&path, root.path()));
    assert_eq!(child.event()["event"], "connected");
    child.child.kill().expect("kill exact fixture session");
    assert!(!child.wait().success());
    let mut resumed =
        store::connect(&path, root.path(), options()).expect("kernel released dead owner");
    assert_eq!(
        resumed.observe().expect("fresh observation").user_version,
        11
    );
    resumed.close().expect("close");
    assert_eq!(
        fs::read_dir(root.path()).expect("no lock residue").count(),
        0
    );
}

#[test]
fn stale_namespace_cannot_heal_by_restoring_its_path_spelling() {
    let root = private_directory().expect("private parent");
    let path = root.path().join("store.db");
    fixture(&path, 1);
    let coordination = root.path().join("coordination");
    fs::create_dir(&coordination).expect("namespace");
    fs::set_permissions(&coordination, fs::Permissions::from_mode(0o700)).expect("private mode");
    let mut session = store::connect(&path, &coordination, options()).expect("session");
    let preserved = root.path().join("preserved");
    fs::rename(&coordination, &preserved).expect("fixture path movement");
    fs::create_dir(&coordination).expect("different namespace inode");
    fs::set_permissions(&coordination, fs::Permissions::from_mode(0o700)).expect("private mode");
    assert!(matches!(session.observe(), Err(store::Error::Changed)));
    fs::remove_dir(&coordination).expect("remove only empty replacement fixture");
    fs::rename(&preserved, &coordination).expect("restore spelling");
    assert!(matches!(session.observe(), Err(store::Error::Changed)));
    assert!(matches!(session.close(), Err(store::Error::Changed)));
    store::connect(&path, &coordination, options())
        .expect("new lifetime required")
        .close()
        .expect("close");
}

#[test]
fn database_replacement_invalidates_the_session_without_touching_either_store() {
    let root = private_directory().expect("stores");
    let coordination = private_directory().expect("namespace");
    let path = root.path().join("store.db");
    fixture(&path, 1);
    let before = fs::read(&path).expect("before opening");
    let mut session = store::connect(&path, coordination.path(), options()).expect("session");
    let original = root.path().join("original.db");
    fs::rename(&path, &original).expect("preserve original");
    fixture(&path, 2);
    assert!(matches!(session.observe(), Err(store::Error::Changed)));
    assert!(session.close().is_err());
    assert_eq!(fs::read(&original).expect("original unchanged"), before);
    assert_eq!(
        store::observe(&path, options().read)
            .expect("replacement not repaired")
            .user_version,
        2
    );
}

#[test]
fn permission_and_namespace_failures_never_initialize_or_replace_paths() {
    let source = private_directory().expect("source");
    let root = private_directory().expect("namespace");
    let path = source.path().join("store.db");
    fixture(&path, 1);
    assert!(matches!(
        store::connect(&path, root.path(), SessionOptions::default()),
        Err(store::Error::ReservationPermission)
    ));
    assert!(matches!(
        store::connect(
            &path,
            root.path(),
            SessionOptions {
                allow_local_reservation: true,
                ..SessionOptions::default()
            }
        ),
        Err(store::Error::CoordinationPermission)
    ));
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755))
        .expect("nonprivate fixture");
    assert!(matches!(
        store::connect(&path, root.path(), options()),
        Err(store::Error::UnsafePath)
    ));
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).expect("private again");
    let alias = source.path().join("alias");
    symlink(root.path(), &alias).expect("namespace alias");
    assert!(store::connect(&path, &alias, options()).is_err());
    assert!(store::connect(&path, &alias.join(""), options()).is_err());
    let missing = source.path().join("missing.db");
    assert!(store::connect(&missing, root.path(), options()).is_err());
    assert!(!missing.exists());
    store::connect(&path, root.path(), options())
        .expect("failed open released lock")
        .close()
        .expect("close");
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no generated marker")
            .count(),
        0
    );
}

#[test]
fn fresh_snapshots_observe_independent_sql_writers_without_claiming_global_fencing() {
    let root = private_directory().expect("source");
    let coordination = private_directory().expect("namespace");
    let path = root.path().join("store.db");
    fixture(&path, 1);
    let writer = Connection::open(&path).expect("uncoordinated fixture writer");
    writer
        .execute_batch("PRAGMA journal_mode=WAL;")
        .expect("WAL fixture");
    let mut session = store::connect(&path, coordination.path(), options()).expect("session");
    assert_eq!(session.observe().expect("first snapshot").user_version, 1);
    writer
        .pragma_update(None, "user_version", 2)
        .expect("independent commit");
    assert_eq!(session.observe().expect("fresh snapshot").user_version, 2);
    session.close().expect("close");
}

#[test]
fn connection_lifetime_budget_is_not_reset_by_repeated_observations() {
    let source = private_directory().expect("source");
    let root = private_directory().expect("namespace");
    let path = source.path().join("store.db");
    fixture(&path, 9);
    let help = command(&path, root.path())
        .arg("--help")
        .output()
        .expect("native help");
    assert!(help.status.success());
    assert!(
        String::from_utf8_lossy(&help.stdout)
            .contains("per native connection, including all session observations")
    );
    let mut selected = options();
    selected.read.vm_steps = 10_000;
    let mut session = store::connect(&path, root.path(), selected).expect("bounded connection");
    assert_eq!(session.observe().expect("initial snapshot").user_version, 9);
    let mut exhausted = false;
    for _ in 0..1000 {
        match session.observe() {
            Ok(report) => assert_eq!(report.user_version, 9),
            Err(store::Error::Interrupted) => {
                exhausted = true;
                break;
            }
            Err(error) => panic!("unexpected failure: {error}"),
        }
    }
    assert!(
        exhausted,
        "each observation must spend the same lifetime VM budget"
    );
    assert!(matches!(session.observe(), Err(store::Error::Changed)));
    assert!(matches!(session.close(), Err(store::Error::Changed)));
    store::connect(&path, root.path(), options())
        .expect("new explicit connection")
        .close()
        .expect("released");
}
