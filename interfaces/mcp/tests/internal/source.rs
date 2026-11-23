//! Real selected filesystem/Git reads and exact SDK projection parity.

use std::{fs, path::Path};

use git2::{Repository, Signature};
use serde_json::json;
use tempfile::tempdir;

use super::*;

fn call(route: Route, input: Value) -> CallToolResult {
    let result = route
        .decode(input.as_object().expect("named arguments").clone())
        .expect("admitted arguments")
        .execute(256 * 1024)
        .expect("tool response");
    assert_output(route, &result);
    result
}

fn assert_output(route: Route, result: &CallToolResult) {
    let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
    jsonschema::validator_for(&descriptor["outputSchema"])
        .expect("native result schema")
        .validate(
            result
                .structured_content
                .as_ref()
                .expect("structured result"),
        )
        .expect("native emitted result conforms to discovery");
}

fn value(result: CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "{result:?}");
    result
        .structured_content
        .expect("complete native projection")
}

fn observed(root: &Path) -> Value {
    json!({"root":root,"allow_native_reads":true,"selection":{"kind":"observation"}})
}

#[test]
fn complete_observation_matches_sdk_and_does_not_open_unselected_siblings() {
    let root = tempdir().expect("fixture");
    fs::create_dir(root.path().join("selected")).expect("directory");
    fs::write(root.path().join("selected/.hidden"), "exact 🦀 bytes").expect("file");
    fs::write(root.path().join("selected/z.md"), "# Page\n").expect("file");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/missing", root.path().join("unselected"))
        .expect("unselected link");
    let input = json!({"root":root.path(),"allow_native_reads":true,"selection":{"kind":"observation","subdirectory":"selected"}});
    let expected = source::inspect(
        root.path(),
        Some(&"selected".parse().expect("path")),
        source::Limits::default(),
    )
    .expect("SDK observation");
    assert_eq!(value(call(Route::Source, input)), json!(expected));
    assert_eq!(
        fs::read_to_string(root.path().join("selected/.hidden")).expect("unchanged"),
        "exact 🦀 bytes"
    );
    assert_eq!(
        fs::read_dir(root.path().join("selected"))
            .expect("directory")
            .count(),
        2
    );
}

fn seed(root: &Path) -> (Repository, source::CommitId) {
    fs::create_dir(root.join("docs")).expect("docs");
    fs::write(root.join("docs/page.md"), "# Pinned\n").expect("page");
    let repo = Repository::init(root).expect("disposable repository");
    let mut index = repo.index().expect("index");
    index
        .add_path(Path::new("docs/page.md"))
        .expect("add fixture");
    index.write().expect("index bytes");
    let id = index.write_tree().expect("tree");
    let tree = repo.find_tree(id).expect("tree object");
    let signature = Signature::now("fixture", "fixture@example.invalid").expect("signature");
    let commit = repo
        .commit(Some("HEAD"), &signature, &signature, "fixture", &tree, &[])
        .expect("fixture commit");
    drop(tree);
    (repo, commit.to_string().parse().expect("native commit"))
}

#[test]
fn exact_revision_matches_sdk_and_changed_bytes_never_fall_back_to_observation() {
    let root = tempdir().expect("fixture");
    let (_repo, commit) = seed(root.path());
    let input = json!({"root":root.path(),"allow_native_reads":true,"selection":{"kind":"revision","subdirectory":"docs","commit":commit}});
    let expected = source::verify(
        root.path(),
        &"docs".parse().expect("path"),
        &commit,
        source::RevisionLimits::default(),
    )
    .expect("SDK verification");
    assert_eq!(value(call(Route::Source, input.clone())), json!(expected));
    fs::write(root.path().join("docs/page.md"), "changed").expect("change disposable bytes");
    let expected = source::verify(
        root.path(),
        &"docs".parse().expect("path"),
        &commit,
        source::RevisionLimits::default(),
    )
    .expect_err("mismatch");
    let failed = call(Route::Source, input);
    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content.expect("failure")["code"],
        expected.code()
    );
}

#[test]
fn parent_gitlink_uses_the_selected_child_and_preserves_exact_native_evidence() {
    let parent = tempdir().expect("parent");
    let child = parent.path().join("child");
    fs::create_dir(&child).expect("child");
    let (_child_repo, child_commit) = seed(&child);
    let repo = Repository::init(parent.path()).expect("parent repo");
    let mut builder = repo.treebuilder(None).expect("tree builder");
    builder
        .insert(
            "child",
            git2::Oid::from_str(child_commit.as_str()).expect("oid"),
            0o160000,
        )
        .expect("gitlink");
    let tree = repo
        .find_tree(builder.write().expect("tree"))
        .expect("tree object");
    let signature = Signature::now("fixture", "fixture@example.invalid").expect("signature");
    let commit: source::CommitId = repo
        .commit(Some("HEAD"), &signature, &signature, "parent", &tree, &[])
        .expect("fixture commit")
        .to_string()
        .parse()
        .expect("commit");
    let input = json!({"root":parent.path(),"allow_native_reads":true,"selection":{"kind":"submodule","parent_commit":commit,"submodule_path":"child","subdirectory":"docs"}});
    let expected = source::verify_submodule(
        parent.path(),
        &commit,
        &"child".parse().expect("path"),
        &"docs".parse().expect("path"),
        source::RevisionLimits::default(),
    )
    .expect("SDK gitlink");
    assert_eq!(value(call(Route::Source, input.clone())), json!(expected));
    fs::rename(&child, parent.path().join("sibling")).expect("move disposable child");
    assert_eq!(call(Route::Source, input).is_error, Some(true));
}

#[test]
fn whole_toml_tree_matches_sdk_and_bad_hidden_file_rejects_the_complete_set() {
    let root = tempdir().expect("fixture");
    fs::write(root.path().join("a.toml"), "title='Rêve'\n").expect("TOML");
    fs::write(root.path().join(".hidden"), "enabled=true\n").expect("hidden TOML");
    let input = json!({"root":root.path(),"allow_native_reads":true});
    let expected = configuration::inspect_toml(root.path(), None, configuration::Limits::default())
        .expect("SDK complete set");
    assert_eq!(value(call(Route::Toml, input.clone())), json!(expected));
    fs::write(root.path().join(".hidden"), "invalid=[").expect("invalid fixture");
    let expected = configuration::inspect_toml(root.path(), None, configuration::Limits::default())
        .expect_err("whole set failure");
    let failed = call(Route::Toml, input);
    assert_eq!(failed.is_error, Some(true));
    let evidence = failed.structured_content.expect("typed error");
    assert_eq!(evidence["code"], expected.code());
    assert!(evidence.get("documents").is_none());
}

#[test]
fn permissions_path_selection_and_nested_shapes_fail_before_native_reads() {
    for input in [
        json!({"root":"/absent","allow_native_reads":false,"selection":{"kind":"observation"}}),
        json!({"root":"/absent","selection":{"kind":"observation"}}),
        json!({"root":"relative","allow_native_reads":true,"selection":{"kind":"observation"}}),
        json!({"root":"/absent","allow_native_reads":true,"selection":{"kind":"observation","subdirectory":"../escape"}}),
        json!({"root":"/absent","allow_native_reads":true,"selection":{"kind":"revision","subdirectory":"docs","commit":"HEAD"}}),
        json!({"root":"/absent","allow_native_reads":true,"selection":{"kind":"observation","limits":[1,2,3,4]}}),
        json!({"root":"/absent","allow_native_reads":true,"selection":{"kind":"observation","extra":true}}),
    ] {
        assert!(
            Route::Source
                .decode(input.as_object().expect("object").clone())
                .is_err(),
            "{input}"
        );
    }
    let invalid = json!({"root":"/absent","allow_native_reads":true,"limits":{"syntax":{"document":[1,2,3]}}});
    assert!(
        Route::Toml
            .decode(invalid.as_object().expect("object").clone())
            .is_err()
    );
}

#[test]
fn native_limits_links_and_output_ceiling_never_become_partial_success() {
    let root = tempdir().expect("fixture");
    fs::write(root.path().join("page"), "abcdef").expect("file");
    for limits in [
        json!({"entries":0}),
        json!({"depth":0}),
        json!({"file_bytes":5}),
        json!({"total_bytes":5}),
    ] {
        let mut input = observed(root.path());
        input["selection"]["limits"] = limits;
        assert_eq!(call(Route::Source, input).is_error, Some(true));
    }
    let input = observed(root.path());
    let result = Route::Source
        .decode(input.as_object().expect("object").clone())
        .expect("decode")
        .execute(1)
        .expect("bounded failure");
    assert_output(Route::Source, &result);
    assert_eq!(
        result.structured_content.expect("failure")["code"],
        "mcp_output_limit"
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("page", root.path().join("link")).expect("link");
        assert_eq!(call(Route::Source, input).is_error, Some(true));
    }
}

#[test]
fn output_schemas_reject_corrupt_inventory_and_toml_evidence() {
    let root = tempdir().expect("fixture");
    fs::write(root.path().join("page"), "title='One'\n").expect("source");
    let observation = value(call(Route::Source, observed(root.path())));
    let descriptor = serde_json::to_value(Route::Source.descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("source schema");
    for (pointer, wrong) in [
        ("/identity/format", json!("unrecognized")),
        ("/identity/sha256", json!("not-a-hash")),
        ("/entries/0/kind", json!("symlink")),
        ("/entries/0/sha256", json!([])),
        ("/entries/0/path", json!("../escape")),
    ] {
        let mut changed = observation.clone();
        *changed.pointer_mut(pointer).expect("real native field") = wrong;
        assert!(!schema.is_valid(&changed), "{pointer}");
    }
    let input = json!({"root":root.path(),"allow_native_reads":true});
    let toml = value(call(Route::Toml, input));
    let descriptor = serde_json::to_value(Route::Toml.descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("TOML schema");
    let mut changed = toml.clone();
    assert_eq!(changed["documents"][0]["path"], "page");
    changed["documents"][0]["syntax"]["source_sha256"] = json!("invalid");
    assert!(!schema.is_valid(&changed));
    let mut changed = toml;
    changed.as_object_mut().expect("report").remove("documents");
    assert!(!schema.is_valid(&changed));
}

#[test]
fn discovery_schemas_use_native_defaults_and_enforce_integer_ranges() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
        assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
        let validator =
            jsonschema::validator_for(&descriptor["inputSchema"]).expect("valid JSON Schema");
        let mut input = match route {
            Route::Source => {
                json!({"root":"/selected","allow_native_reads":true,"selection":{"kind":"observation","limits":{}}})
            }
            Route::Toml => json!({"root":"/selected","allow_native_reads":true,"limits":{}}),
        };
        assert!(validator.is_valid(&input), "{input}");
        assert!(
            route
                .decode(input.as_object().expect("object").clone())
                .is_ok()
        );
        let location = match route {
            Route::Source => "/selection/limits",
            Route::Toml => "/limits",
        };
        *input.pointer_mut(location).expect("limits") = json!([]);
        assert!(!validator.is_valid(&input));
        assert!(
            route
                .decode(input.as_object().expect("object").clone())
                .is_err()
        );
    }
    let descriptor = serde_json::to_value(Route::Source.descriptor()).expect("descriptor");
    let validator = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
    for key in ["entries", "depth", "file_bytes", "total_bytes"] {
        let mut input = json!({"root":"/selected","allow_native_reads":true,"selection":{"kind":"observation","limits":{}}});
        input["selection"]["limits"][key] = json!(u64::MAX);
        assert!(validator.is_valid(&input));
        assert!(
            Route::Source
                .decode(input.as_object().expect("object").clone())
                .is_ok()
        );
        input["selection"]["limits"][key] = json!(-1);
        assert!(!validator.is_valid(&input));
        assert!(
            Route::Source
                .decode(input.as_object().expect("object").clone())
                .is_err()
        );
    }
}
