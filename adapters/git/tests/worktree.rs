//! Native independent metadata preparation, without checkout or source writes.

use gazette_adapter_git::Git;
use gazette_core::{
    release::{
        CommitMetadata, CommitOptions, CommitSignature, WorktreeEffect, WorktreeLimits,
        WorktreeOptions,
    },
    source::{
        CapturedSource, Content, EntryPath, InventoryBuilder, Limits, RevisionLimits, Sha256Digest,
    },
};
use gazette_ports::release::{ReleaseCommits, ReleaseWorktrees};
use git2::{Oid, Repository};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

#[path = "worktree/history.rs"]
mod history;

#[path = "worktree/synchronization.rs"]
mod synchronization;

#[path = "worktree/setup.rs"]
mod setup;

fn source() -> CapturedSource {
    let entries: &[(&str, &[u8])] = &[
        (".gitattributes", b"*.txt text eol=lf filter=must-not-run\n"),
        (".gitignore", b"payload.txt\n"),
        ("payload.txt", b"exact\r\nretained\0binary\xff"),
    ];
    let mut inventory = InventoryBuilder::new(Limits::default());
    let mut files = BTreeMap::new();
    for (name, bytes) in entries {
        let path: EntryPath = name.parse().expect("portable fixture name");
        inventory
            .insert(
                path.clone(),
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                },
            )
            .expect("bounded fixture inventory");
        files.insert(path, bytes.to_vec());
    }
    CapturedSource::from_parts(inventory.finish(), files).expect("exact retained fixture")
}

fn recipe() -> CommitMetadata {
    let signature = CommitSignature {
        name: "Explicit fixture".into(),
        email: "test@example.invalid".into(),
        seconds: 100,
        offset_minutes: 0,
    };
    CommitMetadata {
        parent: None,
        author: signature.clone(),
        committer: signature,
        message: "Exact deployment\n".into(),
    }
}

fn options() -> WorktreeOptions {
    WorktreeOptions {
        allow_initialize: true,
        ..Default::default()
    }
}

fn stage(root: &Path, source: &CapturedSource) {
    fs::create_dir(root).expect("fresh exact deployment directory");
    for entry in source.inventory().entries() {
        fs::write(
            root.join(entry.path.as_str()),
            source.file(&entry.path).expect("retained fixture bytes"),
        )
        .expect("preexisting staged output");
    }
}

fn check_output(root: &Path, source: &CapturedSource) {
    for entry in source.inventory().entries() {
        assert_eq!(
            fs::read(root.join(entry.path.as_str())).expect("unchanged staged file"),
            source.file(&entry.path).expect("retained comparison")
        );
    }
}

#[test]
fn independent_metadata_preserves_binary_crlf_ignored_files_and_shared_commit_identity() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("native independent preparation");
    check_output(&root, &input);
    let repository = Repository::open(&root).expect("normal native repository");
    assert!(!repository.is_bare());
    assert!(!repository.is_worktree());
    assert!(repository.head_detached().expect("native detached HEAD"));
    assert_eq!(
        repository
            .head()
            .expect("HEAD")
            .target()
            .expect("direct HEAD")
            .to_string(),
        report.commit.commit.as_str()
    );
    assert_eq!(repository.references().expect("native refs").count(), 0);
    assert!(repository.remotes().expect("native remotes").is_empty());
    assert!(!root.join(".git/logs").exists());
    for directory in ["hooks", "info"] {
        let path = root.join(".git").join(directory);
        if path.exists() {
            assert_eq!(
                fs::read_dir(path)
                    .expect("empty template directory")
                    .count(),
                0
            );
        }
    }
    let index = repository.index().expect("actual native index");
    assert_eq!(index.len(), input.inventory().entries().len());
    for entry in index.iter() {
        let path: EntryPath = std::str::from_utf8(&entry.path)
            .expect("literal path")
            .parse()
            .expect("portable path");
        assert_eq!(
            repository
                .find_blob(entry.id)
                .expect("indexed native blob")
                .content(),
            input.file(&path).expect("retained bytes")
        );
        assert_eq!(entry.mode, 0o100644);
    }
    let before_head = fs::read(root.join(".git/HEAD")).expect("HEAD bytes");
    let before_index = fs::read(root.join(".git/index")).expect("index bytes");
    assert_eq!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .expect("independent observation"),
        report
    );
    assert_eq!(
        fs::read(root.join(".git/HEAD")).expect("HEAD unchanged"),
        before_head
    );
    assert_eq!(
        fs::read(root.join(".git/index")).expect("index unchanged"),
        before_index
    );
    let bare = temporary.path().join("release.git");
    Repository::init_bare(&bare).expect("separate test-owned bare storage");
    let detached = Git
        .prepare_commit(
            &bare,
            input.as_input(),
            &recipe(),
            CommitOptions {
                allow_write_objects: true,
                ..Default::default()
            },
        )
        .expect("existing bare path");
    assert_eq!(
        report.commit, detached,
        "both entry routes share exact commit encoding"
    );
    assert!(
        Git.verify_commit(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            RevisionLimits::default()
        )
        .is_err(),
        "bare API must not adopt a normal repository"
    );
}

#[test]
fn permission_recipe_parent_and_input_limits_refuse_before_metadata_creation() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let mut parent = recipe();
    parent.parent = Some(
        "1111111111111111111111111111111111111111"
            .parse()
            .expect("full fixture parent"),
    );
    let mut invalid = recipe();
    invalid.author.name.clear();
    let mut limited = options();
    limited.limits.output.content.total_bytes = 0;
    let mut metadata_limited = options();
    metadata_limited.limits.output.metadata_bytes = 0;
    for (recipe, options) in [
        (recipe(), WorktreeOptions::default()),
        (parent, options()),
        (invalid, options()),
        (recipe(), limited),
        (recipe(), metadata_limited),
    ] {
        let error = Git
            .prepare_worktree(&root, input.as_input(), &recipe, None, options)
            .expect_err("invalid preconditions cannot initialize metadata");
        assert_eq!(error.effect, WorktreeEffect::NotAttempted);
        assert!(!root.join(".git").exists());
        check_output(&root, &input);
    }
}

#[test]
fn existing_metadata_including_invalid_files_directories_and_links_is_never_adopted() {
    for kind in 0..3 {
        let temporary = tempfile::tempdir().expect("disposable fixture parent");
        let root = temporary.path().join("deployment");
        let input = source();
        stage(&root, &input);
        let target = root.join(".git");
        match kind {
            0 => fs::write(&target, b"invalid metadata").expect("unowned file collision"),
            1 => fs::create_dir(&target).expect("unowned directory collision"),
            _ => {
                symlink(temporary.path().join("missing"), &target).expect("dangling metadata link")
            }
        }
        let before = fs::symlink_metadata(&target).expect("literal collision metadata");
        let error = Git
            .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
            .expect_err("every existing administrative object refuses");
        assert_eq!(error.effect, WorktreeEffect::NotAttempted);
        assert_eq!(
            fs::symlink_metadata(&target)
                .expect("collision preserved")
                .file_type(),
            before.file_type()
        );
        check_output(&root, &input);
    }
}

#[test]
fn replay_is_observation_not_reinitialization_and_index_or_head_changes_fail() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("prepared native repository");
    let error = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect_err("no reinitialization replay");
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    let repository = Repository::open(&root).expect("fixture repository");
    let index_bytes = fs::read(root.join(".git/index")).expect("exact initial index");
    let mut index = repository.index().expect("fixture native index");
    index
        .remove_path(Path::new("payload.txt"))
        .expect("remove one semantic index member");
    index.write().expect("persist index corruption");
    assert!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .is_err()
    );
    fs::write(root.join(".git/index"), index_bytes).expect("restore disposable index fixture");
    fs::write(root.join(".git/HEAD"), b"ref: refs/heads/unborn\n")
        .expect("change HEAD to symbolic");
    assert!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .is_err()
    );
    check_output(&root, &input);
}

#[test]
fn object_corruption_and_linked_administrative_storage_refuse_observation() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("prepared native repository");
    let external = temporary.path().join("external");
    fs::write(&external, b"unowned").expect("outside fixture file");
    symlink(&external, root.join(".git/linked")).expect("hostile administrative link");
    assert!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .is_err()
    );
    fs::remove_file(root.join(".git/linked")).expect("remove only disposable test link");
    fs::hard_link(&external, root.join(".git/shared")).expect("shared administrative inode");
    assert!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .is_err()
    );
    fs::remove_file(root.join(".git/shared")).expect("remove disposable hardlink");
    let id = report.commit.commit.as_str();
    let object = root.join(".git/objects").join(&id[..2]).join(&id[2..]);
    fs::set_permissions(&object, fs::Permissions::from_mode(0o600))
        .expect("make disposable object writable");
    fs::write(&object, b"corrupt").expect("corrupt exact fixture commit");
    assert!(
        Git.inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default()
        )
        .is_err()
    );
    check_output(&root, &input);
}

#[test]
fn post_creation_storage_failure_preserves_commit_and_metadata_for_reconciliation() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let error = Git
        .prepare_worktree(
            &root,
            input.as_input(),
            &recipe(),
            None,
            WorktreeOptions {
                storage_bytes: 1,
                ..options()
            },
        )
        .expect_err("complete native storage exceeds the permitted synchronization budget");
    let WorktreeEffect::CommitCreated(commit) = error.effect else {
        panic!("known created commit coordinate must survive synchronization failure");
    };
    assert!(root.join(".git/index").is_file());
    let repository = Repository::open(&root).expect("native residue preserved");
    assert!(
        repository
            .find_commit(Oid::from_str(commit.as_str()).expect("known full commit"))
            .is_ok()
    );
    Git.inspect_worktree(
        &root,
        &commit,
        input.inventory().identity(),
        &recipe(),
        WorktreeLimits::default(),
    )
    .expect("read-only observation is not prior durability proof");
    check_output(&root, &input);
}

#[test]
fn altered_configuration_is_refused_before_native_include_loading() {
    let temporary = tempfile::tempdir().expect("disposable fixture parent");
    let root = temporary.path().join("deployment");
    let input = source();
    stage(&root, &input);
    let report = Git
        .prepare_worktree(&root, input.as_input(), &recipe(), None, options())
        .expect("prepared native repository");
    // The referenced path must not be consulted by a Git parser: rejection belongs
    // to the exact owned-byte profile, before opening the native repository.
    let configuration = b"[include]\n\tpath = /gazette-test-missing-external-config\n";
    fs::write(root.join(".git/config"), configuration).expect("alter disposable local config");
    let error = Git
        .inspect_worktree(
            &root,
            &report.commit.commit,
            report.commit.output,
            &recipe(),
            WorktreeLimits::default(),
        )
        .expect_err("no external configuration interpretation");
    assert_eq!(
        error.reason,
        "deployment configuration differs from its exact owned profile"
    );
    assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    assert_eq!(
        fs::read(root.join(".git/config")).expect("configuration preserved"),
        configuration
    );
}
