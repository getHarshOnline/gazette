//! Working repository selection through the CLI and editor's shared discovery path.

use std::{fs, os::unix::fs::symlink, path::Path, process::Command};

use gazette_sdk::{configuration, source};
use git2::{Repository, Signature};
use serde_json::{Value, json};

fn fixture(root: &Path) -> Repository {
    let repo = Repository::init(root).unwrap();
    fs::create_dir_all(root.join("docs/ignored")).unwrap();
    fs::write(root.join("docs/guide.md"), "# Original\n").unwrap();
    fs::write(root.join("docs/ignored/kept.txt"), "tracked").unwrap();
    let mut index = repo.index().unwrap();
    for path in ["docs/guide.md", "docs/ignored/kept.txt"] {
        index.add_path(Path::new(path)).unwrap();
    }
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = Signature::now("Fixture", "fixture@example.invalid").unwrap();
    let commit = repo
        .commit(None, &signature, &signature, "fixture", &tree, &[])
        .unwrap();
    repo.reference("refs/heads/manuals", commit, true, "fixture")
        .unwrap();
    repo.set_head("refs/heads/manuals").unwrap();
    drop(tree);
    fs::write(root.join(".gitignore"), "ignored/\n*.tmp\n!keep.tmp\n").unwrap();
    fs::write(root.join("docs/guide.md"), "# Working edit\n").unwrap();
    fs::write(root.join("docs/image.png"), "new image").unwrap();
    fs::write(root.join("docs/keep.tmp"), "explicit exception").unwrap();
    fs::write(root.join("docs/drop.tmp"), "ignored").unwrap();
    symlink("missing", root.join("docs/ignored/tool")).unwrap();
    fs::create_dir_all(root.join("sibling-repository")).unwrap();
    symlink("missing", root.join("sibling-repository/private")).unwrap();
    repo
}

fn placement(root: &Path, branch: Option<&str>) -> Value {
    json!({
        "target": "manual", "project": "example", "deployment": "local",
        "source": {"kind": "working_repository", "root": root, "subdirectory": "docs", "expected_branch": branch}
    })
}

fn check(root: &Path, branch: Option<&str>) -> std::process::Output {
    let authored = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("check-stage")
        .arg(authored.path())
        .arg(destination.path())
        .args(["--workspace", "fixture", "--site", "site", "--placement"])
        .arg(placement(root, branch).to_string())
        .output()
        .unwrap()
}

#[test]
fn working_selection_keeps_edits_and_assets_and_omits_ignored_and_sibling_inputs() {
    let root = tempfile::tempdir().unwrap();
    let _repo = fixture(root.path());
    let authored = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("stage")
        .arg(authored.path())
        .arg(destination.path())
        .args(["--workspace", "fixture", "--site", "site", "--placement"])
        .arg(placement(root.path(), Some("manuals")).to_string())
        .arg("--create-stage")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let staged = Path::new(report["directory"].as_str().unwrap()).join("manual");
    for (path, expected) in [
        ("guide.md", "# Working edit\n"),
        ("image.png", "new image"),
        ("ignored/kept.txt", "tracked"),
        ("keep.tmp", "explicit exception"),
    ] {
        assert_eq!(fs::read_to_string(staged.join(path)).unwrap(), expected);
    }
    for path in [
        "drop.tmp",
        "ignored/tool",
        "sibling-repository",
        ".git",
        "docs",
    ] {
        assert!(!staged.join(path).exists(), "{path}");
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(staged.join(source::GENERATED_TREE_MANIFEST)).unwrap())
            .unwrap();
    assert_eq!(
        manifest["source"],
        json!({"kind":"working_repository", "subdirectory":"docs", "expected_branch":"manuals"})
    );

    fs::write(root.path().join("gazette.toml"), "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:manual'\nlocation={kind='working-repository',root='.',subdirectory='docs',expected_branch='manuals'}\n").unwrap();
    let loaded = configuration::load_workspace(
        root.path(),
        &"gazette.toml".parse().unwrap(),
        Default::default(),
    )
    .unwrap();
    let discovered = configuration::discover_sources(
        &loaded,
        [&"source:manual".parse().unwrap()],
        Default::default(),
    )
    .unwrap();
    let paths = discovered[0].listing.entries();
    assert!(paths.contains_key("image.png"));
    assert!(paths.contains_key("ignored/kept.txt"));
    assert!(!paths.contains_key("drop.tmp"));
    assert!(!paths.contains_key("ignored/tool"));
    assert_eq!(
        discovered[0].subdirectory.as_ref().unwrap().to_string(),
        "docs"
    );
    assert!(
        configuration::discover_sources(
            &loaded,
            [&"source:manual".parse().unwrap()],
            configuration::ListingLimits {
                path_bytes: 1,
                ..Default::default()
            }
        )
        .is_err()
    );

    let parsed = source::input::parse(&placement(root.path(), None).to_string()).unwrap();
    let (_, requests) = source::input::bind("fixture".into(), "site".into(), vec![parsed]).unwrap();
    assert!(
        source::prepare(
            requests,
            source::PreparationLimits {
                sources: 1,
                entries: 100,
                bytes: 1,
                per_source: Default::default(),
            }
        )
        .is_err(),
        "working captures retain their byte budget"
    );
}

#[test]
fn working_selection_refuses_unavailable_roots_branches_and_unsafe_selected_files() {
    let root = tempfile::tempdir().unwrap();
    let repo = fixture(root.path());
    assert!(check(root.path(), None).status.success());
    assert!(!check(root.path(), Some("other")).status.success());
    let head = repo.head().unwrap().target().unwrap();
    repo.set_head_detached(head).unwrap();
    assert!(!check(root.path(), Some("manuals")).status.success());
    assert!(check(root.path(), None).status.success());
    repo.set_head("refs/heads/unborn").unwrap();
    assert!(!check(root.path(), Some("unborn")).status.success());
    repo.set_head("refs/heads/manuals").unwrap();
    symlink("image.png", root.path().join("docs/link.png")).unwrap();
    assert!(!check(root.path(), None).status.success());
    let plain = tempfile::tempdir().unwrap();
    fs::create_dir(plain.path().join("docs")).unwrap();
    assert!(!check(plain.path(), None).status.success());
    fs::create_dir_all(root.path().join("nested/docs")).unwrap();
    assert!(!check(&root.path().join("nested"), None).status.success());
}

#[test]
fn omitted_folder_selects_exact_repository_root_in_capture_manifest_and_discovery() {
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("project");
    fs::create_dir(&root).unwrap();
    let repo = fixture(&root);
    // Put the fixture's unsafe sibling outside this repository.
    fs::rename(
        root.join("sibling-repository"),
        parent.path().join("sibling-repository"),
    )
    .unwrap();
    fs::write(root.join("README.md"), "# Repository\n").unwrap();
    let mut selection = placement(&root, Some("manuals"));
    selection["source"]
        .as_object_mut()
        .unwrap()
        .remove("subdirectory");
    let authored = tempfile::tempdir().unwrap();
    let destination = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("stage")
        .arg(authored.path())
        .arg(destination.path())
        .args(["--workspace", "fixture", "--site", "site", "--placement"])
        .arg(selection.to_string())
        .arg("--create-stage")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let staged = Path::new(report["directory"].as_str().unwrap()).join("manual");
    assert_eq!(
        fs::read_to_string(staged.join("README.md")).unwrap(),
        "# Repository\n"
    );
    assert_eq!(
        fs::read_to_string(staged.join("docs/guide.md")).unwrap(),
        "# Working edit\n"
    );
    assert!(!staged.join(".git").exists());
    assert!(!staged.join("sibling-repository").exists());
    assert!(!staged.join("docs/drop.tmp").exists());
    let manifest: Value =
        serde_json::from_slice(&fs::read(staged.join(source::GENERATED_TREE_MANIFEST)).unwrap())
            .unwrap();
    assert_eq!(
        manifest["source"],
        json!({"kind":"working_repository", "expected_branch":"manuals"})
    );
    selection["source"]["subdirectory"] = json!(".");
    assert!(source::input::parse(&selection.to_string()).is_err());
    fs::write(root.join("gazette.toml"), "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:manual'\nlocation={kind='working-repository',root='.'}\n").unwrap();
    let loaded =
        configuration::load_workspace(&root, &"gazette.toml".parse().unwrap(), Default::default())
            .unwrap();
    let discovered = configuration::discover_sources(
        &loaded,
        [&"source:manual".parse().unwrap()],
        Default::default(),
    )
    .unwrap();
    assert!(discovered[0].subdirectory.is_none());
    assert!(discovered[0].listing.entries().contains_key("README.md"));
    assert!(
        discovered[0]
            .listing
            .entries()
            .contains_key("docs/image.png")
    );
    assert!(!discovered[0].listing.entries().contains_key(".git"));
    assert!(repo.head().unwrap().is_branch());
}
