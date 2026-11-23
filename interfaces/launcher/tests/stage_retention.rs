//! Actual SDK staging lifetimes in child processes, independently read by Gazette CLI.
//! Process death is not a hardware power-loss or recovery-journal qualification.

use gazette_sdk::source::{self, Limits};
use serde_json::{Value, json};
use std::{
    env, fs,
    io::{self, Read, Write},
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::tempdir;

#[path = "support/session.rs"]
mod session;
use session::Running;

fn main() {
    let args: Vec<_> = env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == "--fixture-stage") {
        assert_eq!(args.len(), 5, "exact fixture invocation");
        fixture_stage(
            args[2].to_str().expect("mode"),
            Path::new(&args[3]),
            Path::new(&args[4]),
        );
        return;
    }
    for retained in [false, true] {
        for killed in [false, true] {
            process_boundary(retained, killed);
        }
    }
    println!("4 SDK stage-lifetime cases passed with independent Gazette CLI read-back");
}

fn fixture_stage(mode: &str, authored: &Path, parent: &Path) {
    assert!(matches!(mode, "temporary" | "retained"));
    let prepared = source::prepare(
        Vec::new(),
        source::PreparationLimits {
            sources: 0,
            entries: 0,
            bytes: 0,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("explicit empty project set; authored content is still staged");
    let context = source::StageContext {
        workspace: "fixture-workspace".into(),
        site: "fixture-site".into(),
        placements: Default::default(),
    };
    let stage = source::stage(
        &prepared,
        &context,
        authored,
        source::StageLocation::Fresh(parent),
        Limits::default(),
    )
    .expect("production stage");
    if mode == "retained" {
        let retained = stage.retain().expect("production retention");
        announce(mode, retained.path(), retained.inventory());
    } else {
        announce(mode, stage.path(), stage.inventory());
        // A normal return drops the temporary guard; SIGKILL cannot run it.
    }
}

fn announce(mode: &str, path: &Path, inventory: &source::Inventory) {
    let mut stdout = io::stdout().lock();
    serde_json::to_writer(
        &mut stdout,
        &json!({"mode": mode, "path": path, "inventory": inventory}),
    )
    .expect("evidence");
    stdout.write_all(b"\n").expect("line");
    stdout.flush().expect("readiness");
    let mut byte = [0];
    if let Err(error) = io::stdin().lock().read_exact(&mut byte) {
        assert_eq!(
            error.kind(),
            io::ErrorKind::UnexpectedEof,
            "one byte or EOF finishes the fixture"
        );
    }
}

fn process_boundary(retained: bool, killed: bool) {
    let authored = tempdir().expect("authored fixture");
    fs::create_dir_all(authored.path().join("nested/deeper/empty")).expect("directories");
    fs::write(
        authored.path().join("nested/deeper/page.md"),
        b"# Retained fixture\n",
    )
    .expect("page");
    let parent = tempdir().expect("stage parent");
    fs::write(parent.path().join("unrelated"), b"preserve").expect("unrelated");
    let expected = serde_json::to_value(
        source::inspect(authored.path(), None, Limits::default()).expect("authored inventory"),
    )
    .expect("JSON");
    let mut command = Command::new(env::current_exe().expect("fixture executable"));
    command
        .arg("--fixture-stage")
        .arg(if retained { "retained" } else { "temporary" })
        .arg(authored.path())
        .arg(parent.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = Running::start(command);
    let event = child.event();
    assert_eq!(
        event["mode"],
        if retained { "retained" } else { "temporary" }
    );
    assert_eq!(event["inventory"], expected);
    let path = Path::new(event["path"].as_str().expect("stage path"));
    assert!(path.starts_with(parent.path().canonicalize().expect("canonical parent")));
    if killed {
        child.child.kill().expect("SIGKILL live fixture");
    } else {
        drop(child.child.stdin.take());
    }
    let status = child.wait();
    if killed {
        assert_eq!(status.signal(), Some(9));
    } else {
        assert!(status.success(), "{status:?}");
    }
    assert!(child.stderr().is_empty());
    assert!(
        matches!(
            child
                .events
                .recv_timeout(std::time::Duration::from_secs(10)),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
        ),
        "no fabricated post-exit evidence"
    );
    if retained || killed {
        for _ in 0..2 {
            let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("inspect-source")
                .arg(path)
                .output()
                .expect("independent Gazette");
            assert!(output.status.success(), "{output:?}");
            let observed: Value = serde_json::from_slice(&output.stdout).expect("inventory");
            assert_eq!(observed, expected);
        }
        assert!(path.join("nested/deeper/empty").is_dir());
    } else {
        assert!(!path.exists(), "normal temporary guard still cleans up");
    }
    assert_eq!(
        fs::read(parent.path().join("unrelated")).expect("unrelated preserved"),
        b"preserve"
    );
    assert_eq!(
        serde_json::to_value(
            source::inspect(authored.path(), None, Limits::default()).expect("authored unchanged")
        )
        .expect("JSON"),
        expected
    );
}
