//! Native executable shape, permission and discovery boundaries for preparation reads.

use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};
use tempfile::tempdir;

const AUTHORIZATION: &str =
    include_str!("../../../crates/gazette-compiler/tests/release_authorization/authorization.json");
const BINDING: &str =
    include_str!("../../../crates/gazette-compiler/tests/release_authorization/binding.json");

fn input() -> Value {
    let binding: Value = serde_json::from_str(BINDING).expect("native independent binding fixture");
    json!({
        "database": "/gazette-absent-private-test/journal.sqlite",
        "expected_header": binding["journal"],
        "selections": [{"preparation_id": "11".repeat(32), "site": "site:sample",
            "domain": "0.example", "commit": "aa".repeat(20),
            "expected_remote_oid_or_unborn": {"state": "unborn"}}],
        "protected_roots": []
    })
}

fn run(command: &str, bytes: &[u8], arguments: &[&str]) -> Output {
    let directory = tempdir().expect("independent empty invoking directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg(command)
        .args(arguments)
        .current_dir(directory.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette native preparation command");
    child
        .stdin
        .take()
        .expect("private stdin")
        .write_all(bytes)
        .expect("bounded request delivery");
    let output = child.wait_with_output().expect("Gazette completion");
    assert!(
        std::fs::read_dir(directory.path())
            .expect("invoking directory unchanged")
            .next()
            .is_none()
    );
    output
}

fn rejected(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "no partial private report: {output:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "gazette: {code}: no complete preparation verification report returned; no signing or release effects\n"
        )
    );
}

fn rejected_before_native_inspection(output: Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "no partial private report: {output:?}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        ["release_preparation_request", "release_preparation_selection"]
            .iter()
            .any(|code| stderr
                == format!(
                    "gazette: {code}: no complete preparation verification report returned; no signing or release effects\n"
                )),
        "unexpected pre-native refusal: {stderr}"
    );
}

#[test]
fn malformed_private_shapes_never_reach_native_inspection() {
    let valid = input();
    let original = serde_json::to_string(&valid).expect("native request fixture");
    let mut cases = vec![
        "[]".to_owned(),
        "null".to_owned(),
        "{}".to_owned(),
        format!("{original} PRIVATE_SENTINEL"),
        format!("{{\"database\":\"PRIVATE_SENTINEL\",{}", &original[1..]),
    ];
    for (field, value) in [
        ("PRIVATE_SENTINEL", json!(true)),
        ("expected_header", json!([1, vec![0; 32], 1, 9])),
        ("selections", json!([["PRIVATE_SENTINEL"]])),
        ("database", json!("relative.sqlite")),
        ("protected_roots", json!(["relative-source"])),
    ] {
        let mut changed = valid.clone();
        changed[field] = value;
        cases.push(serde_json::to_string(&changed).expect("invalid shape fixture"));
    }
    let mut unknown_selection = valid.clone();
    unknown_selection["selections"][0]["PRIVATE_SENTINEL"] = json!(true);
    cases.push(unknown_selection.to_string());
    let mut unqualified_site = valid.clone();
    unqualified_site["selections"][0]["site"] = json!("sample");
    cases.push(unqualified_site.to_string());
    let duplicate_remote = original.replace(
        "\"state\":\"unborn\"",
        "\"state\":\"unborn\",\"state\":\"unborn\"",
    );
    assert_ne!(
        duplicate_remote, original,
        "fixture inserts actual duplicate nested state"
    );
    cases.push(duplicate_remote);
    for remote in [
        json!(["unborn"]),
        json!(["commit", "aa".repeat(20)]),
        json!({"state": "unborn", "PRIVATE_SENTINEL": true}),
        json!({"state": "commit", "oid": "aa".repeat(20), "PRIVATE_SENTINEL": true}),
    ] {
        let mut changed = valid.clone();
        changed["selections"][0]["expected_remote_oid_or_unborn"] = remote;
        cases.push(changed.to_string());
    }
    let duplicate_oid = original.replace(
        "\"state\":\"unborn\"",
        &format!(
            "\"state\":\"commit\",\"oid\":\"{}\",\"o\\u0069d\":\"{}\"",
            "aa".repeat(20),
            "aa".repeat(20)
        ),
    );
    assert_ne!(
        duplicate_oid, original,
        "fixture preserves escaped duplicate oid"
    );
    cases.push(duplicate_oid);
    for case in cases {
        rejected_before_native_inspection(run(
            "inspect-release-preparations",
            case.as_bytes(),
            &[],
        ));
    }
    rejected_before_native_inspection(run(
        "verify-prepared-authorization",
        original.as_bytes(),
        &[],
    ));
    let mut wrong_inspect = valid;
    wrong_inspect["authorization"] = json!({});
    rejected_before_native_inspection(run(
        "inspect-release-preparations",
        wrong_inspect.to_string().as_bytes(),
        &[],
    ));
}

#[test]
fn input_selection_and_permission_limits_precede_missing_database_access() {
    let bytes = input().to_string().into_bytes();
    rejected(
        run("inspect-release-preparations", &bytes, &[]),
        "store_coordination_permission",
    );
    rejected(
        run(
            "inspect-release-preparations",
            &bytes,
            &["--max-targets", "0"],
        ),
        "release_preparation_selection",
    );
    rejected(
        run(
            "inspect-release-preparations",
            &bytes,
            &["--max-input-bytes", &(bytes.len() - 1).to_string()],
        ),
        "release_preparation_input_limit",
    );
    rejected(
        run(
            "inspect-release-preparations",
            &bytes,
            &["--max-input-bytes", &bytes.len().to_string()],
        ),
        "store_coordination_permission",
    );
    let mut duplicate = input();
    duplicate["selections"]
        .as_array_mut()
        .expect("ordered selections")
        .push(input()["selections"][0].clone());
    rejected(
        run(
            "inspect-release-preparations",
            duplicate.to_string().as_bytes(),
            &[],
        ),
        "release_preparation_selection",
    );
}

#[test]
fn shared_pure_verification_precedes_native_permission_and_reading() {
    let mut request = input();
    request["authorization"] = serde_json::from_str(AUTHORIZATION).expect("signed fixture");
    request["binding"] = serde_json::from_str(BINDING).expect("independent binding");
    rejected(
        run(
            "verify-prepared-authorization",
            request.to_string().as_bytes(),
            &[],
        ),
        "store_coordination_permission",
    );
    rejected(
        run(
            "verify-prepared-authorization",
            request.to_string().as_bytes(),
            &["--max-authorization-bytes", "1"],
        ),
        "release_limit",
    );
    request["binding"]["emergency_stop"] = json!(true);
    rejected(
        run(
            "verify-prepared-authorization",
            request.to_string().as_bytes(),
            &[],
        ),
        "release_inactive",
    );
    request["binding"] = json!([]);
    rejected(
        run(
            "verify-prepared-authorization",
            request.to_string().as_bytes(),
            &[],
        ),
        "release_shape",
    );
}

#[test]
fn help_discovers_native_bounds_without_signing_or_release_flags() {
    for command in [
        "inspect-release-preparations",
        "verify-prepared-authorization",
    ] {
        let output = run(command, b"", &["--help"]);
        assert!(output.status.success(), "{output:?}");
        let help = String::from_utf8(output.stdout).expect("UTF-8 help");
        for flag in [
            "--allow-coordination-files",
            "--max-input-bytes",
            "--max-output-bytes",
            "--max-evidence-bytes",
            "--max-targets",
            "--max-storage-bytes",
            "--max-total-bytes",
            "--max-git-metadata-bytes",
            "--max-history-objects",
            "--max-history-bytes",
            "--max-history-object-bytes",
        ] {
            assert!(help.contains(flag), "missing {flag}: {help}");
        }
        assert!(!help.contains("--publish"));
        assert!(!help.contains("--advance-writer-generation"));
        if command == "verify-prepared-authorization" {
            assert!(help.contains("--max-authorization-depth"));
            assert!(help.contains("--max-authorization-nodes"));
            assert!(help.contains("--max-authorization-bytes"));
        }
    }
}
