//! Real CLI dispatch with a byte-pinned disposable transport, never network.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Output, Stdio},
};

const ALLOW: &[&str] = &["--allow-read", "--allow-execute", "--allow-network"];

fn fixture(body: &str, status: u16) -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("transport");
    let script = format!("#!/bin/sh\nprintf '%s\\n{status}' '{body}'\n");
    fs::write(&program, &script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let request = json!({"curl":{"program":program,"directory":root.path(),"executable":{
        "bytes":script.len(),"sha256":gazette_sdk::source::Sha256Digest::of(script.as_bytes())}},
        "selection":{"remote":"https://github.com/fixture/site.git","build_id":7,"expected_commit":"1111111111111111111111111111111111111111"}});
    (root, request)
}

fn run(input: &[u8], arguments: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("observe-github-pages-build")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
}

fn refused(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(code),
        "{output:?}"
    );
}

#[test]
fn pages_observation_matches_sdk_without_becoming_release_success() {
    let body = r#"{"url":"https://api.github.com/repos/fixture/site/pages/builds/7","commit":"1111111111111111111111111111111111111111","status":"built"}"#;
    let (_root, request) = fixture(body, 200);
    let output = run(&serde_json::to_vec(&request).unwrap(), ALLOW);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let native = gazette_sdk::github::observe_pages_build(
        &serde_json::from_value(request["curl"].clone()).unwrap(),
        &serde_json::from_value(request["selection"].clone()).unwrap(),
        gazette_sdk::github::Options {
            allow_read: true,
            allow_execute: true,
            allow_network: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(report, json!(native));
    assert_eq!(report["outcome"]["build"]["state"], "built");
    assert_eq!(report["outcome"]["build"]["matches_expected_commit"], true);
    assert!(report.get("settlement").is_none());
    let exact = output.stdout.len().to_string();
    let below = (output.stdout.len() - 1).to_string();
    let mut args = ALLOW.to_vec();
    args.extend(["--max-output-bytes", &exact]);
    assert_eq!(
        run(&serde_json::to_vec(&request).unwrap(), &args).stdout,
        output.stdout
    );
    args.pop();
    args.push(&below);
    refused(
        run(&serde_json::to_vec(&request).unwrap(), &args),
        "github_output_limit",
    );
}

#[test]
fn permission_shape_duplicate_and_native_failure_are_explicit() {
    let (_root, request) = fixture("private provider failure", 404);
    let bytes = serde_json::to_vec(&request).unwrap();
    for omitted in 0..ALLOW.len() {
        let args: Vec<_> = ALLOW
            .iter()
            .enumerate()
            .filter_map(|(i, x)| (i != omitted).then_some(*x))
            .collect();
        refused(run(&bytes, &args), "github_permission");
    }
    for pointer in ["/curl", "/curl/executable", "/selection"] {
        let mut wrong = request.clone();
        *wrong.pointer_mut(pointer).unwrap() = json!([]);
        refused(
            run(&serde_json::to_vec(&wrong).unwrap(), ALLOW),
            "github_request",
        );
    }
    let duplicate = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("\"build_id\":7", "\"build_id\":7,\"build\\u005fid\":8");
    refused(run(duplicate.as_bytes(), ALLOW), "github_request");
    let response = run(&bytes, ALLOW);
    assert!(response.status.success());
    let report: Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(report["outcome"]["status"], "not_found_or_hidden");
    assert!(!String::from_utf8_lossy(&response.stdout).contains("private provider failure"));
    let (_root, wrong) = fixture("{}", 200);
    refused(
        run(&serde_json::to_vec(&wrong).unwrap(), ALLOW),
        "github_binding",
    );
    let mut changed = request;
    changed["curl"]["executable"]["sha256"] = json!("0".repeat(64));
    refused(
        run(&serde_json::to_vec(&changed).unwrap(), ALLOW),
        "github_transport",
    );
}
