//! Strict private target transport fails before any destination observation.

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    process::{Command, Output, Stdio},
};

use serde_json::{Value, json};

use gazette_test_support as native;

fn targets() -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
    ))
    .expect("existing native target fixture");
    let template: gazette_sdk::release::Target =
        serde_json::from_value(fixture["ordered_targets"][0].clone())
            .expect("one native target shape, not another signed authorization");
    let targets: Vec<_> = (0..3)
        .map(|ordinal| {
            let mut target = template.clone();
            target.ordinal = ordinal;
            target.domain = format!("{ordinal}.readback.example");
            target.remote = format!("https://example.invalid/readback-{ordinal}.git");
            target
        })
        .collect();
    serde_json::to_value(targets).expect("explicit multi-target read-only selection")
}

fn run(input: &[u8], arguments: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("observe-release-targets")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("actual Gazette executable");
    child
        .stdin
        .take()
        .expect("private stdin")
        .write_all(input)
        .expect("complete request");
    child
        .wait_with_output()
        .expect("actual native process completion")
}

fn refused(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "no partial observations");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "gazette: {code}: no complete ref-observation report delivered; no push or consumption\n"
        )
    );
}

#[test]
fn strict_complete_selection_errors_precede_read_permissions_and_provider_selection() {
    let isolation = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private empty isolation");
    let valid =
        json!({"git":{"program":native::git(), "isolation":isolation.path()}, "targets":targets()});
    // The identical valid target array reaches the separate explicit-read gate.
    refused(
        &run(valid.to_string().as_bytes(), &[]),
        "release_read_permission",
    );
    for input in [
        "null",
        "[]",
        "{}",
        "[\"/absolute/git\",\"/private/unused\",[]]",
        "{\"git\":{\"program\":\"/absolute/git\",\"program\":\"/private/PRIVATE_PROVIDER\",\"isolation\":\"/private/unused\"},\"targets\":[]}",
    ] {
        refused(
            &run(input.as_bytes(), &["--allow-read"]),
            "release_read_request",
        );
    }
    let mut wrong = valid.clone();
    wrong["unexpected"] = json!("PRIVATE_SENTINEL");
    refused(
        &run(wrong.to_string().as_bytes(), &["--allow-read"]),
        "release_read_request",
    );
    for field in ["program", "isolation"] {
        let mut wrong = valid.clone();
        wrong["git"][field] = json!("relative-native-path");
        refused(
            &run(wrong.to_string().as_bytes(), &["--allow-read"]),
            "release_read_request",
        );
    }
    for credentials in [
        json!({"helper":"relative"}),
        json!({"helper":"/"}),
        json!({"helper":"/usr/bin/helper --argument","allow_credentials":true}),
        json!({"helper":"/usr/bin/helper","allow_credentials":true,"password":"PRIVATE_SECRET"}),
    ] {
        let mut wrong = valid.clone();
        wrong["git"]["credentials"] = credentials;
        let output = run(
            wrong.to_string().as_bytes(),
            &["--allow-read", "--allow-network"],
        );
        refused(&output, "release_read_request");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE_SECRET"));
    }
    let helper_root = tempfile::tempdir().expect("private helper fixture");
    let helper = helper_root.path().join("credential-helper");
    let marker = helper_root.path().join("unexpected-invocation");
    std::fs::write(
        &helper,
        format!("#!/bin/sh\n: > '{}'\nexit 93\n", marker.display()),
    )
    .expect("inert selected helper");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700))
        .expect("executable helper fixture");
    let mut denied = valid.clone();
    denied["git"]["credentials"] = json!({"helper":helper,"allow_credentials":false});
    let output = run(
        denied.to_string().as_bytes(),
        &["--allow-read", "--allow-network"],
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    assert!(!marker.exists(), "denied helper is never invoked");
    let report: Value = serde_json::from_slice(&output.stdout).expect("complete readback report");
    assert!(
        report["targets"]
            .as_array()
            .expect("complete rows")
            .iter()
            .all(|row| row["readback"]["status"] == "unavailable"
                && row["readback"]["error"]["failure"] == "permission"
                && row["readback"]["error"]["cleanup"].is_null())
    );
    for case in ["empty", "duplicate", "late-force", "reorder"] {
        let mut wrong = valid.clone();
        match case {
            "empty" => wrong["targets"] = json!([]),
            "duplicate" => wrong["targets"][1] = wrong["targets"][0].clone(),
            "late-force" => {
                wrong["targets"][2]["refspec"] = json!(format!(
                    "+{}",
                    wrong["targets"][2]["refspec"]
                        .as_str()
                        .expect("native refspec")
                ))
            }
            "reorder" => wrong["targets"]
                .as_array_mut()
                .expect("target set")
                .swap(0, 1),
            _ => unreachable!("closed transport cases"),
        }
        refused(
            &run(wrong.to_string().as_bytes(), &["--allow-read"]),
            "release_target",
        );
    }
    let duplicate = valid
        .to_string()
        .replacen("\"ordinal\":0", "\"ordinal\":0,\"ordinal\":0", 1);
    assert_ne!(duplicate, valid.to_string());
    refused(
        &run(duplicate.as_bytes(), &["--allow-read"]),
        "release_json",
    );
    assert!(
        std::fs::read_dir(isolation.path())
            .expect("unchanged isolation")
            .next()
            .is_none()
    );
}

#[test]
fn whole_input_and_target_work_limits_return_no_native_observation_prefix() {
    let isolation = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .expect("private empty isolation");
    let input =
        json!({"git":{"program":native::git(), "isolation":isolation.path()}, "targets":targets()})
            .to_string();
    refused(
        &run(
            input.as_bytes(),
            &["--max-input-bytes", "1", "--allow-read"],
        ),
        "release_read_input_limit",
    );
    for (flag, limit) in [
        ("--max-targets", "1"),
        ("--max-nodes", "1"),
        ("--max-depth", "1"),
    ] {
        refused(
            &run(input.as_bytes(), &[flag, limit, "--allow-read"]),
            "release_limit",
        );
    }
    refused(
        &run(input.as_bytes(), &["--allow-read", "--timeout-ms", "0"]),
        "release_read_limit",
    );
    assert!(
        std::fs::read_dir(isolation.path())
            .expect("unchanged isolation")
            .next()
            .is_none()
    );
}
