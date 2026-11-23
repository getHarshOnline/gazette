//! Executable CLI/MCP parity over disposable native Git objects and custody.

#[path = "support/mcp.rs"]
mod mcp;

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{release_commit, source};
use git2::Repository;
use serde_json::{Value, json};

fn call(name: &str, arguments: Value) -> Value {
    mcp::exchange(vec![json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":arguments}})]).remove(0)
}

fn native(mut command: Command, metadata: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(metadata.as_bytes())
        .expect("complete recipe");
    child.wait_with_output().expect("CLI exit")
}

fn metadata() -> String {
    json!({"parent":null,"author":{"name":"Fixture","email":"fixture@example.invalid","seconds":123,"offset_minutes":330},
        "committer":{"name":"Fixture","email":"fixture@example.invalid","seconds":123,"offset_minutes":330},"message":"Exact recipe\n"}).to_string()
}

fn snapshot(path: &Path) -> source::TreeIdentity {
    source::inspect(path, None, Default::default())
        .expect("independent native inventory")
        .identity()
}

#[test]
fn both_executables_prepare_identical_objects_then_retain_and_inspect_exact_custody() {
    let root = tempfile::tempdir().expect("disposable independent repositories");
    let cli_repo = root.path().join("cli.git");
    let mcp_repo = root.path().join("mcp.git");
    let output = root.path().join("output");
    fs::create_dir(&output).expect("output");
    fs::write(output.join("index.html"), b"<h1>Byte parity</h1>\r\n").expect("page");
    fs::write(output.join("binary"), [0, 255, 13, 10]).expect("binary");
    for repo in [&cli_repo, &mcp_repo] {
        Repository::init_bare(repo).expect("bare fixture");
    }
    let tree = snapshot(&output);
    let metadata = metadata();
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("prepare-release-commit")
        .arg(&cli_repo)
        .arg(&output)
        .args([
            "--allow-write-objects",
            "--expected-tree",
            &json!(tree).to_string(),
        ]);
    let cli = native(command, &metadata);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let expected: Value = serde_json::from_slice(&cli.stdout).expect("native report");
    let response = call(
        "prepare_release_commit",
        json!({"allow_native_reads":true,"allow_write_objects":true,
        "root":mcp_repo,"output_root":output,"protected_roots":[],"expected_tree":tree,"metadata_json":metadata}),
    );
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(response["result"]["structuredContent"], expected);
    let projected: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("JSON");
    assert_eq!(projected, expected);
    let id = source::Sha256Digest::of(b"explicit custody");
    let commit = expected["commit"].as_str().expect("exact commit");
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("retain-release-commit")
        .arg(&cli_repo)
        .arg(commit)
        .args([
            "--allow-retain",
            "--custody-id",
            &id.to_string(),
            "--expected-tree",
            &json!(tree).to_string(),
        ])
        .arg("--protected-root")
        .arg(&output);
    let cli = native(command, &metadata);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let expected: Value = serde_json::from_slice(&cli.stdout).expect("native custody report");
    let response = call(
        "retain_release_commit",
        json!({"allow_native_reads":true,"allow_retain":true,"root":mcp_repo,
        "protected_roots":[output],"commit":commit,"custody_id":id,"expected_tree":tree,"metadata_json":metadata}),
    );
    assert_eq!(response["result"]["isError"], false, "{response}");
    assert_eq!(response["result"]["structuredContent"], expected);
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-release-custody")
        .arg(&mcp_repo)
        .arg(commit)
        .args([
            "--custody-id",
            &id.to_string(),
            "--expected-tree",
            &json!(tree).to_string(),
        ]);
    let readback = native(command, &metadata);
    assert!(
        readback.status.success(),
        "{}",
        String::from_utf8_lossy(&readback.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&readback.stdout).expect("actual independent readback"),
        expected
    );
    for repo in [&cli_repo, &mcp_repo] {
        let repo = Repository::open_bare(repo).expect("native repository");
        let names: Vec<_> = repo
            .references()
            .expect("refs")
            .map(|r| r.expect("ref").name().expect("name").to_owned())
            .collect();
        assert_eq!(names, [release_commit::custody_reference(id)]);
        assert!(repo.head().is_err(), "no publication branch created");
    }
    assert_eq!(snapshot(&output), tree);
}

#[test]
fn exact_metadata_rejections_and_native_failure_effects_match_without_object_writes() {
    let root = tempfile::tempdir().expect("disposable repository");
    let repo = root.path().join("repo.git");
    Repository::init_bare(&repo).expect("bare");
    let output = root.path().join("output");
    fs::create_dir(&output).expect("output");
    fs::write(output.join("page"), b"original").expect("page");
    let tree = snapshot(&output);
    let original = snapshot(&repo);
    let valid = metadata();
    let mut missing: Value = serde_json::from_str(&valid).expect("metadata");
    missing
        .as_object_mut()
        .expect("metadata object")
        .remove("parent");
    for metadata in [
        missing.to_string(),
        valid.replacen('{', "{\"parent\":null,", 1),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
        command
            .arg("prepare-release-commit")
            .arg(&repo)
            .arg(&output)
            .args([
                "--allow-write-objects",
                "--expected-tree",
                &json!(tree).to_string(),
            ]);
        assert!(!native(command, &metadata).status.success());
        let response = call(
            "prepare_release_commit",
            json!({"allow_native_reads":true,"allow_write_objects":true,"root":repo,
            "output_root":output,"protected_roots":[],"expected_tree":tree,"metadata_json":metadata}),
        );
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert_eq!(snapshot(&repo), original);
    }
    fs::write(output.join("page"), b"changed source").expect("moved frontier");
    let response = call(
        "prepare_release_commit",
        json!({"allow_native_reads":true,"allow_write_objects":true,"root":repo,
        "output_root":output,"protected_roots":[],"expected_tree":tree,"metadata_json":valid}),
    );
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["effect"],
        "not_attempted"
    );
    assert_eq!(snapshot(&repo), original);
}
