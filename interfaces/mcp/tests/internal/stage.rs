//! Native staging, schema closure, failure custody and bounded response delivery.

use super::*;
use serde_json::{Value, json};
use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt, path::PathBuf};
use tempfile::{TempDir, tempdir};

struct Fixture {
    root: TempDir,
    arguments: Value,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("fixture");
        for name in ["authored", "parent"] {
            fs::create_dir(root.path().join(name)).expect("fixture root");
        }
        fs::write(root.path().join("authored/page.md"), "# Retained 🦀\n").expect("input");
        fs::write(root.path().join("parent/sibling"), "preserved").expect("sibling");
        let limits = source::Limits::default();
        let preparation = source::PreparationLimits {
            sources: 8,
            entries: limits.entries,
            bytes: limits.total_bytes,
            per_source: source::RevisionLimits::default(),
        };
        let arguments = json!({
            "allow_native_reads":true,"create_stage":true,"directory_bytes":256,
            "authored":root.path().join("authored"),"parent":root.path().join("parent"),
            "workspace":"workspace:one","site":"site:one","placements":[],
            "preparation":preparation,"limits":limits,
        });
        Self { root, arguments }
    }

    fn no_stage(&self) {
        assert_eq!(
            fs::read_dir(self.root.path().join("parent"))
                .expect("parent")
                .count(),
            1
        );
        assert_eq!(
            fs::read(self.root.path().join("parent/sibling")).expect("sibling"),
            b"preserved"
        );
        assert_eq!(
            fs::read_to_string(self.root.path().join("authored/page.md")).expect("source"),
            "# Retained 🦀\n"
        );
    }
}

fn call(arguments: &Value, maximum: usize) -> CallToolResult {
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    assert!(
        jsonschema::validator_for(&descriptor["inputSchema"])
            .expect("schema")
            .is_valid(arguments)
    );
    let result = decode(arguments.as_object().expect("object").clone())
        .expect("input")
        .execute(maximum)
        .expect("tool response");
    validate(&result);
    result
}

fn validate(result: &CallToolResult) {
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("schema")
        .validate(result.structured_content.as_ref().expect("evidence"))
        .expect("actual output conforms");
}

fn native_bytes(value: &Value) -> PathBuf {
    let bytes: Vec<u8> = serde_json::from_value(value.clone()).expect("lossless native bytes");
    PathBuf::from(OsString::from_vec(bytes))
}

#[test]
fn named_and_fresh_retention_match_native_inventory_and_preserve_every_input() {
    let fixture = Fixture::new();
    let mut named = fixture.arguments.clone();
    named["name"] = json!("selected-🦀");
    for arguments in [&named, &fixture.arguments] {
        let result = call(arguments, 128 * 1024);
        assert_ne!(result.is_error, Some(true));
        let report = result.structured_content.expect("retention");
        assert_eq!(report["status"], "retained");
        let path = PathBuf::from(report["directory"].as_str().expect("path"));
        let inventory = source::inspect(&path, None, source::Limits::default())
            .expect("independent native readback");
        assert_eq!(report["inventory"], json!(inventory));
        assert_eq!(
            fs::read_to_string(path.join("page.md")).expect("captured bytes"),
            "# Retained 🦀\n"
        );
        if arguments.get("name").is_some() {
            assert_eq!(path.file_name().expect("selected child"), "selected-🦀");
        }
    }
    let collision = call(&named, 128 * 1024);
    assert_eq!(collision.is_error, Some(true));
    assert_eq!(
        collision.structured_content.expect("refusal")["evidence"]["kind"],
        "no_stage"
    );
    assert_eq!(
        fs::read_dir(fixture.root.path().join("parent"))
            .expect("parent")
            .count(),
        3
    );
    assert_eq!(
        fs::read(fixture.root.path().join("parent/sibling")).expect("sibling"),
        b"preserved"
    );
    assert_eq!(
        fs::read_to_string(fixture.root.path().join("authored/page.md")).expect("source"),
        "# Retained 🦀\n"
    );
}

#[test]
fn discovery_and_decoder_separate_read_permission_write_permission_and_shape() {
    let fixture = Fixture::new();
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    assert_eq!(
        descriptor["annotations"],
        json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":true})
    );
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    for (field, value) in [
        ("allow_native_reads", json!(false)),
        ("create_stage", json!(false)),
        ("directory_bytes", json!(0)),
        ("directory_bytes", json!(1048577)),
        ("authored", json!("relative")),
        ("parent", json!("relative")),
        ("limits", json!([1, 2, 3, 4])),
        ("preparation", json!([1, 2, 3, 4])),
        ("name", json!(true)),
        ("unexpected", json!(true)),
    ] {
        let mut arguments = fixture.arguments.clone();
        arguments[field] = value;
        assert!(
            decode(arguments.as_object().expect("object").clone()).is_err(),
            "{field}"
        );
    }
    for field in [
        "create_stage",
        "allow_native_reads",
        "directory_bytes",
        "placements",
    ] {
        let mut missing = fixture.arguments.clone();
        missing.as_object_mut().expect("object").remove(field);
        assert!(
            !schema.is_valid(&missing),
            "{field} must be discoverably required"
        );
        assert!(decode(missing.as_object().expect("object").clone()).is_err());
    }
    let mut extra = fixture.arguments.clone();
    extra["unexpected"] = json!(true);
    assert!(!schema.is_valid(&extra));
    assert!(
        stage_check::decode(fixture.arguments.as_object().expect("object").clone()).is_err(),
        "read-only route must not accept write arguments"
    );
    fixture.no_stage();
}

#[test]
fn native_limits_unsafe_names_overlap_and_response_reservation_fail_before_creation() {
    let fixture = Fixture::new();
    for (field, value) in [
        ("name", json!("../escape")),
        ("name", json!("sibling")),
        ("directory_bytes", json!(1)),
        ("parent", json!(fixture.root.path().join("authored"))),
        ("limits", json!({"total_bytes":0})),
    ] {
        let mut arguments = fixture.arguments.clone();
        arguments[field] = value;
        assert_eq!(call(&arguments, 128 * 1024).is_error, Some(true), "{field}");
        fixture.no_stage();
    }
    let result = call(&fixture.arguments, 1024);
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content.expect("capacity refusal")["code"],
        "stage_response_limit"
    );
    fixture.no_stage();
}

#[test]
fn receipt_overflow_preserves_original_pin_and_native_directory_as_failure() {
    let fixture = Fixture::new();
    for index in 0..100 {
        fs::write(
            fixture
                .root
                .path()
                .join(format!("authored/page-{index:03}.md")),
            "# Entry\n",
        )
        .expect("many entries");
    }
    let result = call(&fixture.arguments, 2048);
    assert_eq!(
        result.is_error,
        Some(true),
        "compact delivery is not complete success"
    );
    let report = result
        .structured_content
        .expect("compact original evidence");
    assert_eq!(report["code"], "stage_receipt_delivery");
    assert_eq!(report["evidence"]["kind"], "retained");
    let path = native_bytes(&report["directory_unix_bytes"]);
    let original: source::TreeIdentity =
        serde_json::from_value(report["evidence"]["inventory_identity"].clone())
            .expect("original identity");
    source::capture_pinned(&path, original, source::Limits::default())
        .expect("independent verification against returned pin");
    fs::write(path.join("page-000.md"), "changed").expect("adversarial change after delivery");
    assert!(source::capture_pinned(&path, original, source::Limits::default()).is_err());
    assert_eq!(
        fs::read_dir(fixture.root.path().join("parent"))
            .expect("retained once")
            .count(),
        2
    );
}

#[test]
fn real_retention_failure_reports_unverified_residue_without_success_pin_or_cleanup() {
    let fixture = Fixture::new();
    let input = decode(fixture.arguments.as_object().expect("object").clone())
        .expect("input")
        .0;
    let reservation = Reservation::new(256, 4096).expect("pre-effect reservation");
    let prepared = source::prepare(input.placements, input.request.preparation).expect("prepare");
    let stage = source::stage(
        &prepared,
        &input.context,
        &input.request.authored,
        source::StageLocation::FreshBounded {
            parent: &input.request.parent,
            directory_bytes: 256,
        },
        input.request.limits,
    )
    .expect("native stage");
    let path = stage.path().to_owned();
    fs::write(path.join("page.md"), "interference")
        .expect("change before native retention readback");
    let result = stage.retain();
    assert!(matches!(result, Err(source::StageError::Residue { .. })));
    let delivered = deliver(reservation, &input.context, &result).expect("failure delivery");
    validate(&delivered);
    assert_eq!(delivered.is_error, Some(true));
    let report = delivered.structured_content.expect("residue");
    assert_eq!(report["code"], "stage_residue");
    assert_eq!(report["evidence"], json!({"kind":"unverified_residue"}));
    assert_eq!(native_bytes(&report["directory_unix_bytes"]), path);
    assert_eq!(
        fs::read_to_string(path.join("page.md")).expect("never removed"),
        "interference"
    );
}
