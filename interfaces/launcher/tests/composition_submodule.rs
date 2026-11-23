//! Composition capture follows actual parent gitlinks, never a convenient checkout.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use gazette_sdk::{build, source, toml};
use git2::{Oid, Repository, Signature};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

struct Fixture {
    root: TempDir,
    parent: Oid,
    child: Oid,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("disposable parent");
        let parent = Repository::init(root.path()).expect("parent repository");
        let child_path = root.path().join("theme");
        let child = Repository::init(&child_path).expect("child repository");
        fs::create_dir(child_path.join("templates")).expect("selected source tree");
        fs::write(
            child_path.join("templates/page.html"),
            b"<h1>{{ title }}</h1>\n",
        )
        .expect("child template");
        let blob = child
            .blob(b"<h1>{{ title }}</h1>\n")
            .expect("template blob");
        let mut templates = child.treebuilder(None).expect("template tree");
        templates
            .insert("page.html", blob, 0o100644)
            .expect("file entry");
        let mut tree = child.treebuilder(None).expect("child tree");
        tree.insert(
            "templates",
            templates.write().expect("template tree write"),
            0o040000,
        )
        .expect("source subtree");
        let child_commit = commit_tree(
            &child,
            tree.write().expect("child tree write"),
            "child fixture",
        );
        let mut tree = parent.treebuilder(None).expect("parent tree");
        tree.insert("theme", child_commit, 0o160000)
            .expect("exact child gitlink");
        let parent_commit = commit_tree(
            &parent,
            tree.write().expect("parent tree write"),
            "parent fixture",
        );
        Self {
            root,
            parent: parent_commit,
            child: child_commit,
        }
    }

    fn config(&self) -> String {
        format!(
            "[composition]\n\
             theme={{package='theme:selected'}}\n\
             [composition.plugins]\n\
             'fixture-capabilities'={{state='enabled'}}\n\
             [composition.capabilities]\n\
             'commerce.catalog/v1'={{state='enabled',provider={{kind='plugin',package='fixture-capabilities',provider='extra'}},requirement='optional'}}\n\
             [[principals]]\nid='owner:one'\nkind='person'\n\
             [[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n\
             [[sources]]\nid='source:child'\n\
             location={{kind='submodule',root={:?},parent_commit='{}',submodule_path='theme',subdirectory='templates'}}\n\
             [[theme_packages]]\npackage='theme:selected'\nsource='source:child'\nmanifest='theme.toml'\n\
             [[plugin_packages]]\npackage='fixture-capabilities'\nsource='source:child'\nmanifest='plugin.toml'\n",
            self.root.path().to_str().expect("fixture path"),
            self.parent,
        )
    }
}

fn commit_tree(repo: &Repository, tree: Oid, message: &str) -> Oid {
    let tree = repo.find_tree(tree).expect("native tree");
    let signature = Signature::new(
        "Gazette fixture",
        "fixture@example.invalid",
        &git2::Time::new(0, 0),
    )
    .expect("deterministic fixture signature");
    let parent = repo
        .head()
        .ok()
        .and_then(|head| head.target())
        .and_then(|id| repo.find_commit(id).ok());
    let parents: Vec<_> = parent.iter().collect();
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )
    .expect("disposable commit")
}

fn limits() -> source::PreparationLimits {
    source::PreparationLimits {
        sources: 1,
        entries: 1,
        bytes: 1024,
        per_source: source::RevisionLimits::default(),
    }
}

fn acquire(config: &str) -> Result<build::AcquiredInputs, build::CompositionError> {
    build::acquire_composition(
        config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        limits(),
    )
}

fn run(config: &str) -> Output {
    let cwd = tempdir().expect("unrelated empty cwd");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "check",
            "--stdin",
            "--acquire-composition",
            "site:one",
            "--max-placements",
            "1",
        ])
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("native Gazette command");
    child
        .stdin
        .take()
        .expect("configuration stdin")
        .write_all(config.as_bytes())
        .expect("configuration bytes");
    let output = child.wait_with_output().expect("command completion");
    assert!(
        fs::read_dir(cwd.path())
            .expect("cwd observation")
            .next()
            .is_none(),
        "capture creates no output/database/configuration"
    );
    output
}

// Include native Git metadata as well as selected and unselected source bytes.
// No inventory exclusion can hide an acquisition write in these disposable repos.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).expect("fixture directory") {
            let entry = entry.expect("fixture entry");
            let path = entry.path();
            let logical = path
                .strip_prefix(root)
                .expect("fixture-relative path")
                .to_path_buf();
            let kind = entry.file_type().expect("native type");
            if kind.is_dir() {
                files.insert(logical, None);
                pending.push(path);
            } else {
                assert!(
                    kind.is_file(),
                    "fixture must contain only regular files/directories"
                );
                files.insert(logical, Some(fs::read(path).expect("fixture bytes")));
            }
        }
    }
    files
}

#[test]
fn composition_gitlink_capture_retains_parent_child_proof_and_exact_sdk_cli_parity() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let before = snapshot(fixture.root.path());
    let acquired = acquire(&config).expect("whole composition capture");
    assert_eq!(acquired.site().as_str(), "site:one");
    assert_eq!(
        acquired.sources().len(),
        1,
        "Theme and workspace provider share one exact source-key capture"
    );
    let input = &acquired.sources()[&"source:child".parse().expect("source key")];
    let source::AcquiredInput::Submodule(child) = input else {
        panic!("parent-gitlink proof must not become direct repository evidence")
    };
    assert_eq!(
        child.verification().parent_commit().as_str(),
        fixture.parent.to_string()
    );
    assert_eq!(
        child.verification().submodule_path(),
        &"theme"
            .parse::<source::Subdirectory>()
            .expect("selected gitlink path")
    );
    assert_eq!(
        child.verification().source().commit().as_str(),
        fixture.child.to_string()
    );
    assert_eq!(
        child.verification().source().subdirectory(),
        &"templates"
            .parse::<source::Subdirectory>()
            .expect("selected source subtree")
    );
    assert_eq!(
        input.file(&"page.html".parse().expect("path")),
        Some(b"<h1>{{ title }}</h1>\n".as_slice())
    );
    let output = run(&config);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let mut expected =
        serde_json::to_vec_pretty(&acquired.inspect()).expect("shared evidence projection");
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
    let wire: Value = serde_json::from_slice(&output.stdout).expect("native report");
    let evidence = &wire["sources"]["source:child"];
    assert_eq!(evidence["kind"], "submodule");
    assert_eq!(
        evidence["verification"]["parent_commit"],
        fixture.parent.to_string()
    );
    assert_eq!(
        evidence["verification"]["source"]["commit"],
        fixture.child.to_string()
    );
    assert_eq!(
        snapshot(fixture.root.path()),
        before,
        "SDK and CLI preserve source and Git bytes"
    );
    fs::write(
        fixture.root.path().join("theme/templates/page.html"),
        b"later native edit",
    )
    .expect("explicit fixture mutation after capture");
    assert_eq!(
        input.file(&"page.html".parse().expect("path")),
        Some(b"<h1>{{ title }}</h1>\n".as_slice()),
        "retained capture does not reopen live source"
    );
    assert_eq!(
        serde_json::to_value(acquired.inspect()).expect("retained proof"),
        serde_json::from_slice::<Value>(&expected).expect("original report")
    );
    assert_eq!(
        acquire(&config)
            .expect_err("fresh changed-byte acquisition")
            .code(),
        "source_content_mismatch"
    );
}

#[test]
fn parent_and_gitlink_child_pins_remain_independently_required() {
    for mutation in ["parent", "child"] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let path = if mutation == "parent" {
            fixture.root.path().to_path_buf()
        } else {
            fixture.root.path().join("theme")
        };
        let repository = Repository::open(path).expect("selected fixture repository");
        let prior = if mutation == "parent" {
            fixture.parent
        } else {
            fixture.child
        };
        let tree = repository
            .find_commit(prior)
            .expect("prior commit")
            .tree_id();
        assert_ne!(
            commit_tree(&repository, tree, "same bytes, different exact revision"),
            prior
        );
        let before = snapshot(fixture.root.path());
        let error = acquire(&config).expect_err("exact pin required even for identical bytes");
        assert_eq!(error.code(), "source_revision_mismatch");
        assert!(
            matches!(error, build::CompositionError::Source { binding, .. } if binding.as_str() == "source:child")
        );
        let output = run(&config);
        assert_eq!(output.status.code(), Some(1), "{mutation}: {output:?}");
        assert!(output.stdout.is_empty(), "no partial composition evidence");
        assert!(String::from_utf8_lossy(&output.stderr).contains("source_revision_mismatch"));
        assert_eq!(
            snapshot(fixture.root.path()),
            before,
            "failed capture does not repair pins or sources"
        );
    }
}

#[test]
fn missing_selected_child_never_uses_or_initializes_an_identical_sibling() {
    let fixture = Fixture::new();
    let config = fixture.config();
    assert!(
        acquire(&config).is_ok(),
        "valid initial parent-pinned composition"
    );
    let selected = fixture.root.path().join("theme");
    let sibling = fixture.root.path().join("same-repository-elsewhere");
    fs::rename(&selected, &sibling).expect("move only disposable fixture child");
    let before = snapshot(fixture.root.path());
    let error = acquire(&config).expect_err("declared native child is absent");
    assert_eq!(error.code(), "source_io");
    assert!(
        matches!(error, build::CompositionError::Source { binding, .. } if binding.as_str() == "source:child")
    );
    let output = run(&config);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source_io"));
    assert!(
        !selected.exists(),
        "capture cannot initialize, fetch, or recreate the missing child"
    );
    assert_eq!(
        Repository::open(sibling)
            .expect("identical sibling remains")
            .head()
            .expect("sibling head")
            .target(),
        Some(fixture.child)
    );
    assert_eq!(
        snapshot(fixture.root.path()),
        before,
        "failure preserves the sibling and parent Git state"
    );
}
