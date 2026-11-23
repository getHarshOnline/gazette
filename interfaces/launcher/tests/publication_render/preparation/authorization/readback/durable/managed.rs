//! Executable lifecycle proof over real prepared output, Git advertisements and `SQLite`.

use super::{
    Local, REF_OBSERVATION_ROW_CEILING, Sha256Digest, fs, oversized_observation_targets, read,
    release, store, unchanged_inputs,
};
use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

const PERMISSIONS: &[&str] = &[
    "--allow-read",
    "--record-observation",
    "--advance-writer-generation",
    "--reserve-local-coordination",
    "--allow-coordination-files",
];

fn invoke(command: &str, request: &str, flags: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg(command)
        .args(flags)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("real Gazette command");
    child
        .stdin
        .take()
        .expect("private input")
        .write_all(request.as_bytes())
        .expect("complete input");
    child.wait_with_output().expect("native completion")
}

fn input(local: &Local, id: Sha256Digest) -> Value {
    json!({
        "root":local.prepared._journal_root.path(), "expected_header":local.prepared.header,
        "id":id, "git":{"program":crate::native::git(), "isolation":local.isolation.path()},
        "targets":local.targets().targets(), "protected_roots":local.prepared.protected,
    })
}

fn report(output: &Output, success: bool) -> Value {
    assert_eq!(output.status.success(), success, "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "structured native evidence: {output:?}"
    );
    serde_json::from_slice(&output.stdout).expect("complete native result JSON")
}

fn inspect(local: &Local, id: Sha256Digest, expected: Option<store::JournalHeader>) -> Value {
    report(
        &invoke(
            "inspect-release-observation",
            &json!({
                "database":local.prepared.database, "id":id, "expected_header":expected,
            })
            .to_string(),
            &["--allow-coordination-files"],
        ),
        true,
    )
}

#[test]
fn managed_cli_records_then_reopens_exact_history_without_resampling() {
    let local = Local::new();
    local.install_intended(0);
    let id = Sha256Digest::of(b"managed native CLI observation");
    let request = input(&local, id);
    let before = unchanged_inputs(&local);
    let recorded = report(
        &invoke(
            "record-release-observation",
            &request.to_string(),
            PERMISSIONS,
        ),
        true,
    );
    assert_eq!(recorded["status"], "observation-recorded-not-settlement");
    let receipt = &recorded["receipt"];
    assert_eq!(receipt["record"]["targets"], request["targets"]);
    assert_eq!(
        receipt["record"]["readbacks"][0]["readback"]["comparison"],
        "intended"
    );
    assert_eq!(
        receipt["record"]["readbacks"][1]["readback"]["comparison"],
        "prestate"
    );
    assert_eq!(unchanged_inputs(&local), before);
    let header: store::JournalHeader = serde_json::from_value(receipt["journal"].clone())
        .expect("native fixture operation or asserted evidence shape");
    assert_eq!(header.writer_fence, local.prepared.header.writer_fence + 1);
    local.install_intended(1);
    let before = local.snapshot();
    let reopened = inspect(&local, id, Some(header));
    assert_eq!(
        reopened["record"], receipt["record"],
        "remote movement cannot rewrite retained evidence"
    );
    local.unchanged(&before);
    let targets = release::parse_targets(
        request["targets"].to_string().as_bytes(),
        Default::default(),
    )
    .expect("native fixture operation or asserted evidence shape");
    let fresh = local.observe(&targets);
    assert!(matches!(
        fresh.targets[1].readback,
        release::TargetReadback::Observed {
            comparison: release::RefComparison::Intended,
            ..
        }
    ));
    assert_eq!(
        reopened["record"]["readbacks"][1]["readback"]["comparison"],
        "prestate"
    );
}

#[test]
fn every_permission_and_protected_root_refuses_before_generation_or_provider_work() {
    let local = Local::new();
    let request = input(&local, Sha256Digest::of(b"permission refusal"));
    let before = local.snapshot();
    for omitted in PERMISSIONS {
        let flags: Vec<_> = PERMISSIONS
            .iter()
            .copied()
            .filter(|flag| flag != omitted)
            .collect();
        let result = report(
            &invoke("record-release-observation", &request.to_string(), &flags),
            false,
        );
        assert_eq!(result["status"], "not-started", "missing {omitted}");
        local.unchanged(&before);
    }
    let mut invalid_selections = Vec::new();
    let mut wrong = request.clone();
    wrong["root"] = json!("relative-path");
    invalid_selections.push(wrong);
    for field in ["program", "isolation"] {
        let mut wrong = request.clone();
        wrong["git"][field] = json!("relative-path");
        invalid_selections.push(wrong);
    }
    for wrong in invalid_selections {
        assert_eq!(
            report(
                &invoke(
                    "record-release-observation",
                    &wrong.to_string(),
                    PERMISSIONS
                ),
                false
            )["status"],
            "not-started"
        );
        local.unchanged(&before);
    }
    let mut overlap = request.clone();
    overlap["protected_roots"] = json!([local.prepared._journal_root.path()]);
    assert_eq!(
        report(
            &invoke(
                "record-release-observation",
                &overlap.to_string(),
                PERMISSIONS
            ),
            false
        )["status"],
        "not-started"
    );
    local.unchanged(&before);
}

#[test]
fn stale_header_and_existing_id_do_not_resample_or_rewrite_the_original_record() {
    use std::os::unix::fs::PermissionsExt;
    let local = Local::new();
    let id = Sha256Digest::of(b"managed history original");
    let request = input(&local, id);
    let recorded = report(
        &invoke(
            "record-release-observation",
            &request.to_string(),
            PERMISSIONS,
        ),
        true,
    );
    let before = local.snapshot();
    let stale = report(
        &invoke(
            "record-release-observation",
            &request.to_string(),
            PERMISSIONS,
        ),
        false,
    );
    assert_eq!(stale["status"], "acquisition-failed");
    assert_eq!(stale["generation"], "NotAttempted");
    local.unchanged(&before);
    let marker = local.remotes.path().join("must-not-run");
    let program = local.remotes.path().join("reader-probe");
    fs::write(
        &program,
        format!("#!/bin/sh\n: > '{}'\nexit 91\n", marker.display()),
    )
    .expect("native fixture operation or asserted evidence shape");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700))
        .expect("native fixture operation or asserted evidence shape");
    let mut replay = request;
    replay["expected_header"] = recorded["receipt"]["journal"].clone();
    replay["git"]["program"] = json!(program);
    let refused = report(
        &invoke(
            "record-release-observation",
            &replay.to_string(),
            PERMISSIONS,
        ),
        false,
    );
    assert_eq!(refused["status"], "recording-failed");
    assert_eq!(refused["failure"]["phase"], "before-read");
    assert_eq!(refused["close_error"], Value::Null);
    assert!(!marker.exists(), "existing ID cannot silently resample");
    let reopened = inspect(&local, id, None);
    assert_eq!(reopened["record"], recorded["receipt"]["record"]);
    assert_eq!(
        reopened["journal"]["header"]["writer_fence"]
            .as_u64()
            .expect("native fixture operation or asserted evidence shape"),
        recorded["receipt"]["journal"]["writer_fence"]
            .as_u64()
            .expect("native fixture operation or asserted evidence shape")
            + 1,
        "the explicitly permitted new acquisition is retained, never rolled back"
    );
}

#[test]
fn managed_history_keeps_unavailable_members_and_failed_append_readbacks() {
    let local = Local::new();
    let mut request = input(&local, Sha256Digest::of(b"unavailable member"));
    request["targets"][1]["remote"] = json!(local.remotes.path().join("not-present.git"));
    let recorded = report(
        &invoke(
            "record-release-observation",
            &request.to_string(),
            PERMISSIONS,
        ),
        true,
    );
    assert_eq!(recorded["receipt"]["record"]["targets"], request["targets"]);
    assert_eq!(
        recorded["receipt"]["record"]["readbacks"]
            .as_array()
            .expect("native fixture operation or asserted evidence shape")
            .len(),
        2
    );
    assert_eq!(
        recorded["receipt"]["record"]["readbacks"][1]["readback"]["status"],
        "unavailable"
    );
    let original = release::parse_targets(
        request["targets"].to_string().as_bytes(),
        Default::default(),
    )
    .expect("original managed target set");
    request["targets"] = serde_json::to_value(oversized_observation_targets(&original, "cli"))
        .expect("bounded observation fixture");
    let failed_id = Sha256Digest::of(b"completed reads cannot fit row");
    request["id"] = json!(failed_id);
    request["expected_header"] = recorded["receipt"]["journal"].clone();
    let mut flags = PERMISSIONS.to_vec();
    let row_ceiling = REF_OBSERVATION_ROW_CEILING.to_string();
    flags.extend(["--max-value-bytes", &row_ceiling]);
    let failed = report(
        &invoke("record-release-observation", &request.to_string(), &flags),
        false,
    );
    assert_eq!(failed["status"], "recording-failed");
    assert_eq!(failed["failure"]["phase"], "append");
    assert_eq!(failed["failure"]["effect"], "not_attempted");
    assert_eq!(
        failed["failure"]["report"]["targets"]
            .as_array()
            .expect("native fixture operation or asserted evidence shape")
            .len(),
        usize::try_from(super::OVERSIZED_OBSERVATION_TARGETS).expect("fixture count")
    );
    assert_eq!(failed["close_error"], Value::Null);
    assert_eq!(inspect(&local, failed_id, None)["record"], Value::Null);
}

#[test]
fn missing_delivery_is_not_rollback_and_native_history_is_independently_readable() {
    let local = Local::new();
    let id = Sha256Digest::of(b"recorded but output ceiling lost response");
    let request = input(&local, id);
    let mut flags = PERMISSIONS.to_vec();
    flags.extend(["--max-output-bytes", "1"]);
    let lost = invoke("record-release-observation", &request.to_string(), &flags);
    assert!(!lost.status.success());
    assert!(lost.stdout.is_empty(), "no partial receipt");
    let message = String::from_utf8(lost.stderr)
        .expect("native fixture operation or asserted evidence shape");
    assert!(message.contains("append may have occurred"), "{message}");
    assert!(!message.contains("no history append attempted"));
    let retained = inspect(&local, id, None);
    assert_eq!(retained["record"]["targets"], request["targets"]);
    assert_eq!(retained["record"]["id"], json!(id));
    assert_eq!(
        retained["journal"]["header"]["writer_fence"]
            .as_u64()
            .expect("native fixture operation or asserted evidence shape"),
        local.prepared.header.writer_fence + 1
    );
    let historical = release::inspect_target_observation(
        &local.prepared.database,
        id,
        None,
        read(),
        Default::default(),
    )
    .expect("native fixture operation or asserted evidence shape");
    assert_eq!(
        serde_json::to_value(historical)
            .expect("native fixture operation or asserted evidence shape"),
        retained
    );
}

#[test]
fn private_transport_rejects_positional_headers_duplicate_targets_and_hidden_effects() {
    let local = Local::new();
    let request = input(&local, Sha256Digest::of(b"strict managed transport"));
    let before = local.snapshot();
    for invalid in [
        "[]".to_owned(),
        request
            .to_string()
            .replacen("\"ordinal\":0", "\"ordinal\":0,\"ordinal\":0", 1),
        {
            let mut wrong = request.clone();
            wrong["expected_header"] = json!([1, vec![0; 32], 1, 1]);
            wrong.to_string()
        },
        {
            let mut wrong = request.clone();
            wrong["hidden_effect"] = json!(true);
            wrong.to_string()
        },
    ] {
        let output = invoke("record-release-observation", &invalid, PERMISSIONS);
        assert!(!output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
        local.unchanged(&before);
    }
    let inspect_request = json!({"database":local.prepared.database,"id":Sha256Digest::of(b"absent"),"expected_header":local.prepared.header}).to_string();
    let denied = invoke("inspect-release-observation", &inspect_request, &[]);
    assert!(!denied.status.success());
    assert!(denied.stdout.is_empty());
    local.unchanged(&before);
}

fn mcp(tool: &str, arguments: Value, failed: bool) -> Value {
    let responses = crate::mcp_carrier::exchange(vec![
        json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}}),
    ]);
    let descriptor = responses[0]["result"]["tools"]
        .as_array()
        .expect("native fixture operation or asserted evidence shape")
        .iter()
        .find(|entry| entry["name"] == tool)
        .expect("discoverable native operation");
    jsonschema::validator_for(&descriptor["inputSchema"])
        .expect("native fixture operation or asserted evidence shape")
        .validate(&arguments)
        .expect("advertised input");
    assert!(responses[1].get("error").is_none(), "{}", responses[1]);
    assert_eq!(
        responses[1]["result"]["isError"], failed,
        "{}",
        responses[1]
    );
    let result = responses[1]["result"]["structuredContent"].clone();
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native fixture operation or asserted evidence shape")
        .validate(&result)
        .expect("actual native result conforms to discovery");
    let text: Value = serde_json::from_str(
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .expect("native fixture operation or asserted evidence shape"),
    )
    .expect("native fixture operation or asserted evidence shape");
    assert_eq!(text, result);
    result
}

fn mcp_arguments(request: &Value) -> Value {
    let mut arguments = request.clone();
    let targets = arguments
        .as_object_mut()
        .expect("native fixture operation or asserted evidence shape")
        .remove("targets")
        .expect("native fixture operation or asserted evidence shape");
    arguments["targets_json"] = json!(targets.to_string());
    for field in [
        "allow_read",
        "record_observation",
        "advance_writer_generation",
        "reserve_local_coordination",
    ] {
        arguments[field] = json!(true);
    }
    arguments["read"] = json!(read());
    arguments
}

#[test]
fn mcp_records_real_native_history_and_inspection_matches_cli_and_sdk() {
    let local = Local::new();
    local.install_intended(1);
    let id = Sha256Digest::of(b"MCP managed observation");
    let request = input(&local, id);
    let before = unchanged_inputs(&local);
    let recorded = mcp("record_release_observation", mcp_arguments(&request), false);
    assert_eq!(recorded["status"], "observation-recorded-not-settlement");
    assert_eq!(recorded["receipt"]["record"]["targets"], request["targets"]);
    let header = serde_json::from_value(recorded["receipt"]["journal"].clone())
        .expect("native fixture operation or asserted evidence shape");
    assert_eq!(unchanged_inputs(&local), before);
    local.install_intended(0);
    let before = local.snapshot();
    let inspected = mcp(
        "inspect_release_observation",
        json!({
            "path":local.prepared.database,"id":id,"expected_header":header,
            "allow_read":true,"read":read(),
        }),
        false,
    );
    let direct = inspect(&local, id, Some(header));
    let sdk = release::inspect_target_observation(
        &local.prepared.database,
        id,
        Some(&header),
        read(),
        Default::default(),
    )
    .expect("native fixture operation or asserted evidence shape");
    assert_eq!(inspected, direct);
    assert_eq!(inspected, json!(sdk));
    assert_eq!(inspected["record"], recorded["receipt"]["record"]);
    local.unchanged(&before);
}

#[test]
fn mcp_permissions_precede_generation_and_append_failure_preserves_complete_observations() {
    let local = Local::new();
    let id = Sha256Digest::of(b"MCP append bound");
    let request = input(&local, id);
    let arguments = mcp_arguments(&request);
    let before = local.snapshot();
    for key in [
        "allow_read",
        "record_observation",
        "advance_writer_generation",
        "reserve_local_coordination",
        "read",
    ] {
        let mut denied = arguments.clone();
        denied
            .as_object_mut()
            .expect("native fixture operation or asserted evidence shape")
            .remove(key);
        assert_eq!(
            mcp("record_release_observation", denied, true)["status"],
            "not-started"
        );
        local.unchanged(&before);
    }
    let mut arguments = arguments;
    let original = release::parse_targets(
        request["targets"].to_string().as_bytes(),
        Default::default(),
    )
    .expect("original managed target set");
    let targets = oversized_observation_targets(&original, "mcp");
    arguments["targets_json"] = json!(
        serde_json::to_string(&targets)
            .expect("native fixture operation or asserted evidence shape")
    );
    arguments["read"]["value_bytes"] = json!(REF_OBSERVATION_ROW_CEILING);
    let failed = mcp("record_release_observation", arguments, true);
    assert_eq!(failed["status"], "recording-failed");
    assert_eq!(failed["failure"]["phase"], "append");
    assert_eq!(
        failed["failure"]["report"]["targets"]
            .as_array()
            .expect("native fixture operation or asserted evidence shape")
            .len(),
        usize::try_from(super::OVERSIZED_OBSERVATION_TARGETS).expect("fixture count")
    );
    assert_eq!(inspect(&local, id, None)["record"], Value::Null);
}
