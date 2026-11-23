//! Complete retained-tree probing, exact schemas and private failure behavior.

use super::*;
use serde_json::json;
use std::{collections::BTreeMap, fs};

fn fixture() -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().expect("disposable output");
    fs::write(root.path().join("page.html"), "<p>&#72;IDDEN123</p>").expect("HTML fixture");
    fs::write(
        root.path().join("data.json"),
        r#"{"value":"HID\u0044EN123"}"#,
    )
    .expect("JSON fixture");
    let tree = source::inspect(root.path(), None, Default::default())
        .expect("independent tree evidence")
        .identity();
    let arguments = json!({"allow_native_reads":true,"root":root.path(),"expected_tree":tree,"rules_json":json!({"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private","literal":"HIDDEN123"}]}).to_string()});
    (root, arguments)
}

fn call(arguments: Value, maximum: usize) -> CallToolResult {
    let response = decode(arguments.as_object().expect("named input").clone())
        .expect("input shape")
        .execute(maximum)
        .expect("tool response");
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native complete schema")
        .validate(
            response
                .structured_content
                .as_ref()
                .expect("structured output"),
        )
        .expect("actual response conforms");
    response
}

#[test]
fn exact_native_report_and_schema_preserve_decoded_channels_not_private_payloads() {
    let (root, arguments) = fixture();
    let response = call(arguments.clone(), 128 * 1024);
    assert_ne!(response.is_error, Some(true));
    let report = response.structured_content.expect("complete report");
    assert_eq!(report["status"], "partial-evidence");
    for (path, representation) in [("page.html", "html-text"), ("data.json", "json-string")] {
        assert!(report["findings"].as_array().expect("findings").iter().any(
            |finding| finding["path"] == path
                && finding["representation"] == representation
                && finding["rule"] == "rule:private"
        ));
    }
    assert!(!report.to_string().contains("HIDDEN123"));
    let pinned = source::capture_pinned(
        root.path(),
        serde_json::from_value(arguments["expected_tree"].clone()).expect("pin"),
        Default::default(),
    )
    .expect("same capture");
    let rules = build::disclosure_input::output_rules(
        arguments["rules_json"].as_str().expect("rules"),
        16 * 1024,
    )
    .expect("same decoder");
    assert_eq!(
        report,
        json!(
            build::probe_disclosure(&pinned, &BTreeMap::new(), &rules, Default::default())
                .expect("same scanner")
        )
    );
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    assert_eq!(
        descriptor["annotations"],
        json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":true})
    );
    let validator = jsonschema::validator_for(&descriptor["outputSchema"]).expect("report schema");
    let mut damaged = report.clone();
    damaged
        .as_object_mut()
        .expect("report")
        .remove("findings_sha256");
    assert!(!validator.is_valid(&damaged));
    let mut damaged = report;
    damaged["findings"][0]["representation"] = json!("invented-decoder");
    assert!(!validator.is_valid(&damaged));
    assert_eq!(fs::read_dir(root.path()).expect("no new output").count(), 2);
}

#[test]
fn permissions_pins_named_limits_and_private_input_fail_without_partial_reports() {
    let (root, arguments) = fixture();
    for (pointer, value) in [
        ("/allow_native_reads", json!(false)),
        ("/root", json!("relative")),
        ("/expected_tree", json!(["wrong", "bad"])),
    ] {
        let mut input = arguments.clone();
        *input.pointer_mut(pointer).expect("field") = value;
        assert!(decode(input.as_object().expect("object").clone()).is_err());
    }
    for field in ["source_limits", "scan_limits"] {
        let mut input = arguments.clone();
        input[field] = json!([1, 2]);
        assert!(decode(input.as_object().expect("object").clone()).is_err());
    }
    let mut malformed = arguments.clone();
    malformed["root"] = json!(root.path().join("PRIVATE-NONEXISTENT"));
    malformed["rules_json"] = json!(
        r#"{"format":"gazette-disclosure-output-input/1","rules":[["rule:one","PRIVATE-NEEDLE"]]}"#
    );
    let failed = call(malformed, 128 * 1024);
    assert_eq!(failed.is_error, Some(true));
    let evidence = failed.structured_content.expect("redacted error");
    assert_eq!(evidence["code"], "disclosure_output_input");
    assert!(!evidence.to_string().contains("PRIVATE"));
    assert!(evidence.get("findings").is_none());
    fs::write(root.path().join("data.json"), "changed PRIVATE bytes")
        .expect("change disposable output");
    let failed = call(arguments, 128 * 1024);
    assert_eq!(failed.is_error, Some(true));
    let evidence = failed.structured_content.expect("pin failure");
    assert!(evidence.get("findings").is_none());
    assert!(!evidence.to_string().contains("PRIVATE"));
}

#[test]
fn capture_rules_scanning_and_response_limits_are_independent_complete_failures() {
    let (_root, arguments) = fixture();
    for (field, value) in [
        ("max_rules_bytes", json!(1)),
        ("source_limits", json!({"total_bytes":1})),
        ("scan_limits", json!({"work_bytes":1})),
        ("scan_limits", json!({"encoded_depth":33})),
    ] {
        let mut input = arguments.clone();
        input[field] = value;
        let failed = call(input, 128 * 1024);
        assert_eq!(failed.is_error, Some(true), "{field}");
        assert!(
            failed
                .structured_content
                .expect("failure")
                .get("findings")
                .is_none()
        );
    }
    let failed = call(arguments, 1);
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content.expect("response ceiling")["code"],
        "mcp_output_limit"
    );
}

#[test]
fn html_callback_budget_discovery_matches_the_shared_typed_request() {
    let (_root, arguments) = fixture();
    let schema = serde_json::to_value(descriptor()).unwrap();
    let validator = jsonschema::validator_for(&schema["inputSchema"]).unwrap();
    for value in [json!(0), json!(100_000), json!(1_000_000)] {
        let mut input = arguments.clone();
        input["scan_limits"] = json!({"html_tokens":value});
        assert!(validator.is_valid(&input));
        let decoded = decode(input.as_object().unwrap().clone()).unwrap();
        assert_eq!(
            decoded.scan_limits.native().html.tokens,
            value.as_u64().unwrap() as usize
        );
    }
    for value in [
        json!(-1),
        json!(1.5),
        json!(1e100),
        json!("1000000"),
        json!(null),
    ] {
        let mut input = arguments.clone();
        input["scan_limits"] = json!({"html_tokens":value});
        assert!(!validator.is_valid(&input));
        assert!(decode(input.as_object().unwrap().clone()).is_err());
    }
}
