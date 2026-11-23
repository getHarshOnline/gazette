//! Exact-pin verification through the real executable and disposable Git repos.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use git2::{Oid, Repository, Signature};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

#[path = "support/mcp.rs"]
mod mcp_carrier;

#[path = "revision/preparation.rs"]
mod preparation;

#[path = "revision/markdown.rs"]
mod markdown;

#[path = "revision/workspace.rs"]
mod workspace;

struct Fixture {
    root: TempDir,
    commit: Oid,
}

impl Fixture {
    fn new() -> Self {
        let root = tempdir().expect("fixture");
        let commit = seed_repo(root.path());
        Self { root, commit }
    }

    fn inspect(&self, extra: &[&str]) -> Output {
        command(self.root.path(), &self.commit.to_string())
            .args(extra)
            .output()
            .expect("Gazette")
    }
}

fn seed_repo(root: &Path) -> Oid {
    let repo = Repository::init(root).expect("temporary repository");
    fs::create_dir(root.join("docs")).expect("docs");
    fs::write(root.join("docs/readme.md"), b"abc").expect("source");
    let blob = repo.blob(b"abc").expect("blob");
    commit_docs(&repo, &[("readme.md", blob, 0o100644)], "fixture")
}

fn commit_docs(repo: &Repository, entries: &[(&str, Oid, i32)], message: &str) -> Oid {
    let mut docs = repo.treebuilder(None).expect("docs tree");
    for &(name, id, mode) in entries {
        docs.insert(name, id, mode).expect("tree entry");
    }
    let docs = docs.write().expect("write docs tree");
    let mut root = repo.treebuilder(None).expect("root tree");
    root.insert("docs", docs, 0o040000).expect("docs entry");
    commit_tree(repo, root.write().expect("write root tree"), message)
}

fn commit_tree(repo: &Repository, tree: Oid, message: &str) -> Oid {
    let tree = repo.find_tree(tree).expect("tree");
    let signature = Signature::new(
        "Gazette fixture",
        "fixture@example.invalid",
        &git2::Time::new(0, 0),
    )
    .expect("signature");
    let parent = repo
        .head()
        .ok()
        .and_then(|r| r.target())
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
    .expect("temporary commit")
}

fn command(root: &Path, commit: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command
        .arg("inspect-source")
        .arg(root)
        .args(["--subdir", "docs", "--commit", commit]);
    command
}

fn failure(output: Output, expected: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "partial verified report: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(expected),
        "{output:?}"
    );
}

#[test]
fn exact_pin_binds_observed_bytes_and_sdk_cli_parity_without_writes() {
    use gazette_sdk::source::{self, Limits, RevisionLimits};

    let fixture = Fixture::new();
    let before = source::inspect(fixture.root.path(), None, Limits::default())
        .expect("full fixture inventory");
    let output = fixture.inspect(&[]);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["commit"], fixture.commit.to_string());
    assert_eq!(report["subdirectory"], "docs");
    assert_eq!(report["inventory"]["entries"][0]["path"], "readme.md");
    assert_eq!(
        report["inventory"]["identity"]["format"],
        "gazette.inventory.v1"
    );
    assert_eq!(
        report["inventory"]["identity"]["sha256"],
        "2c53e22d0ebcd3daee756bdb6f0c2227f0e8254d2f5e88a1e63e33c52a240156"
    );
    assert_eq!(
        report["inventory"]["entries"][0]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let sdk = source::verify(
        fixture.root.path(),
        &"docs".parse().expect("subdirectory"),
        &fixture.commit.to_string().parse().expect("commit"),
        RevisionLimits::default(),
    )
    .expect("SDK verification");
    assert_eq!(report, serde_json::to_value(sdk).expect("SDK JSON"));
    assert_eq!(
        before,
        source::inspect(fixture.root.path(), None, Limits::default()).expect("after")
    );
}

#[test]
fn changed_removed_extra_and_ignored_source_paths_fail_closed() {
    for mutation in ["changed", "removed", "extra", "ignored", "empty directory"] {
        let fixture = Fixture::new();
        match mutation {
            "changed" => fs::write(fixture.root.path().join("docs/readme.md"), b"xyz")
                .expect("same-size change"),
            "removed" => fs::remove_file(fixture.root.path().join("docs/readme.md"))
                .expect("remove fixture file"),
            "empty directory" => {
                fs::create_dir(fixture.root.path().join("docs/extra")).expect("directory")
            }
            _ => {
                fs::write(fixture.root.path().join("docs/extra"), b"private").expect("extra");
                if mutation == "ignored" {
                    fs::write(
                        fixture.root.path().join(".git/info/exclude"),
                        b"docs/extra\n",
                    )
                    .expect("ignore fixture entry");
                }
            }
        }
        failure(fixture.inspect(&[]), "source_content_mismatch");
    }
}

#[test]
fn unselected_changes_do_not_change_verified_output() {
    let fixture = Fixture::new();
    let before = fixture.inspect(&[]);
    assert!(before.status.success(), "{before:?}");
    fs::write(fixture.root.path().join("private.txt"), b"not selected").expect("outside docs");
    let after = fixture.inspect(&[]);
    assert!(after.status.success(), "{after:?}");
    assert_eq!(before.stdout, after.stdout);
}

#[test]
fn another_head_with_identical_source_bytes_does_not_satisfy_the_pin() {
    let fixture = Fixture::new();
    let repo = Repository::open(fixture.root.path()).expect("repo");
    let blob = repo.blob(b"abc").expect("blob");
    let newer = commit_docs(&repo, &[("readme.md", blob, 0o100644)], "different commit");
    assert_ne!(newer, fixture.commit);
    failure(fixture.inspect(&[]), "source_revision_mismatch");
}

#[test]
fn commit_spelling_and_required_selection_are_validated_before_reads() {
    let missing = Path::new("/gazette-test-intentionally-absent");
    for pin in [
        "HEAD",
        "main",
        "abcd123",
        "fffffffffffffffffffffffffffffffffffffffff",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggggggggggg",
    ] {
        let output = command(missing, pin).output().expect("invalid pin");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "inspect-source",
            "/gazette-test-intentionally-absent",
            "--commit",
            "1111111111111111111111111111111111111111",
        ])
        .output()
        .expect("missing subdirectory");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn git_metadata_and_source_byte_limits_are_independent() {
    let fixture = Fixture::new();
    failure(
        fixture.inspect(&["--max-git-metadata-bytes", "0"]),
        "source_limit",
    );
    failure(fixture.inspect(&["--max-file-bytes", "2"]), "source_limit");
    let repo = Repository::open(fixture.root.path()).expect("repo");
    let commit = repo.find_commit(fixture.commit).expect("commit");
    let root = commit.tree().expect("root");
    let docs = root.get_name("docs").expect("docs").id();
    let odb = repo.odb().expect("objects");
    let total: usize = [fixture.commit, root.id(), docs]
        .into_iter()
        .map(|id| odb.read_header(id).expect("metadata").0)
        .sum();
    let exact = fixture.inspect(&["--max-git-metadata-bytes", &total.to_string()]);
    assert!(exact.status.success(), "{exact:?}");
    failure(
        fixture.inspect(&["--max-git-metadata-bytes", &(total - 1).to_string()]),
        "source_limit",
    );
}

#[test]
fn exact_root_is_required_and_environment_cannot_select_another_repository() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.root.path().join("nested/docs")).expect("nested");
    fs::write(fixture.root.path().join("nested/docs/readme.md"), b"abc").expect("nested source");
    failure(
        command(
            &fixture.root.path().join("nested"),
            &fixture.commit.to_string(),
        )
        .output()
        .expect("nested selection"),
        "source_io",
    );
    let other = Fixture::new();
    fs::write(other.root.path().join("docs/readme.md"), b"wrong").expect("other source");
    let output = command(fixture.root.path(), &fixture.commit.to_string())
        .env("GIT_DIR", other.root.path().join(".git"))
        .env("GIT_WORK_TREE", other.root.path())
        .output()
        .expect("explicit root");
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn committed_links_and_gitlinks_are_not_treated_as_document_files() {
    for mode in [0o120000, 0o160000] {
        let mut fixture = Fixture::new();
        let repo = Repository::open(fixture.root.path()).expect("repo");
        let id = if mode == 0o160000 {
            fixture.commit
        } else {
            repo.blob(b"abc").expect("link bytes")
        };
        fixture.commit = commit_docs(&repo, &[("readme.md", id, mode)], "unsupported entry");
        // The local entry stays a regular file: this exercises Git-side policy,
        // not the filesystem reader's earlier symlink rejection.
        failure(fixture.inspect(&[]), "source_unsupported");
    }
}

#[test]
fn missing_or_misidentified_git_objects_fail_before_a_verified_report() {
    for corrupt in [false, true] {
        let fixture = Fixture::new();
        let repo = Repository::open(fixture.root.path()).expect("repo");
        let original = repo.blob(b"abc").expect("source blob").to_string();
        let replacement = repo.blob(b"xyz").expect("other blob").to_string();
        let object_path = |id: &str| {
            fixture
                .root
                .path()
                .join(".git/objects")
                .join(&id[..2])
                .join(&id[2..])
        };
        // Only disposable fixture object files are removed/replaced. The real
        // executable must detect missing bytes or a valid object under a false ID.
        fs::remove_file(object_path(&original)).expect("remove fixture object");
        if corrupt {
            fs::copy(object_path(&replacement), object_path(&original))
                .expect("misidentified fixture object");
        }
        failure(fixture.inspect(&[]), "source_io");
    }
}

#[test]
fn committed_object_limit_is_checked_even_when_local_file_is_smaller() {
    let mut fixture = Fixture::new();
    let repo = Repository::open(fixture.root.path()).expect("repo");
    let large = repo.blob(&[b'a'; 1024]).expect("larger committed blob");
    fixture.commit = commit_docs(&repo, &[("readme.md", large, 0o100644)], "larger source");
    failure(fixture.inspect(&["--max-file-bytes", "3"]), "source_limit");
}

#[test]
fn detached_head_at_the_selected_pin_is_accepted() {
    let fixture = Fixture::new();
    let repo = Repository::open(fixture.root.path()).expect("repo");
    repo.set_head_detached(fixture.commit)
        .expect("fixture detached HEAD");
    let output = fixture.inspect(&[]);
    assert!(output.status.success(), "{output:?}");
}

struct ParentFixture {
    root: TempDir,
    parent: Oid,
    child: Oid,
}

impl ParentFixture {
    fn new() -> Self {
        let root = tempdir().expect("parent fixture");
        let repo = Repository::init(root.path()).expect("parent repository");
        let child = seed_repo(&root.path().join("packages/tool"));
        let parent = commit_child(&repo, child, 0o160000);
        Self {
            root,
            parent,
            child,
        }
    }

    fn inspect(&self, extra: &[&str]) -> Output {
        command(self.root.path(), &self.parent.to_string())
            .args(["--submodule", "packages/tool"])
            .args(extra)
            .output()
            .expect("Gazette parent source")
    }
}

fn commit_child(repo: &Repository, child: Oid, mode: i32) -> Oid {
    let mut packages = repo.treebuilder(None).expect("packages");
    packages.insert("tool", child, mode).expect("child entry");
    let mut root = repo.treebuilder(None).expect("parent tree");
    root.insert(
        "packages",
        packages.write().expect("packages tree"),
        0o040000,
    )
    .expect("parent entry");
    commit_tree(repo, root.write().expect("parent root"), "parent fixture")
}

#[test]
fn parent_gitlink_derives_child_pin_with_sdk_cli_parity_and_no_source_writes() {
    use gazette_sdk::source::{self, Limits, RevisionLimits};

    let fixture = ParentFixture::new();
    let before = source::inspect(fixture.root.path(), None, Limits::default()).expect("before");
    let output = fixture.inspect(&[]);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(report["parent_commit"], fixture.parent.to_string());
    assert_eq!(report["submodule_path"], "packages/tool");
    assert_eq!(report["source"]["commit"], fixture.child.to_string());
    assert_eq!(report["source"]["subdirectory"], "docs");
    assert_eq!(
        report["source"]["inventory"]["entries"][0]["path"],
        "readme.md"
    );
    let sdk = source::verify_submodule(
        fixture.root.path(),
        &fixture.parent.to_string().parse().expect("parent"),
        &"packages/tool".parse().expect("submodule"),
        &"docs".parse().expect("docs"),
        RevisionLimits::default(),
    )
    .expect("SDK parent verification");
    assert_eq!(report, serde_json::to_value(sdk).expect("SDK JSON"));
    assert_eq!(
        before,
        source::inspect(fixture.root.path(), None, Limits::default()).expect("after")
    );
}

#[test]
fn parent_and_child_identity_and_child_bytes_are_independently_required() {
    for mutation in ["parent HEAD", "child HEAD", "child bytes"] {
        let fixture = ParentFixture::new();
        match mutation {
            "parent HEAD" => {
                let repo = Repository::open(fixture.root.path()).expect("parent");
                let tree = repo.find_commit(fixture.parent).expect("commit").tree_id();
                let changed = commit_tree(&repo, tree, "another parent revision");
                assert_ne!(changed, fixture.parent);
            }
            "child HEAD" => {
                let repo =
                    Repository::open(fixture.root.path().join("packages/tool")).expect("child");
                let tree = repo.find_commit(fixture.child).expect("commit").tree_id();
                let changed = commit_tree(&repo, tree, "another child revision");
                assert_ne!(changed, fixture.child);
            }
            _ => fs::write(
                fixture.root.path().join("packages/tool/docs/readme.md"),
                b"xyz",
            )
            .expect("changed child bytes"),
        }
        failure(
            fixture.inspect(&[]),
            if mutation == "child bytes" {
                "source_content_mismatch"
            } else {
                "source_revision_mismatch"
            },
        );
    }
}

#[test]
fn parent_selection_never_substitutes_an_unselected_sibling_checkout() {
    let fixture = ParentFixture::new();
    let baseline = fixture.inspect(&[]);
    assert!(baseline.status.success(), "{baseline:?}");
    seed_repo(&fixture.root.path().join("sibling"));
    fs::write(
        fixture.root.path().join("sibling/docs/readme.md"),
        b"unselected",
    )
    .expect("unselected changes");
    let again = fixture.inspect(&[]);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(baseline.stdout, again.stdout);
    fs::rename(
        fixture.root.path().join("packages/tool"),
        fixture.root.path().join("moved-child"),
    )
    .expect("move fixture child");
    failure(fixture.inspect(&[]), "source_io");
    assert!(
        !fixture.root.path().join("packages/tool").exists(),
        "verification must not initialize or fetch the missing child"
    );
}

#[test]
fn parent_selection_rejects_intermediate_symlinks_and_non_gitlink_entries() {
    let fixture = ParentFixture::new();
    fs::rename(
        fixture.root.path().join("packages"),
        fixture.root.path().join("moved-packages"),
    )
    .expect("move fixture directory");
    std::os::unix::fs::symlink("moved-packages", fixture.root.path().join("packages"))
        .expect("fixture intermediate link");
    failure(fixture.inspect(&[]), "source_io");

    let mut fixture = ParentFixture::new();
    let repo = Repository::open(fixture.root.path()).expect("parent");
    let blob = repo.blob(b"not a gitlink").expect("blob");
    fixture.parent = commit_child(&repo, blob, 0o100644);
    failure(fixture.inspect(&[]), "source_unsupported");
}

#[test]
fn parent_and_child_reads_share_one_exact_metadata_budget() {
    let fixture = ParentFixture::new();
    let parent = Repository::open(fixture.root.path()).expect("parent");
    let child = Repository::open(fixture.root.path().join("packages/tool")).expect("child");
    let metadata = |repo: &Repository, commit: Oid, subtree: &str| -> usize {
        let root = repo
            .find_commit(commit)
            .expect("commit")
            .tree()
            .expect("root");
        let selected = root.get_name(subtree).expect("subtree").id();
        let odb = repo.odb().expect("objects");
        [commit, root.id(), selected]
            .into_iter()
            .map(|id| odb.read_header(id).expect("metadata size").0)
            .sum()
    };
    let parent_bytes = metadata(&parent, fixture.parent, "packages");
    let child_bytes = metadata(&child, fixture.child, "docs");
    // Parent intent is read before and after the child verification.
    let total = parent_bytes * 2 + child_bytes;
    let exact = fixture.inspect(&["--max-git-metadata-bytes", &total.to_string()]);
    assert!(exact.status.success(), "{exact:?}");
    failure(
        fixture.inspect(&["--max-git-metadata-bytes", &(total - 1).to_string()]),
        "source_limit",
    );
}

#[test]
fn submodule_arguments_require_exact_parent_pin_and_safe_selection() {
    for selection in [
        "../sibling",
        "/absolute",
        "packages//tool",
        "packages/%2e%2e",
    ] {
        let output = command(
            Path::new("/gazette-test-intentionally-absent"),
            "1111111111111111111111111111111111111111",
        )
        .args(["--submodule", selection])
        .output()
        .expect("invalid selection");
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(output.stdout.is_empty());
    }
    let output = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .args([
            "inspect-source",
            "/gazette-test-intentionally-absent",
            "--submodule",
            "packages/tool",
            "--subdir",
            "docs",
        ])
        .output()
        .expect("missing parent pin");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
}

#[test]
fn acquired_repository_bytes_survive_source_changes_without_reopening() {
    use gazette_sdk::source::{self, Limits, RevisionLimits};
    let fixture = Fixture::new();
    let before = source::inspect(fixture.root.path(), None, Limits::default()).expect("before");
    let pin = fixture.commit.to_string().parse().expect("pin");
    let docs = "docs".parse().expect("docs");
    let acquired = source::acquire(fixture.root.path(), &docs, &pin, RevisionLimits::default())
        .expect("acquisition");
    assert_eq!(
        before,
        source::inspect(fixture.root.path(), None, Limits::default()).expect("after")
    );
    let output = fixture.inspect(&[]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        serde_json::to_value(acquired.verification()).expect("proof"),
        serde_json::from_slice::<Value>(&output.stdout).expect("CLI proof")
    );
    fs::write(fixture.root.path().join("docs/readme.md"), b"xyz").expect("later change");
    assert_eq!(
        acquired.file(&"readme.md".parse().expect("path")),
        Some(b"abc".as_slice())
    );
    assert!(matches!(
        source::acquire(fixture.root.path(), &docs, &pin, RevisionLimits::default()),
        Err(source::Error::ContentMismatch(_))
    ));
    fs::remove_file(fixture.root.path().join("docs/readme.md")).expect("remove fixture source");
    assert_eq!(
        acquired.file(&"readme.md".parse().expect("path")),
        Some(b"abc".as_slice())
    );
}

#[test]
fn acquired_child_keeps_the_parent_proof_and_payload_when_child_moves() {
    use gazette_sdk::source::{self, RevisionLimits};
    let fixture = ParentFixture::new();
    let acquired = source::acquire_submodule(
        fixture.root.path(),
        &fixture.parent.to_string().parse().expect("parent"),
        &"packages/tool".parse().expect("child"),
        &"docs".parse().expect("docs"),
        RevisionLimits::default(),
    )
    .expect("child acquisition");
    assert_eq!(
        acquired.verification().parent_commit().as_str(),
        fixture.parent.to_string()
    );
    assert_eq!(
        acquired.verification().source().commit().as_str(),
        fixture.child.to_string()
    );
    fs::rename(
        fixture.root.path().join("packages/tool"),
        fixture.root.path().join("moved"),
    )
    .expect("move fixture child");
    assert_eq!(
        acquired.file(&"readme.md".parse().expect("path")),
        Some(b"abc".as_slice())
    );
}

#[test]
fn acquisition_obeys_content_metadata_and_confinement_limits() {
    use gazette_sdk::source::{self, Limits, RevisionLimits};
    let fixture = Fixture::new();
    let pin = fixture.commit.to_string().parse().expect("pin");
    let docs = "docs".parse().expect("docs");
    for limits in [
        RevisionLimits {
            metadata_bytes: 0,
            ..RevisionLimits::default()
        },
        RevisionLimits {
            content: Limits {
                file_bytes: 2,
                ..Limits::default()
            },
            ..RevisionLimits::default()
        },
        RevisionLimits {
            content: Limits {
                total_bytes: 2,
                ..Limits::default()
            },
            ..RevisionLimits::default()
        },
    ] {
        assert!(matches!(
            source::acquire(fixture.root.path(), &docs, &pin, limits),
            Err(source::Error::Limit { .. })
        ));
    }
    fs::rename(
        fixture.root.path().join("docs"),
        fixture.root.path().join("real-docs"),
    )
    .expect("move fixture docs");
    std::os::unix::fs::symlink("real-docs", fixture.root.path().join("docs"))
        .expect("fixture link");
    assert!(source::acquire(fixture.root.path(), &docs, &pin, RevisionLimits::default()).is_err());
}
