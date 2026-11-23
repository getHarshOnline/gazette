//! Effectful Git operations through the real MCP service and independent native read-back.

use std::{
    fs,
    path::{Path, PathBuf},
};

use gazette_cli::sdk::{release_commit, source};
use git2::Repository;
use serde_json::{Value, json};
use tempfile::TempDir;

use super::{call, exchange, request, result, runtime, server};

struct Fixture {
    _root: TempDir,
    repository: PathBuf,
    output: PathBuf,
    protected: PathBuf,
    tree: source::TreeIdentity,
    metadata: release_commit::CommitMetadata,
}

fn snapshot(root: &Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("native inventory")
        .identity()
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("disposable Git fixture");
        let repository = root.path().join("objects.git");
        let output = root.path().join("output");
        let protected = root.path().join("authored");
        Repository::init_bare(&repository).expect("explicit native bare repository");
        fs::create_dir(&output).expect("output directory");
        fs::create_dir(&protected).expect("authored directory");
        fs::write(output.join("index.html"), b"<h1>Exact output</h1>\r\n").expect("HTML");
        fs::write(output.join("opaque.bin"), [0, 255, 13, 10]).expect("binary");
        fs::write(protected.join("private.md"), b"PRIVATE unchanged").expect("protected");
        let signature = release_commit::CommitSignature {
            name: "Fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        };
        let metadata = release_commit::CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "Exact message\n".into(),
        };
        let tree = snapshot(&output);
        Self {
            _root: root,
            repository,
            output,
            protected,
            tree,
            metadata,
        }
    }

    fn input(&self) -> Value {
        json!({"root":self.repository, "output_root":self.output,
            "expected_tree":self.tree, "metadata_json":serde_json::to_string(&self.metadata).expect("metadata"),
            "protected_roots":[self.protected], "allow_native_reads":true, "allow_write_objects":true})
    }

    fn retain_input(&self, commit: &Value) -> Value {
        let mut input = self.input();
        let object = input.as_object_mut().expect("object");
        object.remove("output_root");
        object.remove("allow_write_objects");
        object.insert("allow_retain".into(), json!(true));
        object.insert("commit".into(), commit.clone());
        object.insert(
            "custody_id".into(),
            json!(source::Sha256Digest::of(b"exact custody")),
        );
        object.insert(
            "protected_roots".into(),
            json!([self.protected, self.output]),
        );
        input
    }
}

fn checked(name: &str, response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    let value = response["result"]["structuredContent"].clone();
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("text"),
    )
    .expect("JSON");
    assert_eq!(value, text);
    let catalog = runtime().block_on(exchange(
        request("tools/list", json!({})),
        server::Limits::default(),
    ));
    let tool = catalog["result"]["tools"]
        .as_array()
        .expect("catalog")
        .iter()
        .find(|tool| tool["name"] == name)
        .expect("discoverable effect");
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(tool["annotations"]["destructiveHint"], true);
    assert_eq!(tool["annotations"]["idempotentHint"], false);
    jsonschema::validator_for(&tool["outputSchema"])
        .expect("output schema")
        .validate(&value)
        .expect("real output conforms");
    value
}

#[test]
fn prepare_and_retain_use_native_objects_and_exact_independent_readback() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.protected);
    let head = fs::read(fixture.repository.join("HEAD")).expect("HEAD");
    let response = call("prepare_release_commit", fixture.input());
    let prepared = checked("prepare_release_commit", &response);
    assert_eq!(result(&response), &prepared);
    let commit = serde_json::from_value(prepared["commit"].clone()).expect("commit");
    let observed = release_commit::verify(
        &fixture.repository,
        &commit,
        fixture.tree,
        &fixture.metadata,
        Default::default(),
    )
    .expect("independent object verifier");
    assert_eq!(prepared, json!(observed));
    assert_eq!(
        Repository::open_bare(&fixture.repository)
            .expect("repo")
            .references()
            .expect("refs")
            .count(),
        0
    );
    let retained_input = fixture.retain_input(&prepared["commit"]);
    let response = call("retain_release_commit", retained_input.clone());
    let retained = checked("retain_release_commit", &response);
    assert_eq!(result(&response), &retained);
    let id = serde_json::from_value(retained_input["custody_id"].clone()).expect("id");
    let observed = release_commit::inspect_custody(
        &fixture.repository,
        id,
        &commit,
        fixture.tree,
        &fixture.metadata,
        Default::default(),
    )
    .expect("independent custody readback");
    assert_eq!(retained, json!(observed));
    assert_eq!(snapshot(&fixture.output), fixture.tree);
    assert_eq!(snapshot(&fixture.protected), before);
    assert_eq!(
        fs::read(fixture.repository.join("HEAD")).expect("HEAD"),
        head
    );
}

#[test]
fn independent_permissions_named_fields_metadata_and_overlap_refuse_without_writes() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.repository);
    for (key, value) in [
        ("allow_native_reads", json!(false)),
        ("allow_write_objects", json!(false)),
        ("root", json!("relative")),
        ("output_root", json!("relative")),
        ("protected_roots", json!(["relative"])),
        ("limits", json!([])),
        ("limits", json!({"content":[]})),
        ("expected_tree", json!([])),
        ("unexpected", json!(true)),
    ] {
        let mut input = fixture.input();
        input[key] = value;
        let response = call("prepare_release_commit", input);
        assert_eq!(response["error"]["code"], -32602, "{response}");
        assert_eq!(snapshot(&fixture.repository), before);
    }
    let mut missing = fixture.input();
    missing
        .as_object_mut()
        .expect("object")
        .remove("protected_roots");
    assert_eq!(
        call("prepare_release_commit", missing)["error"]["code"],
        -32602
    );
    let metadata = serde_json::to_string(&fixture.metadata).expect("metadata");
    for raw in [
        metadata.replace("\"parent\":null,", ""),
        metadata.replacen("{", "{\"parent\":null,", 1),
        metadata.replace("\"author\":{", "\"author\":{\"name\":\"duplicate\","),
    ] {
        let mut input = fixture.input();
        input["metadata_json"] = json!(raw);
        assert_eq!(
            call("prepare_release_commit", input)["result"]["isError"],
            true
        );
        assert_eq!(snapshot(&fixture.repository), before);
    }
    for (key, value) in [
        ("protected_roots", json!([fixture.repository])),
        ("limits", json!({"content":{"total_bytes":1}})),
        ("max_metadata_bytes", json!(1)),
    ] {
        let mut input = fixture.input();
        input[key] = value;
        let response = call("prepare_release_commit", input);
        assert_eq!(response["result"]["isError"], true, "{response}");
        assert_eq!(snapshot(&fixture.repository), before);
    }
}

#[test]
fn insufficient_delivery_capacity_is_refused_before_objects_or_custody() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.repository);
    let response = runtime().block_on(exchange(
        request(
            "tools/call",
            json!({"name":"prepare_release_commit","arguments":fixture.input()}),
        ),
        server::Limits {
            output_bytes: 128,
            ..Default::default()
        },
    ));
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "release_commit_response_limit"
    );
    assert_eq!(snapshot(&fixture.repository), before);
    let prepared = call("prepare_release_commit", fixture.input());
    let input = fixture.retain_input(&result(&prepared)["commit"]);
    let before = snapshot(&fixture.repository);
    let response = runtime().block_on(exchange(
        request(
            "tools/call",
            json!({"name":"retain_release_commit","arguments":input}),
        ),
        server::Limits {
            output_bytes: 128,
            ..Default::default()
        },
    ));
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        response["result"]["structuredContent"]["code"],
        "release_commit_response_limit"
    );
    assert_eq!(snapshot(&fixture.repository), before);
}

#[test]
fn custody_permission_and_conflict_preserve_original_exact_native_reference() {
    let fixture = Fixture::new();
    let prepared = call("prepare_release_commit", fixture.input());
    let input = fixture.retain_input(&result(&prepared)["commit"]);
    let before = snapshot(&fixture.repository);
    for (key, value) in [
        ("allow_retain", json!(false)),
        ("allow_native_reads", json!(false)),
        ("root", json!("relative")),
        ("protected_roots", json!(["relative"])),
    ] {
        let mut denied = input.clone();
        denied[key] = value;
        assert_eq!(
            call("retain_release_commit", denied)["error"]["code"],
            -32602
        );
        assert_eq!(snapshot(&fixture.repository), before);
    }
    let first = call("retain_release_commit", input.clone());
    result(&first);
    let mut other = fixture.input();
    let mut metadata = fixture.metadata.clone();
    metadata.message = "Distinct commit\n".into();
    other["metadata_json"] = json!(serde_json::to_string(&metadata).expect("metadata"));
    let second = call("prepare_release_commit", other.clone());
    let mut conflicting = fixture.retain_input(&result(&second)["commit"]);
    conflicting["metadata_json"] = other["metadata_json"].clone();
    let before = snapshot(&fixture.repository);
    let failed = call("retain_release_commit", conflicting);
    assert_eq!(failed["result"]["isError"], true);
    let evidence = checked("retain_release_commit", &failed);
    assert_eq!(evidence["effect"], "not_attempted");
    assert_eq!(snapshot(&fixture.repository), before);
    let observed = release_commit::inspect_custody(
        &fixture.repository,
        serde_json::from_value(input["custody_id"].clone()).expect("id"),
        &serde_json::from_value(input["commit"].clone()).expect("commit"),
        fixture.tree,
        &fixture.metadata,
        Default::default(),
    )
    .expect("original custody survives");
    assert_eq!(json!(observed), *result(&first));
}

#[test]
fn postcreation_storage_failure_reports_remaining_custody_without_retry_or_cleanup() {
    let fixture = Fixture::new();
    let prepared = call("prepare_release_commit", fixture.input());
    let mut input = fixture.retain_input(&result(&prepared)["commit"]);
    // The existing repository fits exactly; the newly created ref crosses this
    // native synchronization ceiling only after actual reference creation.
    input["storage_bytes"] = json!(
        source::inspect(&fixture.repository, None, Default::default())
            .expect("pre-write storage inventory")
            .total_bytes()
    );
    let response = call("retain_release_commit", input.clone());
    assert_eq!(response["result"]["isError"], true, "{response}");
    let failure = checked("retain_release_commit", &response);
    assert_eq!(failure["effect"], "reference_may_remain");
    assert!(failure.get("commit").is_none());
    let id = serde_json::from_value(input["custody_id"].clone()).expect("custody");
    let commit = serde_json::from_value(input["commit"].clone()).expect("commit");
    let report = release_commit::inspect_custody(
        &fixture.repository,
        id,
        &commit,
        fixture.tree,
        &fixture.metadata,
        Default::default(),
    )
    .expect("actual original ref survives failed synchronization");
    assert_eq!(report.commit.commit, commit);
    assert_eq!(snapshot(&fixture.output), fixture.tree);
}
