//! The real executable and MCP share exact retained-output disclosure evidence.

use gazette_sdk::{build, source};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    process::{Command, Stdio},
};

#[path = "support/mcp.rs"]
mod carrier;

fn cli(
    root: &std::path::Path,
    identity: source::TreeIdentity,
    rules: &str,
) -> std::process::Output {
    cli_with_limits(root, identity, rules, &[])
}

fn cli_with_limits(
    root: &std::path::Path,
    identity: source::TreeIdentity,
    rules: &str,
    limits: &[&str],
) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("probe-output")
        .args(limits)
        .arg("--tree")
        .arg(root)
        .arg("--tree-identity")
        .arg(serde_json::to_string(&identity).expect("identity JSON"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native CLI");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(rules.as_bytes())
        .expect("private rules");
    child.wait_with_output().expect("complete CLI result")
}

fn request(arguments: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"probe_output","arguments":arguments}})
}

#[test]
fn actual_cli_mcp_and_sdk_reports_match_escaped_canaries_and_exact_tree_binding() {
    let root = tempfile::tempdir().expect("output fixture");
    let files = [
        ("page.html", "<p>&#72;IDDEN123</p>"),
        ("data.json", r#"{"value":"HID\u0044EN123"}"#),
        ("unqualified.bin", "opaque"),
    ];
    for (name, bytes) in files {
        fs::write(root.path().join(name), bytes).expect("fixture file");
    }
    let identity = source::inspect(root.path(), None, Default::default())
        .expect("independent pin")
        .identity();
    let output = source::capture_pinned(root.path(), identity, Default::default())
        .expect("retained exact bytes");
    let text = json!({"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:private","literal":"HIDDEN123"}]}).to_string();
    let rules = build::disclosure_input::output_rules(&text, 16 * 1024).expect("native rules");
    let expected = json!(
        build::probe_disclosure(&output, &BTreeMap::new(), &rules, Default::default())
            .expect("SDK diagnostic")
    );
    let direct = cli(root.path(), identity, &text);
    assert!(
        direct.status.success(),
        "{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert!(direct.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&direct.stdout).expect("CLI report"),
        expected
    );
    let arguments = json!({"allow_native_reads":true,"root":root.path(),"expected_tree":identity,"rules_json":text});
    let reports = carrier::exchange(vec![request(arguments.clone())]);
    assert_ne!(reports[0]["result"]["isError"], true);
    assert_eq!(reports[0]["result"]["structuredContent"], expected);
    for selection in [json!({}), json!({"html_tokens":100_000})] {
        let mut explicit = arguments.clone();
        explicit["scan_limits"] = selection;
        assert_eq!(
            carrier::exchange(vec![request(explicit)])[0]["result"]["structuredContent"],
            expected
        );
    }
    assert_eq!(
        serde_json::from_slice::<Value>(
            &cli_with_limits(
                root.path(),
                identity,
                &text,
                &["--max-probe-html-tokens", "100000"],
            )
            .stdout
        )
        .expect("explicit native default"),
        expected
    );
    assert_eq!(expected["status"], "partial-evidence");
    assert!(!reports[0].to_string().contains("HIDDEN123"));
    for representation in ["html-text", "json-string"] {
        assert!(
            expected["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .any(|finding| finding["representation"] == representation)
        );
    }
    assert!(
        expected["files"]
            .as_array()
            .expect("coverage")
            .iter()
            .any(|file| file["path"] == "unqualified.bin" && file["decoding"] == "none")
    );
    for (name, bytes) in files {
        assert_eq!(
            fs::read_to_string(root.path().join(name)).expect("unchanged"),
            bytes
        );
    }
    assert_eq!(
        fs::read_dir(root.path()).expect("no created paths").count(),
        files.len()
    );

    let mut denied = arguments.clone();
    denied["allow_native_reads"] = json!(false);
    assert_eq!(
        carrier::exchange(vec![request(denied)])[0]["error"]["code"],
        -32602
    );
    let malformed = r#"{"format":"gazette-disclosure-output-input/1","rules":[["rule:private","PRIVATE-INVALID"]]}"#;
    let mut invalid = arguments.clone();
    invalid["rules_json"] = json!(malformed);
    let response = carrier::exchange(vec![request(invalid)]);
    assert_eq!(response[0]["result"]["isError"], true);
    assert_eq!(
        response[0]["result"]["structuredContent"]["code"],
        "disclosure_output_input"
    );
    assert!(!response[0].to_string().contains("PRIVATE-INVALID"));
    let invalid_cli = cli(root.path(), identity, malformed);
    assert!(!invalid_cli.status.success());
    assert!(invalid_cli.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&invalid_cli.stderr).contains("PRIVATE-INVALID"));
    for (field, value) in [
        ("max_rules_bytes", json!(1)),
        ("source_limits", json!({"total_bytes":1})),
        ("scan_limits", json!({"work_bytes":1})),
    ] {
        let mut limited = arguments.clone();
        limited[field] = value;
        let response = carrier::exchange(vec![request(limited)]);
        assert_eq!(response[0]["result"]["isError"], true);
        assert!(
            response[0]["result"]["structuredContent"]
                .get("findings")
                .is_none()
        );
    }
    fs::write(root.path().join("data.json"), "changed").expect("mutate fixture after pin");
    let response = carrier::exchange(vec![request(arguments)]);
    assert_eq!(response[0]["result"]["isError"], true);
    assert!(
        response[0]["result"]["structuredContent"]
            .get("findings")
            .is_none()
    );
    let changed_cli = cli(root.path(), identity, &text);
    assert!(!changed_cli.status.success());
    assert!(changed_cli.stdout.is_empty());
    assert_eq!(
        json!(
            build::probe_disclosure(&output, &BTreeMap::new(), &rules, Default::default())
                .expect("retained SDK output unchanged")
        ),
        expected
    );
}

#[test]
fn html_callback_budget_has_one_sdk_cli_mcp_mapping_and_preserves_native_refusals() {
    let root = tempfile::tempdir().expect("bounded output fixture");
    let bytes = "<p>x</p>".repeat(40_000);
    fs::write(root.path().join("page.html"), &bytes).expect("exact HTML payload");
    let identity = source::inspect(root.path(), None, Default::default())
        .unwrap()
        .identity();
    let output = source::capture_pinned(root.path(), identity, Default::default()).unwrap();
    let text = json!({"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:one","literal":"absent"}]}).to_string();
    let rules = build::disclosure_input::output_rules(&text, 16 * 1024).unwrap();
    let arguments = json!({"allow_native_reads":true,"root":root.path(),"expected_tree":identity,"rules_json":text});
    for limits in [
        json!({}),
        json!({"html_tokens":100_000}),
        json!({"html_tokens":0}),
    ] {
        let mut input = arguments.clone();
        input["scan_limits"] = limits.clone();
        let response = carrier::exchange(vec![request(input)]);
        assert_eq!(response[0]["result"]["isError"], true);
        assert_eq!(
            response[0]["result"]["structuredContent"]["code"],
            "disclosure_probe_limit"
        );
        assert!(
            response[0]["result"]["structuredContent"]
                .get("files")
                .is_none()
        );
        let native: build::disclosure_input::ScanLimits = serde_json::from_value(limits).unwrap();
        assert!(matches!(
            build::probe_disclosure(&output, &BTreeMap::new(), &rules, native.native()),
            Err(build::DisclosureProbeError::Limit("HTML tokens"))
        ));
    }
    for flags in [
        vec![],
        vec!["--max-probe-html-tokens", "100000"],
        vec!["--max-probe-html-tokens", "0"],
    ] {
        let response = cli_with_limits(root.path(), identity, &text, &flags);
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        assert!(String::from_utf8_lossy(&response.stderr).contains("HTML tokens ceiling"));
    }
    let native: build::disclosure_input::ScanLimits =
        serde_json::from_value(json!({"html_tokens":1_000_000})).unwrap();
    let expected = json!(
        build::probe_disclosure(&output, &BTreeMap::new(), &rules, native.native())
            .expect("explicit adequate callback budget")
    );
    let direct = cli_with_limits(
        root.path(),
        identity,
        &text,
        &["--max-probe-html-tokens", "1000000"],
    );
    assert!(
        direct.status.success(),
        "{}",
        String::from_utf8_lossy(&direct.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&direct.stdout).unwrap(),
        expected
    );
    let mut selected = arguments.clone();
    selected["scan_limits"] = json!({"html_tokens":1_000_000});
    assert_eq!(
        carrier::exchange(vec![request(selected)])[0]["result"]["structuredContent"],
        expected
    );
    for value in [json!(-1), json!(1.5), json!("1000000"), json!(null)] {
        let mut invalid = arguments.clone();
        invalid["scan_limits"] = json!({"html_tokens":value});
        assert_eq!(
            carrier::exchange(vec![request(invalid)])[0]["error"]["code"],
            -32602
        );
        assert!(
            serde_json::from_value::<build::disclosure_input::ScanLimits>(
                json!({"html_tokens":value})
            )
            .is_err()
        );
    }
    for value in ["-1", "1.5", "18446744073709551616"] {
        let invalid = cli_with_limits(
            root.path(),
            identity,
            &text,
            &["--max-probe-html-tokens", value],
        );
        assert!(!invalid.status.success());
        assert!(invalid.stdout.is_empty());
    }
    assert_eq!(
        fs::read(root.path().join("page.html")).unwrap(),
        bytes.as_bytes()
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}
