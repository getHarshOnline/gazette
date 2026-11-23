//! Shared private rule transport preserves strict shape and native rule validation.

use gazette_sdk::build::disclosure_input;
use serde_json::json;

#[test]
fn exact_named_output_rules_share_native_validation_and_byte_boundary() {
    let text = json!({"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:z","literal":"private-z"},{"id":"rule:a","literal":"private-a"}]}).to_string();
    let rules = disclosure_input::output_rules(&text, text.len()).expect("exact bytes");
    assert_eq!(
        rules.keys().map(|key| key.as_str()).collect::<Vec<_>>(),
        ["rule:a", "rule:z"]
    );
    assert_eq!(rules[&"rule:a".parse().expect("key")], "private-a");
    assert_eq!(
        disclosure_input::output_rules(&text, text.len() - 1)
            .expect_err("one below")
            .code(),
        "disclosure_output_input_limit"
    );
    jsonschema::validator_for(&disclosure_input::output_schema().into())
        .expect("native envelope schema")
        .validate(&serde_json::from_str::<serde_json::Value>(&text).expect("JSON"))
        .expect("same transport shape");
    let workspace = json!({"format":"gazette-disclosure-probe-input/1","workspace":"private native text","rules":[{"id":"rule:z","literal":"private-z"},{"id":"rule:a","literal":"private-a"}]}).to_string();
    let original =
        disclosure_input::WorkspaceInput::parse(&workspace).expect("existing CLI envelope");
    assert_eq!(original.workspace, "private native text");
    assert_eq!(original.rules, rules);
}

#[test]
fn malformed_private_json_never_becomes_duplicate_loss_or_value_bearing_error() {
    for text in [
        r#"["gazette-disclosure-output-input/1",[]]"#,
        r#"{"format":{"gazette-disclosure-output-input/1":null},"rules":[{"id":"rule:a","literal":"PRIVATE-NEEDLE"}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[["rule:a","PRIVATE-NEEDLE"]]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[],"rules":[]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:a","literal":"PRIVATE-NEEDLE","lit\u0065ral":"duplicate"}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:a","literal":"PRIVATE-NEEDLE"},{"id":"rule:a","literal":"other"}]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:a","literal":"PRIVATE-NEEDLE","extra":true}]}"#,
        r#"{"format":"wrong","rules":[],"PRIVATE-FIELD":"PRIVATE-NEEDLE"}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[]}"#,
        r#"{"format":"gazette-disclosure-output-input/1","rules":[{"id":"rule:a","literal":""}]}"#,
    ] {
        let error =
            disclosure_input::output_rules(text, 16 * 1024).expect_err("invalid complete rules");
        assert!(!error.to_string().contains("PRIVATE"));
    }
}

#[test]
fn native_output_checks_decode_markdown_instead_of_treating_it_as_plain_text() {
    use std::collections::BTreeMap;

    use gazette_core::source::{EntryPath, InventoryBuilder};
    use gazette_sdk::{
        build::{DisclosureDecoding, DisclosureProbeError, check_disclosure, probe_disclosure},
        source::{CapturedSource, Content, Sha256Digest},
    };

    let path: EntryPath = "public.md".parse().expect("entry path");
    let bytes = b"# Public\n\n[reference](https://public.test/%53ECRET)\n";
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: bytes.len() as u64,
                sha256: Sha256Digest::of(bytes),
            },
        )
        .expect("inventory");
    let output = CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([(path.clone(), bytes.to_vec())]),
    )
    .expect("retained bytes");
    let identity = output.inventory().identity();
    let rules = BTreeMap::from([("rule:private".parse().expect("rule"), "SECRET".into())]);
    let report = probe_disclosure(&output, &BTreeMap::new(), &rules, Default::default())
        .expect("SDK selects the native parser");
    assert!(matches!(
        report.files()[0].decoding,
        DisclosureDecoding::MarkdownSyntax { .. }
    ));
    assert!(!report.findings().is_empty());
    assert!(matches!(
        check_disclosure(&output, &BTreeMap::new(), &rules, Default::default()),
        Err(DisclosureProbeError::Findings { .. })
    ));
    assert_eq!(output.inventory().identity(), identity);
    assert_eq!(output.file(&path), Some(bytes.as_slice()));
}
