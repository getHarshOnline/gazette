//! Actual framed operation and native transport, with no network fixture.
use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

const FIXTURE_TIMEOUT_MS: u64 = 30_000;

fn fixture() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("transport");
    let body = r#"{"url":"https://api.github.com/repos/fixture/site/pages/builds/7","commit":"1111111111111111111111111111111111111111","status":"building"}"#;
    let script = format!("#!/bin/sh\nprintf '%s\\n200' '{body}'\n");
    fs::write(&program, &script).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let request = json!({"curl":{"program":program,"directory":root.path(),"executable":{
        "bytes":script.len(),"sha256":gazette_cli::sdk::source::Sha256Digest::of(script.as_bytes())}},
        "selection":{"remote":"https://github.com/fixture/site.git","build_id":7,"expected_commit":"1111111111111111111111111111111111111111"},
        "options":{"allow_read":true,"allow_execute":true,"allow_network":true,"response_bytes":4096,"timeout_ms":FIXTURE_TIMEOUT_MS}});
    (root, request)
}

fn arguments(request: &Value) -> Value {
    json!({"request_json":serde_json::to_string(request).unwrap()})
}

#[test]
fn framed_pages_operation_preserves_sdk_state_and_schema() {
    let (_root, request) = fixture();
    let response = call("observe_github_pages_build", arguments(&request));
    let report = result(&response);
    let native = gazette_cli::sdk::github::observe_pages_build(
        &serde_json::from_value(request["curl"].clone()).unwrap(),
        &serde_json::from_value(request["selection"].clone()).unwrap(),
        serde_json::from_value(request["options"].clone()).unwrap(),
    )
    .unwrap();
    assert_eq!(report, &json!(native));
    assert_eq!(report["outcome"]["build"]["state"], "pending");
    let descriptor = json!(crate::github::descriptor());
    let output = jsonschema::validator_for(&descriptor["outputSchema"]).unwrap();
    assert!(output.is_valid(report));
    let input = jsonschema::validator_for(&descriptor["inputSchema"]).unwrap();
    assert!(input.is_valid(&arguments(&request)));
    assert!(!input.is_valid(&json!({"request_json":request})));
    let content = jsonschema::validator_for(
        &descriptor["inputSchema"]["properties"]["request_json"]["contentSchema"],
    )
    .unwrap();
    assert!(content.is_valid(&request));
    for pointer in ["/curl", "/curl/executable", "/selection", "/options"] {
        let mut wrong = request.clone();
        *wrong.pointer_mut(pointer).unwrap() = json!([]);
        assert!(!content.is_valid(&wrong));
    }
    for permission in ["allow_read", "allow_execute", "allow_network"] {
        let mut denied = request.clone();
        denied["options"][permission] = json!(false);
        let response = call("observe_github_pages_build", arguments(&denied));
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["code"],
            "github_permission"
        );
        assert!(output.is_valid(&response["result"]["structuredContent"]));
    }
    let limited = crate::github::decode(arguments(&request).as_object().unwrap().clone())
        .unwrap()
        .execute(1)
        .unwrap();
    assert_eq!(limited.is_error, Some(true));
    assert_eq!(
        limited.structured_content,
        Some(json!({"code":"mcp_output_limit"}))
    );
}

#[test]
fn exact_request_bytes_reject_duplicates_arrays_unknowns_and_wrong_pins() {
    let (_root, request) = fixture();
    for pointer in ["", "/curl", "/curl/executable", "/selection", "/options"] {
        let mut wrong = request.clone();
        *wrong.pointer_mut(pointer).unwrap() = json!([]);
        let response = call("observe_github_pages_build", arguments(&wrong));
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            response["result"]["structuredContent"]["code"],
            "github_request"
        );
    }
    let duplicate = serde_json::to_string(&request)
        .unwrap()
        .replace("\"build_id\":7", "\"build_id\":7,\"build\\u005fid\":8");
    let response = call(
        "observe_github_pages_build",
        json!({"request_json":duplicate}),
    );
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "github_request"
    );
    let mut wrong = request.clone();
    wrong["options"]["unknown"] = json!(true);
    assert_eq!(
        call("observe_github_pages_build", arguments(&wrong))["result"]["structuredContent"]["code"],
        "github_request"
    );
    let mut wrong = request;
    wrong["curl"]["executable"]["sha256"] = json!("0".repeat(64));
    assert_eq!(
        call("observe_github_pages_build", arguments(&wrong))["result"]["structuredContent"]["code"],
        "github_transport"
    );
}
