//! Actual `SQLite`/executable boundary; every database is disposable.

#[path = "store_cli/mcp.rs"]
mod mcp;
#[path = "support/mcp.rs"]
mod mcp_carrier;

use gazette_sdk::store::{self, ReadOptions};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::tempdir;

fn run(path: &Path, args: &[&str]) -> Output {
    let cwd = tempdir().expect("independent cwd");
    let result = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("inspect-store")
        .arg(path)
        .args(args)
        .current_dir(cwd.path())
        .output()
        .expect("Gazette");
    assert!(
        fs::read_dir(cwd.path())
            .expect("cwd unchanged")
            .next()
            .is_none()
    );
    result
}

#[test]
fn undisclosed_effects_and_missing_databases_are_refused_without_creation() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("missing.db");
    for (args, code) in [
        (vec![], "store_coordination_permission"),
        (vec!["--allow-coordination-files"], "store_io"),
        (
            vec!["--allow-coordination-files", "--max-vm-steps", "0"],
            "store_invalid_limits",
        ),
    ] {
        let output = run(&path, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
        assert!(
            fs::read_dir(root.path())
                .expect("no database/sidecars")
                .next()
                .is_none()
        );
    }
    let help = run(&path, &["--help"]);
    let help = String::from_utf8(help.stdout).expect("help");
    assert!(help.contains("no create/migrate/SQL writes"));
    assert!(help.contains("--allow-coordination-files"));
    assert_eq!(run(&path, &["--initialize"]).status.code(), Some(2));
}

#[test]
fn executable_and_sdk_match_without_schema_or_row_changes() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    let writer = Connection::open(&path).expect("fixture database");
    writer.execute_batch("CREATE TABLE fixture(value); INSERT INTO fixture VALUES ('not public output'); PRAGMA application_id=987; PRAGMA user_version=123;").expect("fixture");
    drop(writer);
    let before = fs::read(&path).expect("before");
    let output = run(&path, &["--allow-coordination-files"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let sdk = store::observe(
        &path,
        ReadOptions {
            allow_coordination_files: true,
            ..ReadOptions::default()
        },
    )
    .expect("SDK");
    assert_eq!(report, serde_json::to_value(&sdk).expect("SDK JSON"));
    assert_eq!(report["user_version"], 123);
    assert_eq!(report["application_id"], 987);
    assert_eq!(report["sql_read_only"], true);
    let text = String::from_utf8(output.stdout).expect("text");
    assert!(!text.contains("not public output"));
    assert!(!text.contains(root.path().to_str().expect("root")));
    assert_eq!(fs::read(path).expect("unchanged"), before);
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no sidecars for closed DELETE fixture")
            .count(),
        1
    );
}

#[test]
fn malformed_and_over_limit_inputs_never_emit_partial_metadata() {
    let root = tempdir().expect("fixture");
    let path = root.path().join("store.db");
    fs::write(&path, b"malformed database").expect("bad fixture");
    let output = run(&path, &["--allow-coordination-files"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&path).expect("unchanged"), b"malformed database");
    fs::remove_file(&path).expect("only disposable malformed fixture");
    let writer = Connection::open(&path).expect("fixture database");
    writer
        .execute_batch("CREATE TABLE fixture(x)")
        .expect("fixture");
    for args in [
        ["--allow-coordination-files", "--max-storage-bytes", "1"],
        ["--allow-coordination-files", "--max-vm-steps", "1"],
    ] {
        let output = run(&path, &args);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}
