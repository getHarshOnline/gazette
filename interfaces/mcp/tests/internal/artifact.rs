//! Real native artifact custody through the production MCP carrier, without repair.

use std::{collections::BTreeMap, fs, path::Path};

use gazette_cli::sdk::{release_commit, source};
use git2::{Repository, Signature, Time};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

use super::{call, exchange, request, result, runtime, server};

fn snapshot(root: &Path) -> source::TreeIdentity {
    source::inspect(root, None, Default::default())
        .expect("complete disposable native manifest")
        .identity()
}

fn checked(name: &str, response: &Value) -> Value {
    let value = checked_schema(name, response);
    let text: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .expect("JSON text projection"),
    )
    .expect("complete text projection");
    assert_eq!(value, text);
    value
}

fn checked_schema(name: &str, response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    let value = &response["result"]["structuredContent"];
    let catalog = runtime().block_on(exchange(
        request("tools/list", json!({})),
        server::Limits::default(),
    ));
    let descriptor = catalog["result"]["tools"]
        .as_array()
        .expect("production tool discovery")
        .iter()
        .find(|tool| tool["name"] == name)
        .expect("implemented artifact tool is discoverable");
    assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
    assert_eq!(descriptor["annotations"]["destructiveHint"], false);
    assert_eq!(descriptor["annotations"]["openWorldHint"], true);
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native artifact output schema")
        .validate(value)
        .expect("actual wire result satisfies discovered schema");
    value.clone()
}

fn failed(name: &str, input: Value) -> Value {
    let response = call(name, input);
    assert_eq!(response["result"]["isError"], true, "{response}");
    let value = checked(name, &response);
    assert!(value.get("commit").is_none() && value.get("inventory").is_none());
    value
}

struct Generated {
    stage: source::StagedTree,
    binding: source::GeneratedTreeBinding,
    pin: source::Sha256Digest,
    root: TempDir,
}

impl Generated {
    fn new() -> Self {
        let root = tempdir().expect("disposable generated-tree fixture");
        let repository = root.path().join("repository");
        fs::create_dir_all(repository.join("docs")).expect("source directories");
        fs::write(repository.join("docs/page.md"), "# Retained 🦀 reference\n")
            .expect("native documentation");
        let repo = Repository::init(&repository).expect("native source repository");
        let mut index = repo.index().expect("native source index");
        index
            .add_path(Path::new("docs/page.md"))
            .expect("source entry");
        index.write().expect("source index write");
        let tree = repo
            .find_tree(index.write_tree().expect("source tree"))
            .expect("tree");
        let signature = Signature::new("Fixture", "fixture@example.invalid", &Time::new(100, 0))
            .expect("fixed signature");
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("real source commit");
        let target: source::TargetPath = "project".parse().expect("placement target");
        let prepared = source::prepare(
            vec![source::PlacementRequest {
                target: target.clone(),
                source: source::SourceSelection::Repository {
                    root: repository,
                    subdirectory: "docs".parse().expect("source subtree"),
                    commit: commit.to_string().parse().expect("exact source pin"),
                },
            }],
            source::PreparationLimits {
                sources: 1,
                entries: 8,
                bytes: 4096,
                per_source: Default::default(),
            },
        )
        .expect("real SDK source preparation");
        let context = source::StageContext {
            workspace: "workspace:fixture".into(),
            site: "site:fixture".into(),
            placements: BTreeMap::from([(
                target.clone(),
                source::PlacementIdentity {
                    project: "project:fixture".into(),
                    deployment: "deployment:fixture".into(),
                },
            )]),
        };
        let binding = context.binding(&target).expect("exact placement binding");
        let authored = root.path().join("authored");
        let parent = root.path().join("stages");
        fs::create_dir(&authored).expect("authored root");
        fs::create_dir(&parent).expect("stage parent");
        let stage = source::stage(
            &prepared,
            &context,
            &authored,
            source::StageLocation::Fresh(&parent),
            Default::default(),
        )
        .expect("SDK generated materialization and retained manifest");
        let manifest = format!("project/{}", source::GENERATED_TREE_MANIFEST);
        let pin = stage
            .inventory()
            .entries()
            .iter()
            .find_map(|entry| match entry.content {
                source::Content::File { sha256, .. } if entry.path.as_str() == manifest => {
                    Some(sha256)
                }
                _ => None,
            })
            .expect("pin from original stage receipt, not suspect readback");
        Self {
            stage,
            binding,
            pin,
            root,
        }
    }

    fn arguments(&self) -> Value {
        json!({"root":self.stage.path().join("project"), "allow_native_reads":true,
            "expected_binding":self.binding,"manifest_sha256":self.pin})
    }
}

struct Commit {
    root: TempDir,
    repository: std::path::PathBuf,
    metadata: release_commit::CommitMetadata,
    report: release_commit::CommitReport,
    custody: source::Sha256Digest,
}

impl Commit {
    fn new() -> Self {
        let root = tempdir().expect("disposable native release objects");
        let repository = root.path().join("deployment.git");
        let output = root.path().join("output");
        fs::create_dir(&output).expect("retained output root");
        fs::write(output.join("index.html"), b"<h1>Exact artifact</h1>\n").expect("output bytes");
        Repository::init_bare(&repository).expect("independent bare Git repository");
        let signature = release_commit::CommitSignature {
            name: "PRIVATE fixture".into(),
            email: "fixture@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        };
        let metadata = release_commit::CommitMetadata {
            parent: None,
            author: signature.clone(),
            committer: signature,
            message: "PRIVATE exact recipe\n".into(),
        };
        let report = release_commit::prepare(
            &repository,
            &output,
            snapshot(&output),
            &metadata,
            &[],
            release_commit::CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("actual SDK detached object preparation");
        let custody = source::Sha256Digest::of(b"artifact MCP exact custody");
        release_commit::retain(
            &repository,
            custody,
            &report.commit,
            report.output,
            &metadata,
            &[output],
            release_commit::CustodyOptions {
                allow_retain: true,
                ..Default::default()
            },
        )
        .expect("actual SDK custody reference");
        Self {
            root,
            repository,
            metadata,
            report,
            custody,
        }
    }

    fn arguments(&self, custody: bool) -> Value {
        let mut value = json!({"root":self.repository,"allow_native_reads":true,
            "commit":self.report.commit,"expected_tree":self.report.output,
            "metadata_json":serde_json::to_string(&self.metadata).expect("exact recipe JSON")});
        if custody {
            value["custody_id"] = json!(self.custody);
        }
        value
    }
}

#[test]
fn generated_materialization_matches_sdk_and_rejects_exact_pin_binding_and_byte_changes() {
    let fixture = Generated::new();
    let name = "verify_generated_tree";
    let before = snapshot(fixture.root.path());
    let root = fixture.stage.path().join("project");
    let verified =
        source::verify_generated_tree(&root, &fixture.binding, fixture.pin, Default::default())
            .expect("independent SDK materialization verification");
    let response = call(name, fixture.arguments());
    assert_eq!(result(&response), &json!(verified.inspect()));
    assert_eq!(
        result(&response)["inventory"]["entries"]
            .as_array()
            .expect("source entries")
            .len(),
        1
    );
    assert_eq!(
        result(&response)["inventory"]["entries"][0]["path"],
        "page.md"
    );
    checked(name, &response);
    let mut pin = fixture.arguments();
    pin["manifest_sha256"] = json!(source::Sha256Digest::of(b"wrong independent pin"));
    failed(name, pin);
    let mut binding = fixture.arguments();
    binding["expected_binding"]["site"] = json!("site:other");
    failed(name, binding);
    assert_eq!(snapshot(fixture.root.path()), before);
    fs::write(root.join("page.md"), "Changed fixture bytes\n").expect("deliberate test corruption");
    let corrupt = snapshot(fixture.root.path());
    let expected =
        source::verify_generated_tree(&root, &fixture.binding, fixture.pin, Default::default())
            .expect_err("native content mismatch");
    assert_eq!(failed(name, fixture.arguments())["code"], expected.code());
    assert_eq!(
        snapshot(fixture.root.path()),
        corrupt,
        "inspection never repairs bytes"
    );
}

#[test]
fn native_commit_and_custody_reports_match_sdk_without_changing_objects_or_references() {
    let fixture = Commit::new();
    let before = snapshot(fixture.root.path());
    let verified = release_commit::verify(
        &fixture.repository,
        &fixture.report.commit,
        fixture.report.output,
        &fixture.metadata,
        Default::default(),
    )
    .expect("SDK commit verifier");
    let held = release_commit::inspect_custody(
        &fixture.repository,
        fixture.custody,
        &fixture.report.commit,
        fixture.report.output,
        &fixture.metadata,
        Default::default(),
    )
    .expect("SDK exact custody inspector");
    for (name, args, expected) in [
        (
            "verify_release_commit",
            fixture.arguments(false),
            json!(verified),
        ),
        (
            "inspect_release_custody",
            fixture.arguments(true),
            json!(held),
        ),
    ] {
        let response = call(name, args.clone());
        assert_eq!(result(&response), &expected);
        checked(name, &response);
        assert_eq!(
            result(&call(name, args)),
            &expected,
            "read-only repeat is exact"
        );
        assert!(!response.to_string().contains("PRIVATE"));
        let catalog = runtime().block_on(exchange(
            request("tools/list", json!({})),
            Default::default(),
        ));
        let schema = &catalog["result"]["tools"]
            .as_array()
            .expect("catalog")
            .iter()
            .find(|tool| tool["name"] == name)
            .expect("descriptor")["inputSchema"]["properties"]["metadata_json"]["contentSchema"];
        let validator = jsonschema::validator_for(schema).expect("native embedded metadata schema");
        let metadata = json!(fixture.metadata);
        assert!(
            validator.is_valid(&metadata),
            "explicit parent:null is discoverable"
        );
        let mut absent = metadata.clone();
        absent
            .as_object_mut()
            .expect("recipe object")
            .remove("parent");
        assert!(
            !validator.is_valid(&absent),
            "parent omission is not root-commit intent"
        );
        let mut tuple = metadata;
        tuple["author"] = json!(["Fixture", "fixture@example.invalid", 100, 0]);
        assert!(
            !validator.is_valid(&tuple),
            "signature requires named fields"
        );
    }
    assert_eq!(snapshot(fixture.root.path()), before);
}

#[test]
fn denied_reads_and_malformed_nested_inputs_never_touch_selected_missing_paths() {
    let fixture = Commit::new();
    let generated = Generated::new();
    let before = (
        snapshot(fixture.root.path()),
        snapshot(generated.root.path()),
    );
    let missing = fixture.root.path().join("missing-native-root");
    for (name, args) in [
        ("verify_release_commit", fixture.arguments(false)),
        ("inspect_release_custody", fixture.arguments(true)),
        ("verify_generated_tree", generated.arguments()),
    ] {
        let mut denied = args.clone();
        denied["root"] = json!(missing);
        denied["allow_native_reads"] = json!(false);
        assert_eq!(
            call(name, denied)["error"]["code"],
            -32602,
            "permission precedes native access"
        );
        let mut malformed = args;
        malformed["root"] = json!(missing);
        malformed["limits"] = json!([]);
        assert_eq!(
            call(name, malformed)["error"]["code"],
            -32602,
            "named limits only"
        );
    }
    let mut absent = json!(fixture.metadata);
    absent
        .as_object_mut()
        .expect("recipe object")
        .remove("parent");
    let native_missing = release_commit::verify(
        &missing,
        &fixture.report.commit,
        fixture.report.output,
        &fixture.metadata,
        Default::default(),
    )
    .expect_err("valid recipe reaches the independently missing repository")
    .to_string();
    for metadata in [
        "PRIVATE malformed".to_owned(),
        absent.to_string(),
        serde_json::to_string(&fixture.metadata).expect("recipe").replacen("{","{\"message\":\"duplicate\",",1),
        serde_json::to_string(&fixture.metadata).expect("recipe").replacen("\"name\":","\"name\":\"duplicate\",\"na\\u006de\":",1),
        json!({"parent":null,"author":[],"committer":fixture.metadata.committer,"message":"PRIVATE"}).to_string(),
    ] {
        let expected = release_commit::CommitMetadata::parse_json(metadata.as_bytes(),16 * 1024)
            .expect_err("native exact recipe parser rejects this input").reason.to_owned();
        assert_ne!(expected,native_missing,"fixture distinguishes parsing from native access");
        for (name,custody) in [("verify_release_commit",false),("inspect_release_custody",true)] {
            let mut args = fixture.arguments(custody);
            args["root"] = json!(missing);
            args["metadata_json"] = json!(metadata);
            let failure = failed(name,args);
            assert_eq!(failure["code"],"commit_metadata_input","metadata refuses before missing repository");
            assert_eq!(failure["message"],expected,"exact native parse refusal precedes any repository failure");
            assert!(!failure.to_string().contains("PRIVATE"));
        }
    }
    assert!(!missing.exists());
    assert_eq!(
        (
            snapshot(fixture.root.path()),
            snapshot(generated.root.path())
        ),
        before
    );
}

#[test]
fn changed_recipe_tree_and_custody_reference_are_refused_without_native_repair() {
    let fixture = Commit::new();
    let before = snapshot(fixture.root.path());
    for (name, custody) in [
        ("verify_release_commit", false),
        ("inspect_release_custody", true),
    ] {
        let mut recipe = fixture.arguments(custody);
        let mut changed = fixture.metadata.clone();
        changed.message.push_str("changed");
        recipe["metadata_json"] = json!(serde_json::to_string(&changed).expect("changed recipe"));
        failed(name, recipe);
        let mut tree = fixture.arguments(custody);
        tree["expected_tree"] = json!(snapshot(&fixture.repository));
        failed(name, tree);
    }
    assert_eq!(snapshot(fixture.root.path()), before);
    let repo = Repository::open_bare(&fixture.repository).expect("fixture custody repository");
    repo.find_reference(&release_commit::custody_reference(fixture.custody))
        .expect("actual retained ref")
        .delete()
        .expect("deliberately remove owned custody ref");
    let missing = snapshot(fixture.root.path());
    failed("inspect_release_custody", fixture.arguments(true));
    result(&call("verify_release_commit", fixture.arguments(false)));
    assert_eq!(
        snapshot(fixture.root.path()),
        missing,
        "absent custody is never recreated"
    );
    let reference = release_commit::custody_reference(fixture.custody);
    repo.reference_symbolic(
        &reference,
        "refs/heads/absent-fixture",
        false,
        "fixture only",
    )
    .expect("deliberate symbolic custody conflict");
    let symbolic = snapshot(fixture.root.path());
    failed("inspect_release_custody", fixture.arguments(true));
    assert_eq!(snapshot(fixture.root.path()), symbolic);
    repo.find_reference(&reference)
        .expect("fixture symbolic reference")
        .delete()
        .expect("remove only the fixture symbolic conflict");
    let commit = repo
        .find_commit(git2::Oid::from_str(fixture.report.commit.as_str()).expect("exact OID"))
        .expect("real retained commit");
    repo.reference(&reference, commit.tree_id(), false, "fixture only")
        .expect("deliberately conflicting direct custody target");
    let conflicting = snapshot(fixture.root.path());
    failed("inspect_release_custody", fixture.arguments(true));
    assert_eq!(snapshot(fixture.root.path()), conflicting);
}

#[test]
fn native_and_transport_bounds_return_no_successful_prefix_or_mutation() {
    let fixture = Commit::new();
    let generated = Generated::new();
    let before = (
        snapshot(fixture.root.path()),
        snapshot(generated.root.path()),
    );
    for (name, mut args) in [
        ("verify_release_commit", fixture.arguments(false)),
        ("inspect_release_custody", fixture.arguments(true)),
    ] {
        let limits = source::RevisionLimits {
            metadata_bytes: 0,
            ..Default::default()
        };
        args["limits"] = json!(limits);
        failed(name, args);
        let mut metadata = fixture.arguments(name == "inspect_release_custody");
        let exact = metadata["metadata_json"]
            .as_str()
            .expect("recipe string")
            .len();
        metadata["max_metadata_bytes"] = json!(exact);
        result(&call(name, metadata.clone()));
        metadata["max_metadata_bytes"] = json!(exact - 1);
        assert_eq!(failed(name, metadata)["code"], "commit_metadata_input");
    }
    let mut args = generated.arguments();
    let mut limits = source::Limits {
        entries: 2,
        ..Default::default()
    };
    args["limits"] = json!(limits);
    result(&call("verify_generated_tree", args.clone()));
    limits.entries = 1;
    args["limits"] = json!(limits);
    failed("verify_generated_tree", args); // The excluded marker still consumes capture budget.
    for (name, args) in [
        ("verify_release_commit", fixture.arguments(false)),
        ("inspect_release_custody", fixture.arguments(true)),
        ("verify_generated_tree", generated.arguments()),
    ] {
        let response = runtime().block_on(exchange(
            request("tools/call", json!({"name":name,"arguments":args})),
            server::Limits {
                output_bytes: 1,
                ..Default::default()
            },
        ));
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(
            checked_schema(name, &response),
            json!({"code":"mcp_output_limit"})
        );
        assert_eq!(
            response["result"]["content"],
            json!([{
                "type": "text",
                "text": "Gazette result exceeds the MCP output ceiling; narrow the request or use the direct SDK."
            }]),
            "bounded failure contains only actionable guidance, never a result prefix"
        );
    }
    assert_eq!(
        (
            snapshot(fixture.root.path()),
            snapshot(generated.root.path())
        ),
        before
    );
}

#[cfg(unix)]
#[test]
fn corrupt_native_commit_object_refuses_both_tools_without_replacing_the_object() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = Commit::new();
    let (directory, file) = fixture.report.commit.as_str().split_at(2);
    let object = fixture
        .repository
        .join("objects")
        .join(directory)
        .join(file);
    assert!(
        object.is_file(),
        "native fixture created this exact loose commit"
    );
    fs::set_permissions(&object, fs::Permissions::from_mode(0o600))
        .expect("permit corruption of this disposable object only");
    fs::write(&object, b"corrupt owned commit object").expect("actual object corruption");
    let before = snapshot(fixture.root.path());
    failed("verify_release_commit", fixture.arguments(false));
    failed("inspect_release_custody", fixture.arguments(true));
    assert_eq!(snapshot(fixture.root.path()), before);
}
