//! Real native readback, without signatures, journal access or network fixtures.

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use serde_json::json;

use super::*;

struct Fixture {
    root: tempfile::TempDir,
    isolation: tempfile::TempDir,
    targets: Vec<release::Target>,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable native destinations");
        let isolation = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private empty isolation");
        let repository = git2::Repository::init_bare(root.path().join("remote.git"))
            .expect("real local bare Git");
        let tree_id = repository
            .treebuilder(None)
            .expect("empty tree")
            .write()
            .expect("tree object");
        let tree = repository.find_tree(tree_id).expect("native tree");
        let signature = git2::Signature::new(
            "Readback fixture",
            "fixture@example.invalid",
            &git2::Time::new(1, 0),
        )
        .expect("disposable Git metadata");
        let oid = repository
            .commit(
                Some("refs/heads/main"),
                &signature,
                &signature,
                "read-only fixture",
                &tree,
                &[],
            )
            .expect("fixture commit, not a push");
        let value: Value = serde_json::from_str(include_str!(
            "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
        ))
        .expect("native target shape");
        let template: release::Target = serde_json::from_value(value["ordered_targets"][0].clone())
            .expect("one target, not reused signed authorization");
        let targets = (0..3)
            .map(|ordinal| {
                let mut target = template.clone();
                target.ordinal = ordinal;
                target.domain = format!("{ordinal}.readback.example");
                target.remote = root
                    .path()
                    .join(if ordinal == 2 {
                        "missing.git"
                    } else {
                        "remote.git"
                    })
                    .to_str()
                    .expect("UTF-8 fixture")
                    .to_owned();
                target.branch = if ordinal == 1 { "absent" } else { "main" }.to_owned();
                target.local_commit_oid = oid.to_string().parse().expect("full native OID");
                target.refspec =
                    format!("{}:refs/heads/{}", target.local_commit_oid, target.branch);
                target
            })
            .collect();
        Self {
            root,
            isolation,
            targets,
        }
    }

    fn arguments(&self) -> Value {
        json!({"targets_json":serde_json::to_string(&self.targets).expect("exact target bytes"),"git":{"program":crate::native::git(),"isolation":self.isolation.path()},"allow_read":true})
    }

    fn native(&self) -> Value {
        let targets = release::check_targets(&self.targets, release::Limits::default())
            .expect("complete native selection");
        json!(
            release::observe_targets(
                &release::GitSelection {
                    program: crate::native::git().to_owned(),
                    isolation: self.isolation.path().to_path_buf(),
                    credentials: None,
                },
                &targets,
                release::RemoteReadOptions {
                    allow_read: true,
                    ..Default::default()
                }
            )
            .expect("complete native observations")
        )
    }

    fn snapshot(&self) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
        fn walk(path: &Path, files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>) {
            for entry in fs::read_dir(path).expect("fixture directory") {
                let entry = entry.expect("fixture entry");
                let kind = entry.file_type().expect("native type");
                if kind.is_dir() {
                    walk(&entry.path(), files);
                } else {
                    assert!(kind.is_file());
                    files.insert(entry.path(), fs::read(entry.path()).expect("fixture bytes"));
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(self.root.path(), &mut files);
        assert!(
            fs::read_dir(self.isolation.path())
                .expect("isolation remains empty")
                .next()
                .is_none()
        );
        files
    }
}

fn execute(input: Value, maximum: usize) -> CallToolResult {
    decode(input.as_object().expect("request object").clone())
        .expect("valid transport")
        .execute(maximum)
        .expect("complete tool response")
}

fn payload(response: &CallToolResult) -> &Value {
    response
        .structured_content
        .as_ref()
        .expect("structured native result")
}

#[test]
fn complete_native_rows_match_sdk_and_preserve_destinations_and_selection_identity() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    let expected = fixture.native();
    let response = execute(fixture.arguments(), 1 << 20);
    assert_eq!(response.is_error, Some(false));
    assert_eq!(payload(&response), &expected);
    assert_eq!(expected["status"], "native-ref-observations-not-settlement");
    assert_eq!(expected["targets"][0]["readback"]["comparison"], "intended");
    assert_eq!(
        fixture.targets[1].expected_remote_oid_or_unborn,
        release::RemoteState::Unborn
    );
    // The independent local namespace verifier proves this exact branch absent;
    // this is stronger evidence than an empty advertisement alone.
    assert_eq!(expected["targets"][1]["readback"]["comparison"], "prestate");
    assert_eq!(
        expected["targets"][1]["readback"]["advertisement"]["state"],
        "absent"
    );
    assert_eq!(expected["targets"][2]["readback"]["status"], "unavailable");
    assert_eq!(
        expected["targets"]
            .as_array()
            .expect("all rows")
            .iter()
            .map(|row| row["ordinal"].as_u64().expect("ordinal"))
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    let text = serde_json::to_value(&response).expect("wire result");
    assert_eq!(
        serde_json::from_str::<Value>(text["content"][0]["text"].as_str().expect("JSON text"))
            .expect("same readable projection"),
        expected
    );
    let mut limited = fixture.arguments();
    limited["max_response_bytes"] = json!(0);
    let failed_rows = execute(limited, 1 << 20);
    assert_eq!(
        failed_rows.is_error,
        Some(false),
        "a complete report can contain unavailable rows"
    );
    assert_eq!(
        payload(&failed_rows)["target_set_sha256"],
        expected["target_set_sha256"]
    );
    assert_eq!(
        payload(&failed_rows)["targets"][0]["readback"]["error"]["failure"],
        "limit"
    );
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn strict_target_bytes_and_bounds_fail_before_reads_and_permissions_stay_explicit() {
    let fixture = Fixture::new();
    let before = fixture.snapshot();
    let mut denied = fixture.arguments();
    denied["allow_read"] = json!(false);
    assert_eq!(
        payload(&execute(denied.clone(), 1 << 20))["code"],
        "release_read_permission"
    );
    let raw = denied["targets_json"]
        .as_str()
        .expect("original bytes")
        .to_owned();
    for replacement in [
        "\"ordinal\":0,\"ordinal\":0",
        "\"ord\\u0069nal\":0,\"ordinal\":0",
    ] {
        let duplicate = raw.replacen("\"ordinal\":0", replacement, 1);
        assert_ne!(duplicate, raw);
        denied["targets_json"] = json!(duplicate);
        assert_eq!(
            payload(&execute(denied.clone(), 1 << 20))["code"],
            "release_json"
        );
    }
    for field in ["bytes", "nodes", "depth", "targets"] {
        let mut bounded = fixture.arguments();
        bounded["limits"] = json!({field:0});
        assert_eq!(
            payload(&execute(bounded, 1 << 20))["code"],
            "release_limit",
            "{field}"
        );
    }
    for (field, value) in [
        ("timeout_ms", 0),
        ("timeout_ms", 300_001),
        ("max_response_bytes", (1 << 20) + 1),
    ] {
        let mut bounded = fixture.arguments();
        bounded[field] = json!(value);
        assert_eq!(
            payload(&execute(bounded, 1 << 20))["code"],
            "release_read_limit"
        );
    }
    assert_eq!(fixture.snapshot(), before);
}

#[test]
fn discovery_matches_transport_shape_native_defaults_and_integer_ranges() {
    let fixture = Fixture::new();
    let descriptor = json!(descriptor());
    let schema =
        jsonschema::validator_for(&descriptor["inputSchema"]).expect("native input schema");
    assert_eq!(descriptor["name"], "observe_release_targets");
    for hint in ["readOnlyHint", "idempotentHint", "openWorldHint"] {
        assert_eq!(descriptor["annotations"][hint], true);
    }
    assert_eq!(descriptor["annotations"]["destructiveHint"], false);
    assert_eq!(
        descriptor["inputSchema"]["properties"]["timeout_ms"]["default"],
        json!(default_timeout_ms())
    );
    assert_eq!(
        descriptor["inputSchema"]["properties"]["max_response_bytes"]["default"],
        json!(default_response_bytes())
    );
    let valid = fixture.arguments();
    assert!(schema.is_valid(&valid));
    let native = decode(valid.as_object().expect("object").clone()).expect("default request");
    assert!(!native.allow_network);
    assert!(native.git.credentials.is_none());
    assert_eq!(
        Duration::from_millis(native.timeout_ms),
        release::RemoteReadOptions::default().timeout
    );
    let mut selected = valid.clone();
    selected["git"]["credentials"] = json!({"helper":"/absolute/git-credential-fixture"});
    assert!(schema.is_valid(&selected));
    let selected = decode(selected.as_object().expect("object").clone())
        .expect("explicit denied-by-default helper selection");
    let credentials = selected.git.credentials.expect("selected helper");
    assert_eq!(
        credentials.helper,
        PathBuf::from("/absolute/git-credential-fixture")
    );
    assert!(!credentials.allow_credentials);
    for field in ["targets_json", "git", "allow_read"] {
        let mut missing = valid.clone();
        missing.as_object_mut().expect("object").remove(field);
        assert!(!schema.is_valid(&missing));
        assert!(decode(missing.as_object().expect("object").clone()).is_err());
    }
    for (field, value) in [
        ("targets_json", json!([])),
        ("git", json!({"program":"relative","isolation":"/private"})),
        (
            "git",
            json!({"program":"/absolute/git","isolation":"relative"}),
        ),
        (
            "git",
            json!({"program":"/absolute/git","isolation":"/private","credentials":{"helper":"relative"}}),
        ),
        (
            "git",
            json!({"program":"/absolute/git","isolation":"/private","credentials":{"helper":"/"}}),
        ),
        (
            "git",
            json!({"program":"/absolute/git","isolation":"/private","credentials":{"helper":"/absolute/helper --argument","allow_credentials":true}}),
        ),
        (
            "git",
            json!({"program":"/absolute/git","isolation":"/private","credentials":{"helper":"/absolute/helper","allow_credentials":true,"password":"PRIVATE"}}),
        ),
        ("allow_read", json!(1)),
        ("limits", json!([])),
        ("limits", Value::Null),
        ("limits", json!({"unknown":1})),
        ("unknown", json!("PRIVATE")),
    ] {
        let mut wrong = valid.clone();
        wrong[field] = value;
        assert!(!schema.is_valid(&wrong), "{field}");
        let error = decode(wrong.as_object().expect("object").clone())
            .err()
            .expect("shape refused");
        assert!(!format!("{error:?}").contains("PRIVATE"));
    }
    let beyond: Value =
        serde_json::from_str(&((u64::MAX as u128) + 1).to_string()).expect("oversized integer");
    for field in ["timeout_ms", "max_response_bytes"] {
        let mut wrong = valid.clone();
        wrong[field] = beyond.clone();
        assert!(!schema.is_valid(&wrong));
        assert!(decode(wrong.as_object().expect("object").clone()).is_err());
    }
}

#[test]
fn output_schema_covers_actual_complete_rows_refusal_and_no_partial_result_limit() {
    let fixture = Fixture::new();
    let descriptor = json!(descriptor());
    let schema =
        jsonschema::validator_for(&descriptor["outputSchema"]).expect("native output schema");
    let response = execute(fixture.arguments(), 1 << 20);
    let size = serde_json::to_vec(payload(&response))
        .expect("complete result")
        .len();
    assert_eq!(
        payload(&execute(fixture.arguments(), size)),
        payload(&response)
    );
    let limit = execute(fixture.arguments(), size - 1);
    assert_eq!(limit.is_error, Some(true));
    assert_eq!(payload(&limit), &json!({"code":"mcp_output_limit"}));
    let mut denied = fixture.arguments();
    denied["allow_read"] = json!(false);
    for response in [response, limit, execute(denied, 1 << 20)] {
        assert!(schema.is_valid(payload(&response)));
    }
    let mut wrong = fixture.native();
    wrong["target_set_sha256"] = json!("not-a-digest");
    assert!(!schema.is_valid(&wrong));
    let mut wrong = fixture.native();
    wrong["targets"][0]["readback"]["advertisement"]["oid"] = json!("abbreviated");
    assert!(!schema.is_valid(&wrong));
}
