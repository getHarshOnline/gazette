//! Independent-process crash qualification through the actual Gazette executable.
//!
//! This custom harness also hosts disposable fixture writers. No writer command
//! is added to Gazette, and no fixture schema represents Website adoption.

use std::{
    env, fs,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::tempdir;

const TIMEOUT: Duration = Duration::from_secs(10);

fn main() {
    let args: Vec<_> = env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--fixture-writer") {
        assert_eq!(args.len(), 4, "exact private fixture invocation");
        fixture_writer(args[2].to_str().expect("fixture mode"), Path::new(&args[3]));
    }
    committed_wal_survives_process_death();
    missing_wal_coordination_requires_permission();
    abandoned_wal_transaction_is_not_observed();
    hot_rollback_journal_requires_explicit_recovery();
    println!("4 store crash cases passed through the Gazette executable");
}

fn missing_wal_coordination_requires_permission() {
    let root = tempdir().expect("disposable store");
    let mut writer = writer(root.path(), "wal-committed");
    writer.kill_and_wait();
    let before = store_bytes(root.path(), "-wal");
    let shared_memory = root.path().join("store.db-shm");
    // The exact fixture writer is confirmed dead. Only its derived shared-memory
    // file is removed; the main database and committed WAL remain untouched.
    fs::remove_file(&shared_memory).expect("discard disposable coordination state");
    assert_failure(
        observe_with_permission(root.path(), false),
        "store_coordination_permission",
    );
    assert!(!shared_memory.exists(), "refusal must precede connection");
    assert_eq!(store_bytes(root.path(), "-wal"), before);
    assert_observation(observe(root.path()), 77, "wal");
    assert_eq!(store_bytes(root.path(), "-wal"), before);
}

fn committed_wal_survives_process_death() {
    let root = tempdir().expect("disposable store");
    let mut writer = writer(root.path(), "wal-committed");
    writer.kill_and_wait();
    let before = store_bytes(root.path(), "-wal");
    assert!(!before.1.is_empty(), "committed WAL must actually exist");
    assert_observation(observe(root.path()), 77, "wal");
    assert_eq!(store_bytes(root.path(), "-wal"), before);
    // Read-back must remain stable on a second independent process invocation.
    assert_observation(observe(root.path()), 77, "wal");
    assert_eq!(store_bytes(root.path(), "-wal"), before);
}

fn abandoned_wal_transaction_is_not_observed() {
    let root = tempdir().expect("disposable store");
    let mut writer = writer(root.path(), "wal-uncommitted");
    assert_observation(observe(root.path()), 77, "wal");
    let before = store_bytes(root.path(), "-wal");
    assert!(before.1.len() > 1024 * 1024, "real spilled WAL frames");
    writer.kill_and_wait();
    assert_observation(observe(root.path()), 77, "wal");
    assert_eq!(store_bytes(root.path(), "-wal"), before);
}

fn hot_rollback_journal_requires_explicit_recovery() {
    let root = tempdir().expect("disposable store");
    let mut writer = writer(root.path(), "rollback-uncommitted");
    assert_failure(observe(root.path()), "store_busy");
    writer.kill_and_wait();
    let before = store_bytes(root.path(), "-journal");
    assert!(before.1.len() > 512, "real rollback journal");
    assert!(
        before.1[..8].iter().any(|&byte| byte != 0),
        "hot journal header"
    );
    for _ in 0..2 {
        assert_failure(observe(root.path()), "store_recovery_required");
        assert_eq!(store_bytes(root.path(), "-journal"), before);
    }

    // Only this disposable fixture writer performs rollback recovery. Gazette
    // has no recovery-effect command and must not grant itself this permission.
    let repaired = Connection::open(root.path().join("store.db")).expect("fixture recovery");
    let version: i32 = repaired
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("fixture rollback");
    assert_eq!(version, 77, "never retain abandoned version 99");
    let size: i64 = repaired
        .query_row("SELECT length(value) FROM fixture", [], |row| row.get(0))
        .expect("retained committed row");
    assert_eq!(size, 1024 * 1024, "abandoned replacement must roll back");
    drop(repaired);
    assert_observation(observe(root.path()), 77, "delete");
}

fn fixture_writer(mode: &str, root: &Path) -> ! {
    assert!(matches!(
        mode,
        "wal-committed" | "wal-uncommitted" | "rollback-uncommitted"
    ));
    let connection = Connection::open(root.join("store.db")).expect("fixture creation");
    connection
        .execute_batch("PRAGMA synchronous=FULL; PRAGMA cache_size=4; PRAGMA cache_spill=ON;")
        .expect("fixture durability/spill settings");
    if mode.starts_with("wal-") {
        connection
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;")
            .expect("fixture WAL");
    }
    connection
        .execute_batch("CREATE TABLE fixture(value BLOB); INSERT INTO fixture VALUES (zeroblob(1048576)); PRAGMA user_version=77;")
        .expect("committed fixture baseline");
    if mode.ends_with("uncommitted") {
        connection
            .execute_batch("BEGIN IMMEDIATE; PRAGMA user_version=99; UPDATE fixture SET value=zeroblob(2097152);")
            .expect("uncommitted spilled transaction");
    }
    fs::write(root.join("writer.ready"), b"ready").expect("readiness after SQL completes");
    loop {
        thread::park();
    }
}

fn writer(root: &Path, mode: &str) -> Process {
    let mut command = Command::new(env::current_exe().expect("native test executable"));
    command.args(["--fixture-writer", mode]).arg(root);
    let mut process = Process::spawn(&mut command);
    let started = Instant::now();
    loop {
        assert!(
            process.child().try_wait().expect("writer status").is_none(),
            "writer exited before readiness"
        );
        if fs::read(root.join("writer.ready")).is_ok_and(|bytes| bytes == b"ready") {
            return process;
        }
        assert!(started.elapsed() < TIMEOUT, "writer readiness timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

fn observe(root: &Path) -> Output {
    observe_with_permission(root, true)
}

fn observe_with_permission(root: &Path, allow: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-store")
        .arg(root.join("store.db"))
        .args(["--busy-timeout-ms", "0"]);
    if allow {
        command.arg("--allow-coordination-files");
    }
    Process::spawn(&mut command).output()
}

fn assert_observation(output: Output, version: i32, mode: &str) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete native JSON");
    assert_eq!(report["user_version"], version);
    assert_eq!(report["journal_mode"], mode);
    assert_eq!(report["sql_read_only"], true);
}

fn assert_failure(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "no successful partial report");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

fn store_bytes(root: &Path, sidecar: &str) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(root.join("store.db")).expect("main bytes"),
        fs::read(root.join(format!("store.db{sidecar}"))).expect("sidecar bytes"),
    )
}

struct Process(Option<Child>);

impl Process {
    fn spawn(command: &mut Command) -> Self {
        Self(Some(
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("fixture process"),
        ))
    }

    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("unconsumed child")
    }

    fn kill_and_wait(&mut self) {
        self.child().kill().expect("kill exact fixture child");
        let status = self.child().wait().expect("confirm terminal process");
        assert_eq!(
            status.signal(),
            Some(9),
            "must be process death, not graceful connection close"
        );
        self.0.take();
    }

    fn output(mut self) -> Output {
        let started = Instant::now();
        while self.child().try_wait().expect("process status").is_none() {
            assert!(started.elapsed() < TIMEOUT, "Gazette observation timed out");
            thread::sleep(Duration::from_millis(10));
        }
        self.0
            .take()
            .expect("terminal child")
            .wait_with_output()
            .expect("native output")
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
