//! Actual SDK source acquisition and write-free complete staging checks.

use super::*;
use git2::{Repository, Signature};
use serde_json::json;
use std::{fs, path::Path};
use tempfile::{TempDir, tempdir};

struct Fixture {
    root: TempDir,
    arguments: Value,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("fixture");
        for directory in ["authored", "parent", "repo", "repo/docs"] {
            fs::create_dir(root.path().join(directory)).expect("fixture directory");
        }
        fs::write(root.path().join("authored/home.md"), "# Authored\n").expect("authored");
        fs::write(root.path().join("parent/existing"), "untouched").expect("existing sibling");
        fs::write(root.path().join("repo/docs/page.md"), "# Pinned 🦀\n").expect("project");
        let repo = Repository::init(root.path().join("repo")).expect("disposable repository");
        let mut index = repo.index().expect("index");
        index
            .add_path(Path::new("docs/page.md"))
            .expect("fixture entry");
        index.write().expect("fixture index");
        let tree = repo
            .find_tree(index.write_tree().expect("fixture tree"))
            .expect("tree");
        let signature = Signature::now("fixture", "fixture@example.invalid").expect("signature");
        let commit = repo
            .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
            .expect("fixture commit");
        let limits = source::Limits::default();
        let preparation = PreparationLimits {
            sources: 128,
            entries: limits.entries,
            bytes: limits.total_bytes,
            per_source: source::RevisionLimits::default(),
        };
        let arguments = json!({"allow_native_reads":true,"authored":root.path().join("authored"),"parent":root.path().join("parent"),"workspace":"workspace:one","site":"site:one","placements":[{"target":"docs/project","project":"project:one","deployment":"deployment:one","source":{"kind":"repository","root":root.path().join("repo"),"commit":commit.to_string(),"subdirectory":"docs"}}],"preparation":preparation,"limits":limits});
        Self { root, arguments }
    }

    fn unchanged(&self) {
        assert_eq!(
            fs::read_to_string(self.root.path().join("parent/existing")).expect("retained sibling"),
            "untouched"
        );
        assert_eq!(
            fs::read_dir(self.root.path().join("parent"))
                .expect("parent")
                .count(),
            1,
            "no stage or receipt created"
        );
        assert_eq!(
            fs::read_to_string(self.root.path().join("authored/home.md")).expect("authored bytes"),
            "# Authored\n"
        );
    }
}

fn call(arguments: Value) -> CallToolResult {
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    assert!(
        jsonschema::validator_for(&descriptor["inputSchema"])
            .expect("input schema")
            .is_valid(&arguments)
    );
    let result = decode(arguments.as_object().expect("named arguments").clone())
        .expect("arguments")
        .execute(1024 * 1024)
        .expect("tool response");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("output schema")
        .validate(
            result
                .structured_content
                .as_ref()
                .expect("structured response"),
        )
        .expect("actual output conforms");
    result
}

#[test]
fn checked_inventory_matches_sdk_and_no_stage_or_source_bytes_change() {
    let fixture = Fixture::new();
    let config_before = fs::read(fixture.root.path().join("repo/.git/config")).expect("config");
    let index_before = fs::read(fixture.root.path().join("repo/.git/index")).expect("index");
    let head_before = fs::read(fixture.root.path().join("repo/.git/HEAD")).expect("HEAD");
    let request: Request =
        serde_json::from_value(fixture.arguments.clone()).expect("same shared input");
    let (context, placements) =
        input::bind(request.workspace, request.site, request.placements).expect("bind");
    let prepared = source::prepare(placements, request.preparation).expect("native acquisition");
    let inventory = source::check_stage(
        &prepared,
        &context,
        &request.authored,
        &request.parent,
        request.limits,
    )
    .expect("SDK check");
    let result = call(fixture.arguments.clone());
    assert_ne!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content.expect("report"),
        json!({"status":"checked","workspace":"workspace:one","site":"site:one","inventory":inventory})
    );
    assert_eq!(
        fs::read(fixture.root.path().join("repo/.git/config")).expect("config after"),
        config_before
    );
    assert_eq!(
        fs::read(fixture.root.path().join("repo/.git/index")).expect("index after"),
        index_before
    );
    assert_eq!(
        fs::read(fixture.root.path().join("repo/.git/HEAD")).expect("HEAD after"),
        head_before
    );
    fixture.unchanged();
    let mut authored_only = fixture.arguments.clone();
    authored_only["placements"] = json!([]);
    assert_ne!(call(authored_only).is_error, Some(true));
    fixture.unchanged();
}

#[test]
fn authored_only_still_validates_whole_context_before_native_reads() {
    let fixture = Fixture::new();
    for identity in ["workspace", "site"] {
        let mut arguments = fixture.arguments.clone();
        arguments["placements"] = json!([]);
        arguments[identity] = json!("");
        arguments["authored"] = json!(fixture.root.path().join("not-created"));
        let result = call(arguments);
        assert_eq!(result.is_error, Some(true));
        assert!(
            result.structured_content.expect("native failure")["message"]
                .as_str()
                .expect("message")
                .contains("identity must be explicit")
        );
        fixture.unchanged();
    }
}

#[test]
fn explicit_reads_complete_placement_set_and_named_limit_shapes_are_required() {
    let fixture = Fixture::new();
    let mut invalid = Vec::new();
    for (pointer, value) in [
        ("/allow_native_reads", json!(false)),
        ("/authored", json!("relative")),
        ("/parent", json!("relative")),
        ("/placements/0/source/root", json!("relative")),
        ("/placements/0/source/commit", json!("HEAD")),
        ("/placements/0/source/subdirectory", json!("../escape")),
        (
            "/placements/0",
            json!([
                "docs/project",
                "project:one",
                "deployment:one",
                fixture.arguments["placements"][0]["source"]
            ]),
        ),
        ("/preparation", json!([128, 100, 10000, {}])),
        ("/preparation/per_source", json!([{}, 10000])),
        ("/preparation/per_source/content", json!([1, 2, 3, 4])),
        ("/limits", json!([1, 2, 3, 4])),
        ("/preparation/sources", json!(0)),
    ] {
        let mut changed = fixture.arguments.clone();
        *changed.pointer_mut(pointer).expect("actual field") = value;
        invalid.push(changed);
    }
    let mut duplicate = fixture.arguments.clone();
    duplicate["placements"]
        .as_array_mut()
        .expect("placements")
        .push(fixture.arguments["placements"][0].clone());
    invalid.push(duplicate);
    let mut missing = fixture.arguments.clone();
    missing
        .as_object_mut()
        .expect("request")
        .remove("placements");
    invalid.push(missing);
    let mut extra = fixture.arguments.clone();
    extra["create_stage"] = json!(true);
    invalid.push(extra);
    for input in invalid {
        assert!(
            decode(input.as_object().expect("object").clone()).is_err(),
            "{input}"
        );
    }
    fixture.unchanged();
}

#[test]
fn independent_native_budgets_and_output_budget_never_return_partial_success() {
    let fixture = Fixture::new();
    for pointer in [
        "/preparation/entries",
        "/preparation/bytes",
        "/preparation/per_source/metadata_bytes",
        "/preparation/per_source/content/file_bytes",
        "/preparation/per_source/content/depth",
        "/limits/entries",
        "/limits/total_bytes",
        "/limits/file_bytes",
    ] {
        let mut changed = fixture.arguments.clone();
        *changed.pointer_mut(pointer).expect("native bound") = json!(0);
        let result = call(changed);
        assert_eq!(result.is_error, Some(true), "{pointer}");
        assert!(
            result
                .structured_content
                .expect("failure")
                .get("inventory")
                .is_none()
        );
        fixture.unchanged();
    }
    let result = decode(fixture.arguments.as_object().expect("arguments").clone())
        .expect("decode")
        .execute(1)
        .expect("bounded output failure");
    assert_eq!(result.is_error, Some(true));
    assert_eq!(
        result.structured_content.expect("failure")["code"],
        "mcp_output_limit"
    );
    fixture.unchanged();
}

#[test]
fn pin_mismatch_collision_and_native_parent_overlap_fail_the_complete_check() {
    let fixture = Fixture::new();
    let mut overlapping = fixture.arguments.clone();
    overlapping["parent"] = json!(fixture.root.path().join("repo"));
    assert_eq!(call(overlapping).is_error, Some(true));
    fs::create_dir_all(fixture.root.path().join("authored/docs/project"))
        .expect("conflicting directory");
    fs::write(
        fixture.root.path().join("authored/docs/project/page.md"),
        "different",
    )
    .expect("collision");
    assert_eq!(call(fixture.arguments.clone()).is_error, Some(true));
    fixture.unchanged();
    fs::write(
        fixture.root.path().join("repo/docs/page.md"),
        "unpinned bytes",
    )
    .expect("dirty fixture");
    let failed = call(fixture.arguments.clone());
    assert_eq!(failed.is_error, Some(true));
    assert!(
        failed.structured_content.expect("failure")["message"]
            .as_str()
            .expect("message")
            .contains("cannot prepare")
    );
    fixture.unchanged();
}
