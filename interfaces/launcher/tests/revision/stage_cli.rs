//! Direct executable check/create parity, retained bytes and honest effect failures.

use gazette_sdk::source::{self, Limits, PlacementRequest, SourceSelection};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use tempfile::tempdir;

use super::{Fixture, ParentFixture, context, request};

fn command(mode: &str, authored: &Path, parent: &Path, selections: &[Value]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command.arg(mode).arg(authored).arg(parent).args([
        "--workspace",
        "fixture-workspace",
        "--site",
        "fixture-site",
    ]);
    for selection in selections {
        command.arg("--placement").arg(selection.to_string());
    }
    command
}

fn selection(request: &PlacementRequest) -> Value {
    let source = match &request.source {
        SourceSelection::Filesystem { root, subdirectory } => json!({
            "kind": "filesystem", "root": root,
            "subdirectory": subdirectory.as_ref().map(ToString::to_string),
        }),
        SourceSelection::FilteredFilesystem { root, exclude } => json!({
            "kind": "filtered-filesystem", "root": root, "exclude": exclude,
        }),
        SourceSelection::IncludedFilesystem {
            root,
            include,
            expected_branch,
        } => json!({
            "kind": "included_filesystem", "root": root, "include": include,
            "expected_branch": expected_branch,
        }),
        SourceSelection::WorkingRepository {
            root,
            subdirectory,
            expected_branch,
        } => json!({
            "kind": "working_repository", "root": root,
            "subdirectory": subdirectory.as_ref().map(ToString::to_string), "expected_branch": expected_branch,
        }),
        SourceSelection::Repository {
            root,
            commit,
            subdirectory,
        } => json!({
            "kind": "repository", "root": root, "commit": commit.to_string(),
            "subdirectory": subdirectory.to_string(),
        }),
        SourceSelection::Submodule {
            parent,
            parent_commit,
            submodule_path,
            subdirectory,
        } => json!({
            "kind": "submodule", "parent": parent, "parent_commit": parent_commit.to_string(),
            "submodule_path": submodule_path.to_string(), "subdirectory": subdirectory.to_string(),
        }),
    };
    json!({"target": request.target.to_string(),
        "project": format!("fixture-project:{}", request.target),
        "deployment": format!("fixture-deployment:{}", request.target), "source": source})
}

fn report(output: Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("complete JSON receipt")
}

fn children(path: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(path)
        .expect("parent")
        .map(|entry| entry.expect("entry").path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn mixed_cli_check_create_and_independent_verification_share_exact_sdk_inventory() {
    let repo = Fixture::new();
    let parent = ParentFixture::new();
    let filesystem = tempdir().expect("filesystem source");
    let authored = tempdir().expect("authored");
    let output = tempdir().expect("staging");
    fs::write(authored.path().join("index.md"), b"authored").expect("authored page");
    fs::write(filesystem.path().join("readme.md"), b"abc").expect("filesystem source bytes");
    fs::write(output.path().join("unrelated"), b"preserve").expect("unrelated");
    let requests = vec![
        request("projects/repo", &repo),
        PlacementRequest {
            target: "projects/child".parse().expect("target"),
            source: SourceSelection::Submodule {
                parent: parent.root.path().to_owned(),
                parent_commit: parent.parent.to_string().parse().expect("parent pin"),
                submodule_path: "packages/tool".parse().expect("gitlink"),
                subdirectory: "docs".parse().expect("docs"),
            },
        },
        PlacementRequest {
            target: "projects/filesystem".parse().expect("target"),
            source: SourceSelection::Filesystem {
                root: filesystem.path().to_owned(),
                subdirectory: None,
            },
        },
    ];
    let mut selections: Vec<_> = requests.iter().map(selection).collect();
    let prepared = source::prepare(
        requests,
        source::PreparationLimits {
            sources: 3,
            entries: 3,
            bytes: 9,
            per_source: source::RevisionLimits::default(),
        },
    )
    .expect("SDK preparation");
    let context = context(&prepared);
    let expected = source::check_stage(
        &prepared,
        &context,
        authored.path(),
        output.path(),
        Limits::default(),
    )
    .expect("SDK check");
    let snapshots: Vec<_> = [
        repo.root.path(),
        parent.root.path(),
        filesystem.path(),
        authored.path(),
    ]
    .into_iter()
    .map(|path| source::inspect(path, None, Limits::default()).expect("before"))
    .collect();
    let checked = report(
        command("check-stage", authored.path(), output.path(), &selections)
            .output()
            .expect("check"),
    );
    assert_eq!(checked["status"], "checked");
    assert!(checked.get("directory").is_none());
    assert_eq!(
        checked["inventory"],
        serde_json::to_value(expected).expect("expected JSON")
    );
    assert_eq!(
        children(output.path()),
        vec![output.path().join("unrelated")]
    );
    selections.reverse();
    let retained = report(
        command("stage", authored.path(), output.path(), &selections)
            .arg("--create-stage")
            .output()
            .expect("create"),
    );
    assert_eq!(retained["status"], "retained");
    assert_eq!(retained["workspace"], "fixture-workspace");
    assert_eq!(retained["site"], "fixture-site");
    assert_eq!(retained["inventory"], checked["inventory"]);
    let directory = Path::new(retained["directory"].as_str().expect("retained locator"));
    assert_eq!(
        directory.parent(),
        Some(output.path().canonicalize().expect("parent").as_path())
    );
    let readback = report(
        Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-source")
            .arg(directory)
            .output()
            .expect("fresh readback process"),
    );
    assert_eq!(readback, checked["inventory"]);
    for placement in prepared.placements() {
        let target = &placement.request().target;
        let binding = context.binding(target).expect("binding");
        let manifest = format!("{target}/{}", source::GENERATED_TREE_MANIFEST);
        let pin = checked["inventory"]["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .find(|entry| entry["path"] == manifest)
            .expect("independent check evidence")["sha256"]
            .as_str()
            .expect("manifest pin");
        let verified = report(
            Command::new(env!("CARGO_BIN_EXE_gazette"))
                .arg("verify-generated-tree")
                .arg(directory.join(target.as_str()))
                .arg("--expected-binding")
                .arg(serde_json::to_string(&binding).expect("binding JSON"))
                .args(["--manifest-sha256", pin])
                .output()
                .expect("independent verifier"),
        );
        assert_eq!(
            verified["inventory"],
            serde_json::to_value(placement.source().inventory()).expect("source JSON")
        );
    }
    for (path, before) in [
        repo.root.path(),
        parent.root.path(),
        filesystem.path(),
        authored.path(),
    ]
    .into_iter()
    .zip(snapshots)
    {
        assert_eq!(
            source::inspect(path, None, Limits::default()).expect("after"),
            before
        );
    }
    let later = report(
        command("stage", authored.path(), output.path(), &selections)
            .arg("--create-stage")
            .output()
            .expect("second fresh stage"),
    );
    assert_ne!(later["directory"], retained["directory"]);
    assert_eq!(later["inventory"], retained["inventory"]);
    assert!(
        directory.is_dir(),
        "later invocation did not adopt/delete prior output"
    );
    assert_eq!(
        fs::read(output.path().join("unrelated")).expect("preserved"),
        b"preserve"
    );
}

#[test]
fn missing_selection_or_creation_permission_never_starts_native_effects() {
    let authored = tempdir().expect("authored");
    let parent = tempdir().expect("parent");
    for (mode, flags) in [
        ("stage", vec!["--authored-only"]),
        ("stage", vec!["--create-stage"]),
        ("check-stage", vec![]),
        ("check-stage", vec!["--authored-only", "--create-stage"]),
    ] {
        let output = command(mode, authored.path(), parent.path(), &[])
            .args(flags)
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(children(parent.path()).is_empty());
    }
    fs::write(authored.path().join("page.md"), b"authored only").expect("page");
    let checked = report(
        command("check-stage", authored.path(), parent.path(), &[])
            .arg("--authored-only")
            .output()
            .expect("check"),
    );
    let retained = report(
        command("stage", authored.path(), parent.path(), &[])
            .args(["--authored-only", "--create-stage"])
            .output()
            .expect("stage"),
    );
    assert_eq!(checked["inventory"], retained["inventory"]);
    assert_eq!(
        retained["inventory"]["entries"]
            .as_array()
            .expect("entries")
            .len(),
        1
    );
}

#[test]
fn malformed_unknown_and_incomplete_placements_are_not_silently_ignored() {
    let repo = Fixture::new();
    let authored = tempdir().expect("authored");
    let parent = tempdir().expect("parent");
    let original = selection(&request("project", &repo));
    for mutation in [
        "unknown",
        "missing",
        "source unknown",
        "wrong kind",
        "bad pin",
        "escape",
        "duplicate key",
        "oversized",
    ] {
        let mut value = original.clone();
        match mutation {
            "unknown" => value["optional"] = json!(true),
            "missing" => {
                value.as_object_mut().expect("object").remove("project");
            }
            "source unknown" => value["source"]["fetch"] = json!(true),
            "wrong kind" => value["source"]["kind"] = json!("github"),
            "bad pin" => value["source"]["commit"] = json!("main"),
            "escape" => value["target"] = json!("../escape"),
            _ => (),
        }
        let argument = match mutation {
            "duplicate key" => format!("{{\"target\":\"other\",{}", &value.to_string()[1..]),
            "oversized" => " ".repeat(16 * 1024 + 1),
            _ => value.to_string(),
        };
        let output = command("stage", authored.path(), parent.path(), &[])
            .args(["--create-stage", "--placement", &argument])
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(2), "{mutation}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(children(parent.path()).is_empty());
    }
}

#[test]
fn late_bad_selection_collisions_and_bounds_never_yield_a_partial_stage() {
    let repo = Fixture::new();
    let authored = tempdir().expect("authored");
    let parent = tempdir().expect("parent");
    let first = selection(&request("a", &repo));
    for case in [
        "missing",
        "duplicate",
        "ancestor",
        "identity",
        "source count",
        "entry count",
        "bytes",
        "metadata",
        "output overlap",
    ] {
        let mut second = selection(&request("z", &repo));
        let flags = match case {
            "missing" => {
                second["source"]["root"] = json!(repo.root.path().join("absent"));
                vec![]
            }
            "duplicate" => {
                second["target"] = json!("a");
                vec![]
            }
            "ancestor" => {
                second["target"] = json!("a/child");
                vec![]
            }
            "identity" => {
                second["project"] = json!("");
                vec![]
            }
            "source count" => vec!["--max-sources", "1"],
            "entry count" => vec!["--max-entries", "1"],
            "bytes" => vec!["--max-total-bytes", "1"],
            "metadata" => vec!["--max-git-metadata-bytes", "1"],
            _ => vec![],
        };
        let selected_parent = if case == "output overlap" {
            authored.path()
        } else {
            parent.path()
        };
        for mode in ["check-stage", "stage"] {
            let mut call = command(
                mode,
                authored.path(),
                selected_parent,
                &[first.clone(), second.clone()],
            );
            call.args(&flags);
            if mode == "stage" {
                call.arg("--create-stage");
            }
            let output = call.output().expect("CLI");
            assert_eq!(output.status.code(), Some(1), "{case}/{mode}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(children(parent.path()).is_empty());
            assert!(children(authored.path()).is_empty());
        }
    }
}

#[test]
fn staging_help_discloses_effects_without_requiring_inputs_or_creating_files() {
    let root = tempdir().expect("empty working directory");
    for (mode, effect) in [
        ("check-stage", "no writes, certification or release"),
        ("stage", "no replacement, certification or release"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .args([mode, "--help"])
            .current_dir(root.path())
            .output()
            .expect("help");
        assert!(output.status.success(), "{output:?}");
        let help = String::from_utf8(output.stdout).expect("help text");
        assert!(help.contains(effect), "{help}");
        for option in [
            "--workspace",
            "--site",
            "--placement",
            "--authored-only",
            "--max-sources",
        ] {
            assert!(help.contains(option), "{help}");
        }
        assert_eq!(help.contains("--create-stage"), mode == "stage");
        assert!(output.stderr.is_empty());
        assert!(children(root.path()).is_empty());
    }
}

#[test]
fn broken_receipt_pipe_fails_without_erasing_a_retained_stage() {
    let repo = Fixture::new();
    let authored = tempdir().expect("authored");
    let parent = tempdir().expect("parent");
    let selections = [selection(&request("project", &repo))];
    let checked = report(
        command("check-stage", authored.path(), parent.path(), &selections)
            .output()
            .expect("check"),
    );
    let mut child = command("stage", authored.path(), parent.path(), &selections)
        .arg("--create-stage")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("terminal stage process");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("receipt delivery failed"), "{error}");
    assert!(error.contains("no cleanup attempted"), "{error}");
    let directories = children(parent.path());
    assert_eq!(directories.len(), 1);
    assert!(
        error.contains(
            directories[0]
                .file_name()
                .expect("name")
                .to_str()
                .expect("UTF8")
        )
    );
    assert_eq!(
        serde_json::to_value(
            source::inspect(&directories[0], None, Limits::default())
                .expect("independent readback")
        )
        .expect("inventory"),
        checked["inventory"]
    );
}

fn inspect_document(root: &Path, identity: &Value, document: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-markdown")
        .arg("--tree")
        .arg(root)
        .arg("--tree-identity")
        .arg(identity.to_string())
        .args(["--document", document]);
    command
}

#[test]
fn retained_stage_documents_compile_from_one_pinned_capture_with_sdk_cli_parity() {
    let repo = Fixture::new();
    let authored = tempdir().expect("authored");
    let parent = tempdir().expect("stage parent");
    let page =
        "+++\r\ntitle = 'not inferred authority'\r\n+++\r\n# Body é\r\n\r\n![asset](asset.bin)\r\n";
    fs::write(authored.path().join("page.md"), page).expect("page");
    fs::write(authored.path().join("asset.bin"), [0xff, 0, 0x80]).expect("binary asset");
    let staged = report(
        command(
            "stage",
            authored.path(),
            parent.path(),
            &[selection(&request("project", &repo))],
        )
        .arg("--create-stage")
        .output()
        .expect("stage"),
    );
    let root = Path::new(staged["directory"].as_str().expect("directory"));
    let identity = &staged["inventory"]["identity"];
    let capture = source::capture_pinned(
        root,
        serde_json::from_value(identity.clone()).expect("pin"),
        Limits::default(),
    )
    .expect("SDK captured tree");
    let path = "page.md".parse().expect("path");
    let parsed = gazette_sdk::document::ParsedDocument::parse(
        std::str::from_utf8(capture.file(&path).expect("retained page")).expect("UTF8"),
        gazette_sdk::document::Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("SDK parse");
    let output = report(
        inspect_document(root, identity, "page.md")
            .args(["--front-matter", "toml", "--html"])
            .output()
            .expect("CLI parse"),
    );
    assert_eq!(output["input"]["identity"], *identity);
    assert_eq!(output["input"]["document"], "page.md");
    assert_eq!(
        output["document"],
        serde_json::to_value(parsed.inspect().expect("inspection")).expect("JSON")
    );
    assert_eq!(output["html"], parsed.render_html().expect("HTML"));
    let native_doc = report(
        inspect_document(root, identity, "project/readme.md")
            .arg("--html")
            .output()
            .expect("native Markdown"),
    );
    assert_eq!(native_doc["html"], "<p>abc</p>\n");
    assert_eq!(native_doc["input"]["identity"], *identity);
    fs::write(root.join("page.md"), "changed after capture").expect("later native edit");
    assert_eq!(
        parsed.render_html().expect("same retained parse"),
        output["html"]
    );
    assert_eq!(
        capture.file(&path).expect("same captured bytes"),
        page.as_bytes()
    );
    assert!(matches!(
        source::capture_pinned(root, capture.inventory().identity(), Limits::default()),
        Err(source::Error::TreeMismatch { .. })
    ));
    assert_eq!(
        fs::read(authored.path().join("page.md")).expect("authored unchanged"),
        page.as_bytes()
    );
}

#[test]
fn pinned_document_refuses_changes_anywhere_in_the_tree_without_partial_output() {
    for change in [
        "document",
        "sibling",
        "hidden",
        "extra",
        "missing",
        "empty directory",
        "symlink",
    ] {
        let authored = tempdir().expect("authored");
        let parent = tempdir().expect("parent");
        for (name, bytes) in [
            ("page.md", b"# Kept".as_slice()),
            ("asset.bin", b"asset"),
            (".metadata", b"opaque"),
        ] {
            fs::write(authored.path().join(name), bytes).expect("source");
        }
        fs::create_dir(authored.path().join("empty")).expect("empty directory");
        let staged = report(
            command("stage", authored.path(), parent.path(), &[])
                .args(["--authored-only", "--create-stage"])
                .output()
                .expect("stage"),
        );
        let root = Path::new(staged["directory"].as_str().expect("directory"));
        let identity = &staged["inventory"]["identity"];
        match change {
            "document" => fs::write(root.join("page.md"), b"changed").expect("edit"),
            "sibling" => fs::write(root.join("asset.bin"), b"changed").expect("edit"),
            "hidden" => fs::write(root.join(".metadata"), b"changed").expect("edit"),
            "extra" => fs::write(root.join("extra"), b"extra").expect("extra"),
            "missing" => fs::remove_file(root.join("asset.bin")).expect("remove fixture asset"),
            "empty directory" => {
                fs::remove_dir(root.join("empty")).expect("remove fixture directory")
            }
            _ => std::os::unix::fs::symlink(authored.path().join("asset.bin"), root.join("link"))
                .expect("link"),
        }
        let before = children(root);
        let output = inspect_document(root, identity, "page.md")
            .arg("--html")
            .output()
            .expect("inspect changed tree");
        assert_eq!(output.status.code(), Some(1), "{change}: {output:?}");
        assert!(output.stdout.is_empty());
        let expected = if change == "symlink" {
            "source_unsupported"
        } else {
            "source_tree_mismatch"
        };
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "{change}: {output:?}"
        );
        assert_eq!(
            children(root),
            before,
            "verifier never repairs changed input"
        );
    }
}

#[test]
fn pinned_document_requires_exact_selection_and_obeys_tree_and_parser_limits() {
    let root = tempdir().expect("input tree");
    fs::write(root.path().join("page.md"), b"# Heading").expect("page");
    fs::write(root.path().join("binary"), [0xff]).expect("binary");
    fs::create_dir(root.path().join("directory")).expect("directory");
    let before = source::inspect(root.path(), None, Limits::default()).expect("inventory");
    let identity = serde_json::to_value(before.identity()).expect("identity");
    for (document, flags, code) in [
        ("missing", vec![], "markdown_document"),
        ("directory", vec![], "markdown_document"),
        ("binary", vec![], "markdown_input"),
        ("page.md", vec!["--max-tree-entries", "1"], "source_limit"),
        (
            "page.md",
            vec!["--max-tree-file-bytes", "1"],
            "source_limit",
        ),
        ("page.md", vec!["--max-tree-bytes", "1"], "source_limit"),
        ("page.md", vec!["--max-tree-depth", "0"], "source_limit"),
        ("page.md", vec!["--max-source-bytes", "1"], "markdown_limit"),
        (
            "page.md",
            vec!["--front-matter", "toml"],
            "document_framing",
        ),
    ] {
        let output = inspect_document(root.path(), &identity, document)
            .args(flags)
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{output:?}"
        );
    }
    for flags in [
        vec!["--tree", "missing"],
        vec!["--document", "page.md"],
        vec!["--max-tree-bytes", "1"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-markdown")
            .args(flags)
            .output()
            .expect("CLI");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let bad = json!({"format":"gazette.inventory.v999","sha256":identity["sha256"]});
    let output = inspect_document(Path::new("/nonexistent-gazette-fixture"), &bad, "page.md")
        .output()
        .expect("unsupported pin");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let output = inspect_document(root.path(), &identity, "../escape")
        .output()
        .expect("bad selector");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        source::inspect(root.path(), None, Limits::default()).expect("unchanged"),
        before
    );
}
