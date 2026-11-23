//! Native offline ancestry transfer and destination-only verification.

use super::*;
use gazette_core::release::{HistoryLimits, ParentSource};
use git2::{ObjectType, Signature, Time};
use std::path::PathBuf;

type Objects = BTreeMap<Oid, (ObjectType, Vec<u8>)>;

fn objects(repository: &Repository) -> Objects {
    let odb = repository.odb().expect("native fixture ODB");
    let mut result = BTreeMap::new();
    odb.foreach(|id| {
        let object = odb.read(*id).expect("fixture object bytes");
        result.insert(*id, (object.kind(), object.data().to_vec()));
        true
    })
    .expect("complete fixture object list");
    result
}

fn history(root: &Path) -> (Repository, Oid, Objects) {
    let repository = Repository::init_bare(root).expect("test-owned parent storage");
    let historical = b"historical\r\nbinary\0\xff".repeat(128);
    let blob = repository.blob(&historical).expect("historical bytes");
    let link = repository
        .blob(b"../../not-followed")
        .expect("symlink content as blob only");
    let tree = {
        let mut builder = repository
            .treebuilder(None)
            .expect("native historical tree builder");
        builder
            .insert("binary", blob, 0o100644)
            .expect("regular history");
        builder
            .insert("executable", blob, 0o100755)
            .expect("shared executable blob");
        builder
            .insert("link", link, 0o120000)
            .expect("symlink blob, never checkout");
        builder.write().expect("historical tree")
    };
    let tip = {
        let signature = Signature::new("Original", "original@example.invalid", &Time::new(1, 0))
            .expect("explicit fixture signature");
        let tree = repository.find_tree(tree).expect("native tree");
        let first = repository
            .commit(None, &signature, &signature, "root\n", &tree, &[])
            .expect("history root");
        let first = repository.find_commit(first).expect("root object");
        let left = repository
            .commit(None, &signature, &signature, "left\n", &tree, &[&first])
            .expect("first branch");
        let right = repository
            .commit(None, &signature, &signature, "right\n", &tree, &[&first])
            .expect("second branch");
        let left = repository.find_commit(left).expect("left object");
        let right = repository.find_commit(right).expect("right object");
        repository
            .commit(
                None,
                &signature,
                &signature,
                "merge\n",
                &tree,
                &[&left, &right],
            )
            .expect("all merge parents matter")
    };
    let closure = objects(&repository);
    (repository, tip, closure)
}

pub(super) fn native_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut pending = vec![root.to_owned()];
    let mut files = BTreeMap::new();
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("native fixture directory") {
            let entry = entry.expect("native fixture entry");
            if entry.file_type().expect("fixture type").is_dir() {
                pending.push(entry.path());
            } else {
                files.insert(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("descendant")
                        .to_owned(),
                    fs::read(entry.path()).expect("fixture bytes"),
                );
            }
        }
    }
    files
}

#[test]
fn synchronized_imported_history_needs_no_acquisition_source() {
    use gazette_core::release::WorktreeSyncOptions;
    use gazette_ports::release::WorktreeSync;
    let temporary = tempfile::tempdir().expect("disposable parent history fixture");
    let parent = temporary.path().join("parent.git");
    let (repository, tip, _) = history(&parent);
    let mut metadata = recipe();
    metadata.parent = Some(tip.to_string().parse().expect("exact native parent"));
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(
            &root,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Bare {
                path: parent.clone(),
            }),
            options(),
        )
        .expect("import complete real parent history");
    drop(repository);
    fs::remove_dir_all(&parent).expect("remove only test-owned acquisition repository");
    let before = native_files(&root);
    let options = WorktreeSyncOptions {
        allow_synchronize: true,
        ..Default::default()
    };
    assert_eq!(
        Git.synchronize_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &metadata,
            options
        )
        .expect("flush destination-only imported history"),
        report
    );
    assert_eq!(native_files(&root), before);
    let mut limited = options;
    limited.limits.history.objects = 0;
    let error = Git
        .synchronize_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &metadata,
            limited,
        )
        .expect_err("complete history bounds apply before flush");
    assert!(!error.attempted);
    assert_eq!(native_files(&root), before);
}

#[test]
fn bare_loose_and_packed_merge_history_is_self_contained_after_source_removal() {
    for packed in [false, true] {
        let temporary = tempfile::tempdir().expect("disposable parent");
        let parent_path = temporary.path().join("parent.git");
        let (repository, tip, closure) = history(&parent_path);
        let unrelated = repository
            .blob(b"unreachable object must not be imported")
            .expect("unrelated fixture object");
        repository
            .config()
            .expect("source config")
            .set_str("remote.origin.url", "invalid://never-contact")
            .expect("native source config not copied");
        repository
            .reference("refs/heads/source", tip, false, "fixture")
            .expect("source ref not copied");
        if packed {
            let mut builder = repository.packbuilder().expect("native pack fixture");
            for id in closure.keys() {
                builder
                    .insert_object(*id, None)
                    .expect("exact fixture pack members");
            }
            builder.set_threads(1);
            builder
                .write(&parent_path.join("objects/pack"), 0o644)
                .expect("real pack and native index");
            drop(builder);
            for id in closure.keys() {
                let text = id.to_string();
                fs::remove_file(
                    parent_path
                        .join("objects")
                        .join(&text[..2])
                        .join(&text[2..]),
                )
                .expect("remove only disposable loose duplicate so reads require pack");
            }
        }
        drop(repository);
        let before = native_files(&parent_path);
        let target = temporary.path().join("deployment");
        let input = source();
        stage(&target, &input);
        let mut metadata = recipe();
        metadata.parent = Some(tip.to_string().parse().expect("exact parent pin"));
        let maximum = HistoryLimits {
            objects: closure.len(),
            bytes: closure.values().map(|(_, bytes)| bytes.len() as u64).sum(),
            object_bytes: closure
                .values()
                .map(|(_, bytes)| bytes.len())
                .max()
                .expect("nonempty closure"),
        };
        let report = Git
            .prepare_worktree(
                &target,
                input.as_input(),
                &metadata,
                Some(&ParentSource::Bare {
                    path: parent_path.clone(),
                }),
                WorktreeOptions {
                    limits: WorktreeLimits {
                        output: RevisionLimits {
                            content: Limits {
                                file_bytes: 128,
                                total_bytes: 512,
                                ..Default::default()
                            },
                            ..Default::default()
                        },
                        history: maximum,
                    },
                    ..options()
                },
            )
            .expect("exact bounded full ancestry transfer");
        assert_eq!(
            native_files(&parent_path),
            before,
            "source refs/config/objects are never written"
        );
        fs::remove_dir_all(&parent_path).expect("remove only disposable source repository");
        let repository = Repository::open(&target).expect("self-contained destination");
        for (id, (kind, bytes)) in closure {
            let odb = repository.odb().expect("destination native ODB");
            let actual = odb
                .read(id)
                .expect("every ancestor object retained locally");
            assert_eq!(actual.kind(), kind);
            assert_eq!(actual.data(), bytes);
        }
        assert!(!repository.odb().expect("destination ODB").exists(unrelated));
        assert_eq!(
            repository
                .references()
                .expect("no copied source refs")
                .count(),
            0
        );
        assert!(
            repository
                .remotes()
                .expect("no copied source remotes")
                .is_empty()
        );
        assert!(
            !target.join("link").exists(),
            "historical symlink is never materialized"
        );
        Git.inspect_worktree(
            &target,
            &report.commit.commit,
            report.commit.output,
            &metadata,
            WorktreeLimits {
                history: maximum,
                ..Default::default()
            },
        )
        .expect("full destination-only ancestry verification");
        check_output(&target, &input);
    }
}

#[test]
fn previously_generated_worktree_supplies_exact_history_without_its_current_output() {
    let temporary = tempfile::tempdir().expect("disposable parent");
    let parent_path = temporary.path().join("previous");
    let input = source();
    stage(&parent_path, &input);
    let first = Git
        .prepare_worktree(&parent_path, input.as_input(), &recipe(), None, options())
        .expect("previous standalone worktree");
    let destination = temporary.path().join("next");
    stage(&destination, &input);
    let mut metadata = recipe();
    metadata.parent = Some(first.commit.commit);
    let child = Git
        .prepare_worktree(
            &destination,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Worktree {
                path: parent_path.clone(),
            }),
            options(),
        )
        .expect("owned worktree object source");
    fs::remove_dir_all(parent_path).expect("remove disposable previous worktree");
    Git.inspect_worktree(
        &destination,
        &child.commit.commit,
        child.commit.output,
        &metadata,
        WorktreeLimits::default(),
    )
    .expect("source-free child observation");
}

#[test]
fn closure_limits_missing_ancestors_and_wrong_pins_refuse_before_initialization() {
    for failure in [
        "objects",
        "bytes",
        "object",
        "storage",
        "missing-ancestor",
        "corrupt-ancestor",
        "wrong-pin",
        "wrong-kind",
        "pair",
    ] {
        let temporary = tempfile::tempdir().expect("disposable parent");
        let parent_path = temporary.path().join("parent.git");
        let (repository, tip, closure) = history(&parent_path);
        let root = temporary.path().join("deployment");
        let input = source();
        stage(&root, &input);
        let mut metadata = recipe();
        metadata.parent = Some(tip.to_string().parse().expect("parent pin"));
        let mut options = options();
        match failure {
            "objects" => options.limits.history.objects = closure.len() - 1,
            "bytes" => options.limits.history.bytes = 1,
            "object" => options.limits.history.object_bytes = 1,
            "storage" => options.storage_bytes = 1,
            "wrong-pin" => {
                metadata.parent = Some(
                    "1111111111111111111111111111111111111111"
                        .parse()
                        .expect("missing pin"),
                )
            }
            "wrong-kind" => {
                metadata.parent = Some(
                    repository
                        .blob(b"not a commit")
                        .expect("wrong-kind fixture")
                        .to_string()
                        .parse()
                        .expect("typed SHA-1"),
                )
            }
            "pair" => metadata.parent = None,
            "missing-ancestor" | "corrupt-ancestor" => {
                let ancestor = repository
                    .find_commit(tip)
                    .expect("tip")
                    .parent_id(0)
                    .expect("ancestor")
                    .to_string();
                let path = parent_path
                    .join("objects")
                    .join(&ancestor[..2])
                    .join(&ancestor[2..]);
                if failure == "missing-ancestor" {
                    fs::remove_file(path).expect("delete only disposable ancestor object");
                } else {
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                        .expect("make disposable object writable");
                    fs::write(path, b"corrupt").expect("corrupt only disposable ancestor object");
                }
            }
            _ => (),
        }
        drop(repository);
        let before = native_files(&parent_path);
        let error = Git
            .prepare_worktree(
                &root,
                input.as_input(),
                &metadata,
                Some(&ParentSource::Bare {
                    path: parent_path.clone(),
                }),
                options,
            )
            .expect_err("complete bounded acquisition before effects");
        assert_eq!(error.effect, WorktreeEffect::NotAttempted, "{failure}");
        assert!(!root.join(".git").exists());
        assert_eq!(native_files(&parent_path), before);
        check_output(&root, &input);
    }
}

#[test]
fn parent_source_overlap_and_gitlinks_are_explicitly_refused() {
    let temporary = tempfile::tempdir().expect("disposable parent");
    let parent_path = temporary.path().join("parent.git");
    let (repository, tip, _) = history(&parent_path);
    let input = source();
    let nested = parent_path.join("nested-stage");
    stage(&nested, &input);
    let mut metadata = recipe();
    metadata.parent = Some(tip.to_string().parse().expect("parent"));
    let error = Git
        .prepare_worktree(
            &nested,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Bare {
                path: parent_path.clone(),
            }),
            options(),
        )
        .expect_err("source ancestor cannot contain target");
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    assert!(!nested.join(".git").exists());
    let outer = temporary.path();
    let error = Git
        .prepare_worktree(
            outer,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Bare {
                path: parent_path.clone(),
            }),
            options(),
        )
        .expect_err("target ancestor cannot contain source");
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    assert!(!outer.join(".git").exists());
    let tree = {
        let mut builder = repository.treebuilder(None).expect("gitlink fixture tree");
        builder
            .insert("external", tip, 0o160000)
            .expect("real native gitlink");
        builder.write().expect("gitlink tree")
    };
    let linked = {
        let signature = Signature::new("Fixture", "fixture@example.invalid", &Time::new(1, 0))
            .expect("fixture signature");
        let tree = repository.find_tree(tree).expect("gitlink tree");
        repository
            .commit(None, &signature, &signature, "gitlink\n", &tree, &[])
            .expect("gitlink-bearing commit")
    };
    metadata.parent = Some(linked.to_string().parse().expect("linked parent"));
    let root = temporary.path().join("destination");
    stage(&root, &input);
    let error = Git
        .prepare_worktree(
            &root,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Bare { path: parent_path }),
            options(),
        )
        .expect_err("external submodule closure cannot be silently omitted");
    assert_eq!(
        error.reason,
        "history gitlinks require external repositories and are unsupported"
    );
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    assert!(!root.join(".git").exists());
}

#[test]
fn destination_inspection_checks_deep_ancestry_not_only_immediate_parent() {
    let temporary = tempfile::tempdir().expect("disposable parent");
    let parent_path = temporary.path().join("parent.git");
    let (repository, tip, _) = history(&parent_path);
    let ancestor = repository
        .find_commit(tip)
        .expect("tip")
        .parent_id(0)
        .expect("ancestor")
        .to_string();
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let mut metadata = recipe();
    metadata.parent = Some(tip.to_string().parse().expect("parent pin"));
    let report = Git
        .prepare_worktree(
            &root,
            input.as_input(),
            &metadata,
            Some(&ParentSource::Bare { path: parent_path }),
            options(),
        )
        .expect("complete native preparation");
    fs::remove_file(
        root.join(".git/objects")
            .join(&ancestor[..2])
            .join(&ancestor[2..]),
    )
    .expect("remove only disposable copied ancestor");
    let error = Git
        .inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &metadata,
            WorktreeLimits::default(),
        )
        .expect_err("immediate parent and current output still exist but history is incomplete");
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
}
