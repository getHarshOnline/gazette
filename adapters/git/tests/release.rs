//! Real local Git-object preparation, without ref, index, worktree or remote effects.

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use gazette_adapter_git::Git;
use gazette_core::{
    release::{CommitEffect, CommitMetadata, CommitOptions, CommitSignature},
    source::{
        CapturedSource, Content, EntryPath, InventoryBuilder, Limits, RevisionLimits, Sha256Digest,
    },
};
use gazette_ports::release::ReleaseCommits;
use git2::{ObjectType, Oid, Repository, Signature, Time};
use tempfile::TempDir;

fn capture(entries: &[(&str, Option<&[u8]>)]) -> CapturedSource {
    let mut builder = InventoryBuilder::new(Limits::default());
    let mut files = BTreeMap::new();
    for (path, bytes) in entries {
        let path: EntryPath = path.parse().expect("literal fixture entry path");
        let content = match bytes {
            Some(bytes) => {
                files.insert(path.clone(), bytes.to_vec());
                Content::File {
                    bytes: bytes.len() as u64,
                    sha256: Sha256Digest::of(bytes),
                }
            }
            None => Content::Directory,
        };
        builder
            .insert(path, content)
            .expect("complete parent-first fixture inventory");
    }
    CapturedSource::from_parts(builder.finish(), files).expect("independently bound retained bytes")
}

fn publication() -> CapturedSource {
    capture(&[
        (
            ".gitattributes",
            Some(b"*.txt text eol=lf filter=must-not-run\n"),
        ),
        (".gitignore", Some(b"payload.txt\n")),
        ("assets", None),
        ("assets/main.css", Some(b"body { color: red; }\n")),
        ("payload.txt", Some(b"literal\r\nretained\r\n\0bytes\xff")),
    ])
}

fn metadata() -> CommitMetadata {
    CommitMetadata {
        parent: None,
        author: CommitSignature {
            name: "Explicit Author".into(),
            email: "author@example.invalid".into(),
            seconds: 100,
            offset_minutes: 330,
        },
        committer: CommitSignature {
            name: "Explicit Committer".into(),
            email: "committer@example.invalid".into(),
            seconds: 120,
            offset_minutes: -60,
        },
        message: "Exact publication\n\nTrailing space stays. \n".into(),
    }
}

fn write_options() -> CommitOptions {
    CommitOptions {
        allow_write_objects: true,
        ..CommitOptions::default()
    }
}

fn bare() -> (TempDir, PathBuf, Repository) {
    let directory = tempfile::tempdir().expect("disposable native repository parent");
    let root = directory.path().join("release.git");
    let repository = Repository::init_bare(&root).expect("test-owned explicit bare initialization");
    (directory, root, repository)
}

#[derive(Debug, PartialEq, Eq)]
enum NativeEntry {
    Directory,
    File(Vec<u8>),
    Link(PathBuf),
}

fn snapshot(root: &Path, include_objects: bool) -> BTreeMap<PathBuf, NativeEntry> {
    fn walk(
        root: &Path,
        path: &Path,
        include_objects: bool,
        entries: &mut BTreeMap<PathBuf, NativeEntry>,
    ) {
        for child in fs::read_dir(path).expect("complete fixture directory read") {
            let child = child.expect("native fixture entry");
            let path = child.path();
            let relative = path
                .strip_prefix(root)
                .expect("fixture descendant")
                .to_owned();
            if !include_objects && relative.starts_with("objects") {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).expect("non-following fixture observation");
            if metadata.file_type().is_symlink() {
                entries.insert(
                    relative,
                    NativeEntry::Link(fs::read_link(&path).expect("literal fixture link")),
                );
            } else if metadata.is_dir() {
                entries.insert(relative, NativeEntry::Directory);
                walk(root, &path, include_objects, entries);
            } else {
                assert!(metadata.is_file(), "no special fixture objects");
                entries.insert(
                    relative,
                    NativeEntry::File(fs::read(&path).expect("exact native fixture bytes")),
                );
            }
        }
    }
    let mut entries = BTreeMap::new();
    walk(root, root, include_objects, &mut entries);
    entries
}

fn native_signature(value: &CommitSignature) -> Signature<'static> {
    Signature::new(
        &value.name,
        &value.email,
        &Time::new(value.seconds, value.offset_minutes),
    )
    .expect("explicit test Git signature")
}

fn native_commit(repository: &Repository, tree: Oid, metadata: &CommitMetadata) -> Oid {
    let tree = repository.find_tree(tree).expect("fixture tree exists");
    let parents = metadata.parent.as_ref().map(|parent| {
        repository
            .find_commit(Oid::from_str(parent.as_str()).expect("full parent OID"))
            .expect("fixture parent exists")
    });
    let parents: Vec<_> = parents.iter().collect();
    repository
        .commit(
            None,
            &native_signature(&metadata.author),
            &native_signature(&metadata.committer),
            &metadata.message,
            &tree,
            &parents,
        )
        .expect("detached fixture commit, never a ref update")
}

#[test]
fn retained_bytes_prepare_deterministically_without_filters_refs_index_or_configuration_changes() {
    let (_directory, root, repository) = bare();
    repository
        .config()
        .expect("local test config")
        .set_bool("core.autocrlf", true)
        .expect("hostile normalization setting");
    repository
        .config()
        .expect("local test config")
        .set_str("filter.must-not-run.clean", "false")
        .expect("filter must remain unused");
    repository
        .config()
        .expect("local test config")
        .set_bool("filter.must-not-run.required", true)
        .expect("required filter cannot be silently skipped by an index path");
    let before = snapshot(&root, false);
    let source = publication();
    let metadata = metadata();
    let first = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect("direct retained-object preparation");
    let second = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect("same exact recipe");
    assert_eq!(first, second);
    assert_eq!(first.output, source.inventory().identity());
    assert_eq!(first.entries, source.inventory().entries().len());
    assert_eq!(first.bytes, source.inventory().total_bytes());
    assert!(first.parent.is_none());
    let native = repository
        .find_commit(Oid::from_str(first.commit.as_str()).expect("complete result OID"))
        .expect("actual commit object");
    assert_eq!(native.parent_count(), 0);
    assert_eq!(native.message_bytes(), metadata.message.as_bytes());
    assert_eq!(
        native.author().name().expect("exact UTF-8 author name"),
        metadata.author.name.as_str()
    );
    assert_eq!(
        native.author().email().expect("exact UTF-8 author email"),
        metadata.author.email.as_str()
    );
    assert_eq!(native.author().when().seconds(), metadata.author.seconds);
    assert_eq!(
        native.author().when().offset_minutes(),
        metadata.author.offset_minutes
    );
    assert_eq!(
        native.committer().when().offset_minutes(),
        metadata.committer.offset_minutes
    );
    let tree = native.tree().expect("complete committed tree");
    for entry in source.inventory().entries() {
        let actual = tree
            .get_path(Path::new(entry.path.as_str()))
            .expect("every retained entry committed");
        match &entry.content {
            Content::Directory => assert_eq!(actual.filemode(), 0o040000),
            Content::File { .. } => {
                assert_eq!(actual.filemode(), 0o100644);
                let blob = repository.find_blob(actual.id()).expect("real file blob");
                assert_eq!(Some(blob.content()), source.file(&entry.path));
            }
        }
    }
    assert_eq!(
        snapshot(&root, false),
        before,
        "only detached object files may be added"
    );
    assert_eq!(repository.references().expect("native refs").count(), 0);
    assert!(repository.workdir().is_none());
    assert!(!root.join("index").exists());
    assert_eq!(
        Git.verify_commit(
            &root,
            &first.commit,
            first.output,
            &metadata,
            RevisionLimits::default()
        )
        .expect("independent complete read-back without HEAD"),
        first
    );
}

#[test]
fn a_lexically_interleaved_sibling_does_not_make_a_nonempty_directory_disappear() {
    let (_directory, root, _repository) = bare();
    let source = capture(&[
        ("assets", None),
        ("assets.css", Some(b"sibling")),
        ("assets/main.css", Some(b"child")),
    ]);
    assert_eq!(
        source
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        ["assets", "assets.css", "assets/main.css"]
    );
    let metadata = metadata();
    let report = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect("a later lexical descendant proves the directory is nonempty");
    assert_eq!(report.entries, 3);
    assert_eq!(report.output, source.inventory().identity());
    assert_eq!(
        Git.verify_commit(
            &root,
            &report.commit,
            report.output,
            &metadata,
            RevisionLimits::default()
        )
        .expect("complete independent directory read-back"),
        report
    );
}

#[test]
fn extra_commit_headers_cannot_hide_behind_matching_visible_metadata_and_tree() {
    let (_directory, root, repository) = bare();
    let source = publication();
    let metadata = metadata();
    let report = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect("exact canonical native commit");
    let original_oid = Oid::from_str(report.commit.as_str()).expect("exact original OID");
    let original = repository
        .find_commit(original_oid)
        .expect("baseline native commit");
    let odb = repository.odb().expect("local fixture object database");
    let object = odb.read(original_oid).expect("exact retained commit bytes");
    let body = std::str::from_utf8(object.data()).expect("explicit UTF-8 commit fixture");
    let (headers, message) = body
        .split_once("\n\n")
        .expect("native commit header separator");
    for extra in [
        "encoding ISO-8859-1",
        "gpgsig -----BEGIN PGP SIGNATURE-----\n fixture-only-not-an-authority-signature\n -----END PGP SIGNATURE-----",
        "x-gazette-extra ignored-by-visible-accessors",
    ] {
        let bytes = format!("{headers}\n{extra}\n\n{message}");
        let oid = odb
            .write(ObjectType::Commit, bytes.as_bytes())
            .expect("real alternative Git commit object");
        assert_ne!(oid, original_oid);
        let native = repository
            .find_commit(oid)
            .expect("native parser accepts additional commit headers");
        assert_eq!(native.tree_id(), original.tree_id());
        assert_eq!(native.parent_count(), original.parent_count());
        assert_eq!(native.message_bytes(), original.message_bytes());
        assert_eq!(native.author().name_bytes(), original.author().name_bytes());
        assert_eq!(native.author().when(), original.author().when());
        assert_eq!(
            native.committer().email_bytes(),
            original.committer().email_bytes()
        );
        let before = snapshot(&root, true);
        let error = Git
            .verify_commit(
                &root,
                &oid.to_string()
                    .parse()
                    .expect("complete alternative commit coordinate"),
                report.output,
                &metadata,
                RevisionLimits::default(),
            )
            .expect_err("unlisted headers change the exact deterministic recipe");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(&root, true), before);
    }
}

#[test]
fn exact_existing_parent_and_changed_metadata_have_distinct_oids_without_moving_head() {
    let (_directory, root, repository) = bare();
    let source = publication();
    let original = metadata();
    let parent = Git
        .prepare_commit(&root, source.as_input(), &original, write_options())
        .expect("detached root");
    let before = snapshot(&root, false);
    let mut child_metadata = original.clone();
    child_metadata.parent = Some(parent.commit.clone());
    child_metadata.message = "Explicit child\n".into();
    let child = Git
        .prepare_commit(&root, source.as_input(), &child_metadata, write_options())
        .expect("explicit existing local parent");
    assert_ne!(child.commit, parent.commit);
    let native = repository
        .find_commit(Oid::from_str(child.commit.as_str()).expect("child OID"))
        .expect("child object");
    assert_eq!(native.parent_count(), 1);
    assert_eq!(
        native
            .parent_id(0)
            .expect("single exact parent")
            .to_string(),
        parent.commit.to_string()
    );
    assert_eq!(child.parent, Some(parent.commit));
    for changed in [
        {
            let mut value = child_metadata.clone();
            value.author.seconds += 1;
            value
        },
        {
            let mut value = child_metadata.clone();
            value.committer.offset_minutes = 0;
            value
        },
        {
            let mut value = child_metadata.clone();
            value.message.push('\n');
            value
        },
    ] {
        let error = Git
            .verify_commit(
                &root,
                &child.commit,
                child.output,
                &changed,
                RevisionLimits::default(),
            )
            .expect_err("old commit cannot satisfy changed exact recipe");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        let replacement = Git
            .prepare_commit(&root, source.as_input(), &changed, write_options())
            .expect("new explicitly permitted detached recipe");
        assert_ne!(replacement.commit, child.commit);
    }
    assert_eq!(snapshot(&root, false), before);
    assert_eq!(repository.references().expect("no refs created").count(), 0);
}

#[test]
fn explicit_parent_does_not_leak_old_entries_into_a_complete_replacement_tree() {
    let (_directory, root, repository) = bare();
    let original = publication();
    let parent = Git
        .prepare_commit(&root, original.as_input(), &metadata(), write_options())
        .expect("actual parent with the complete former publication");
    let parent_oid = Oid::from_str(parent.commit.as_str()).expect("exact parent coordinate");
    let native_parent = repository
        .find_commit(parent_oid)
        .expect("native parent remains available");
    let parent_tree = native_parent.tree().expect("complete former tree");
    let old_blob = parent_tree
        .get_name("payload.txt")
        .expect("old regular file")
        .id();
    let before = snapshot(&root, false);
    let replacement = capture(&[
        ("release", None),
        ("release/current.css", Some(b"body { color: blue; }\n")),
        ("index.html", Some(b"<h1>Entirely new output</h1>\n")),
    ]);
    let mut recipe = metadata();
    recipe.parent = Some(parent.commit.clone());
    recipe.message = "Complete replacement\n".into();
    let child = Git
        .prepare_commit(&root, replacement.as_input(), &recipe, write_options())
        .expect("fresh in-memory index contains only the replacement");
    assert_eq!(child.output, replacement.inventory().identity());
    assert_eq!(child.entries, 3);
    assert_eq!(child.bytes, replacement.inventory().total_bytes());
    assert_eq!(child.parent, Some(parent.commit.clone()));
    let native_child = repository
        .find_commit(Oid::from_str(child.commit.as_str()).expect("child coordinate"))
        .expect("actual replacement commit");
    assert_eq!(
        native_child.parent_id(0).expect("explicit retained parent"),
        parent_oid
    );
    let tree = native_child
        .tree()
        .expect("complete native replacement tree");
    assert_eq!(tree.len(), 2, "exactly one root file and one new directory");
    for entry in original.inventory().entries() {
        assert!(
            tree.get_path(Path::new(entry.path.as_str())).is_err(),
            "omitted former entry {} must not survive",
            entry.path
        );
    }
    for entry in replacement.inventory().entries() {
        let native = tree
            .get_path(Path::new(entry.path.as_str()))
            .expect("every replacement entry exists");
        match entry.content {
            Content::Directory => {
                assert_eq!(native.filemode(), 0o040000);
                assert_eq!(
                    repository
                        .find_tree(native.id())
                        .expect("new child tree")
                        .len(),
                    1
                );
            }
            Content::File { .. } => {
                assert_eq!(native.filemode(), 0o100644);
                let blob = repository
                    .find_blob(native.id())
                    .expect("replacement file blob");
                assert_eq!(Some(blob.content()), replacement.file(&entry.path));
            }
        }
    }
    assert!(
        repository.find_blob(old_blob).is_ok(),
        "old unreferenced blob custody is distinct from current tree membership"
    );
    assert_eq!(
        Git.verify_commit(
            &root,
            &child.commit,
            child.output,
            &recipe,
            RevisionLimits::default()
        )
        .expect("complete replacement read-back"),
        child
    );

    let mut sibling_recipe = recipe.clone();
    sibling_recipe.message = "Same output, independently specified sibling\n".into();
    let sibling = Git
        .prepare_commit(
            &root,
            replacement.as_input(),
            &sibling_recipe,
            write_options(),
        )
        .expect("same original parent, not the last prepared commit");
    let repeat = Git
        .prepare_commit(
            &root,
            replacement.as_input(),
            &sibling_recipe,
            write_options(),
        )
        .expect("identical sibling recipe is deterministic");
    assert_eq!(sibling, repeat);
    assert_ne!(sibling.commit, child.commit);
    assert_eq!(sibling.parent, Some(parent.commit));
    let native_sibling = repository
        .find_commit(Oid::from_str(sibling.commit.as_str()).expect("sibling coordinate"))
        .expect("actual sibling");
    assert_eq!(native_sibling.tree_id(), native_child.tree_id());
    assert_eq!(
        native_sibling
            .parent_id(0)
            .expect("original parent, not inferred HEAD"),
        parent_oid
    );

    let empty = capture(&[]);
    recipe.message = "Complete empty output\n".into();
    let empty_child = Git
        .prepare_commit(&root, empty.as_input(), &recipe, write_options())
        .expect("empty root inventory is valid without unrepresentable empty subdirectories");
    assert_eq!(empty_child.entries, 0);
    assert_eq!(empty_child.bytes, 0);
    assert_eq!(empty_child.output, empty.inventory().identity());
    let native_empty = repository
        .find_commit(Oid::from_str(empty_child.commit.as_str()).expect("empty output coordinate"))
        .expect("native empty replacement commit");
    assert!(native_empty.tree().expect("native empty tree").is_empty());
    assert_eq!(
        Git.verify_commit(
            &root,
            &empty_child.commit,
            empty_child.output,
            &recipe,
            RevisionLimits::default()
        )
        .expect("empty complete output read-back"),
        empty_child
    );
    assert_eq!(snapshot(&root, false), before);
    assert_eq!(
        repository
            .references()
            .expect("detached preparation creates no refs")
            .count(),
        0
    );
}

#[test]
fn preflight_permissions_empty_directories_invalid_metadata_and_limits_write_no_objects() {
    let (_directory, root, _repository) = bare();
    let source = publication();
    let metadata = metadata();
    let before = snapshot(&root, true);
    let empty_directory = capture(&[("empty", None)]);
    let failure = Git
        .prepare_commit(
            &root,
            empty_directory.as_input(),
            &metadata,
            write_options(),
        )
        .expect_err("empty directories cannot disappear from an exact Git projection");
    assert_eq!(failure.effect, CommitEffect::NotAttempted);
    assert_eq!(snapshot(&root, true), before);
    for options in [
        CommitOptions::default(),
        CommitOptions {
            limits: RevisionLimits {
                metadata_bytes: 0,
                ..Default::default()
            },
            ..write_options()
        },
        CommitOptions {
            limits: RevisionLimits {
                content: Limits {
                    entries: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..write_options()
        },
        CommitOptions {
            limits: RevisionLimits {
                content: Limits {
                    file_bytes: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..write_options()
        },
    ] {
        let error = Git
            .prepare_commit(&root, source.as_input(), &metadata, options)
            .expect_err("explicit permission or input bounds refuse before writes");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(&root, true), before);
    }
    for bad in [
        {
            let mut value = metadata.clone();
            value.author.seconds = -1;
            value
        },
        {
            let mut value = metadata.clone();
            value.author.offset_minutes = 1440;
            value
        },
        {
            let mut value = metadata.clone();
            value.committer.offset_minutes = -1440;
            value
        },
        {
            let mut value = metadata.clone();
            value.author.name.clear();
            value
        },
        {
            let mut value = metadata.clone();
            value.committer.email.push('\n');
            value
        },
        {
            let mut value = metadata.clone();
            value.message.push('\0');
            value
        },
        {
            let mut value = metadata.clone();
            value.parent = Some(
                "cc".repeat(20)
                    .parse()
                    .expect("full missing parent coordinate"),
            );
            value
        },
    ] {
        let error = Git
            .prepare_commit(&root, source.as_input(), &bad, write_options())
            .expect_err("invalid or unresolved exact commit recipe");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(&root, true), before);
    }
}

#[test]
fn selected_repository_must_already_exist_be_bare_exact_and_not_use_alternates() {
    let (directory, root, _repository) = bare();
    let source = publication();
    let metadata = metadata();
    let ordinary = directory.path().join("ordinary");
    Repository::init(&ordinary).expect("test-owned nonbare repository");
    let link = directory.path().join("linked.git");
    symlink(&root, &link).expect("test-only repository symlink");
    let missing = directory.path().join("missing.git");
    for path in [&ordinary, &link, &missing, &root.join("objects")] {
        let before = snapshot(directory.path(), true);
        let error = Git
            .prepare_commit(path, source.as_input(), &metadata, write_options())
            .expect_err("exact bare repository boundary");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(directory.path(), true), before);
    }
    assert!(
        !missing.exists(),
        "adapter is not repository initialization"
    );
    let (_alternate_directory, alternate, _alternate_repository) = bare();
    fs::write(
        root.join("objects/info/alternates"),
        format!("{}\n", alternate.join("objects").display()),
    )
    .expect("test-only alternate object database configuration");
    let before = snapshot(&root, true);
    let error = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect_err("alternate repository must not supply preparation evidence");
    assert_eq!(error.effect, CommitEffect::NotAttempted);
    assert_eq!(snapshot(&root, true), before);
}

#[test]
fn independent_readback_rejects_wrong_output_wrong_recipe_missing_commit_and_wrong_object_type() {
    let (_directory, root, repository) = bare();
    let source = publication();
    let metadata = metadata();
    let report = Git
        .prepare_commit(&root, source.as_input(), &metadata, write_options())
        .expect("actual detached commit");
    let other = capture(&[("different.txt", Some(b"different retained output"))]);
    let before = snapshot(&root, true);
    let error = Git
        .verify_commit(
            &root,
            &report.commit,
            other.inventory().identity(),
            &metadata,
            RevisionLimits::default(),
        )
        .expect_err("wrong expected output is not silently adopted");
    assert_eq!(error.effect, CommitEffect::NotAttempted);
    for limits in [
        RevisionLimits {
            metadata_bytes: 0,
            ..Default::default()
        },
        RevisionLimits {
            content: Limits {
                total_bytes: 0,
                ..Default::default()
            },
            ..Default::default()
        },
    ] {
        let error = Git
            .verify_commit(&root, &report.commit, report.output, &metadata, limits)
            .expect_err("read-back limits cannot return partial evidence");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
    }
    let missing = "dd".repeat(20).parse().expect("full missing OID");
    let blob = repository
        .blob(b"not a commit")
        .expect("test-only wrong-type object");
    // This deliberate fixture write precedes the no-repair/no-write observation.
    let after_fixture = snapshot(&root, true);
    for commit in [
        missing,
        blob.to_string().parse().expect("full blob coordinate"),
    ] {
        let error = Git
            .verify_commit(
                &root,
                &commit,
                report.output,
                &metadata,
                RevisionLimits::default(),
            )
            .expect_err("an OID spelling is not commit evidence");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(&root, true), after_fixture);
    }
    assert_ne!(
        after_fixture, before,
        "wrong-type fixture really inserted a native object"
    );
    let mut wrong_parent = metadata;
    wrong_parent.parent = Some(
        blob.to_string()
            .parse()
            .expect("syntactically valid noncommit parent"),
    );
    let error = Git
        .prepare_commit(&root, source.as_input(), &wrong_parent, write_options())
        .expect_err("parent must resolve to a native commit before object writes");
    assert_eq!(error.effect, CommitEffect::NotAttempted);
    assert_eq!(snapshot(&root, true), after_fixture);
}

#[test]
fn native_executable_symlink_gitlink_and_empty_directory_trees_are_not_normalized_into_valid_output()
 {
    let (_directory, root, repository) = bare();
    let metadata = metadata();
    let source = capture(&[("entry", Some(b"literal blob"))]);
    let blob = repository
        .blob(b"literal blob")
        .expect("fixture regular blob");
    let empty_tree = repository
        .treebuilder(None)
        .expect("native empty builder")
        .write()
        .expect("native empty tree");
    let base = native_commit(&repository, empty_tree, &metadata);
    for (mode, target) in [
        (0o100755, blob),
        (0o120000, blob),
        (0o160000, base),
        (0o040000, empty_tree),
    ] {
        let mut builder = repository
            .treebuilder(None)
            .expect("fixture unsupported-mode tree");
        builder
            .insert("entry", target, mode)
            .expect("native Git accepts this mode");
        let commit = native_commit(
            &repository,
            builder.write().expect("native unsupported profile tree"),
            &metadata,
        );
        let before = snapshot(&root, true);
        let empty_directory = capture(&[("entry", None)]);
        let expected = if mode == 0o040000 {
            empty_directory.inventory().identity()
        } else {
            source.inventory().identity()
        };
        let error = Git
            .verify_commit(
                &root,
                &commit
                    .to_string()
                    .parse()
                    .expect("native commit coordinate"),
                expected,
                &metadata,
                RevisionLimits::default(),
            )
            .expect_err("unsupported native profile must fail rather than normalize or omit");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert_eq!(snapshot(&root, true), before);
    }
}

#[test]
fn failed_native_object_write_reports_possible_objects_without_fabricating_a_commit() {
    let (_directory, root, repository) = bare();
    let payload = b"write obstruction fixture";
    let source = capture(&[("page.html", Some(payload))]);
    let oid = Oid::hash_object(ObjectType::Blob, payload)
        .expect("native Git object coordinate")
        .to_string();
    // An ordinary file blocks this one loose-object fanout directory. It is not
    // a permission assumption and requires no race, hook, mock or global setting.
    fs::write(root.join("objects").join(&oid[..2]), b"not a directory")
        .expect("test-owned native write obstruction");
    let before = snapshot(&root, false);
    let error = Git
        .prepare_commit(&root, source.as_input(), &metadata(), write_options())
        .expect_err("native ODB write must fail");
    assert_eq!(error.effect, CommitEffect::ObjectsMayRemain);
    assert_eq!(error.reason, "local blob write failed");
    assert_eq!(snapshot(&root, false), before);
    assert_eq!(
        repository
            .references()
            .expect("refs remain untouched")
            .count(),
        0
    );
}

#[test]
fn missing_or_corrupt_retained_blobs_fail_readback_without_repair() {
    for corrupt in [false, true] {
        let (_directory, root, repository) = bare();
        let source = capture(&[("page.html", Some(b"retained page canary"))]);
        let metadata = metadata();
        let report = Git
            .prepare_commit(&root, source.as_input(), &metadata, write_options())
            .expect("complete native object preparation");
        let commit = repository
            .find_commit(Oid::from_str(report.commit.as_str()).expect("commit OID"))
            .expect("actual prepared commit");
        let tree = commit.tree().expect("prepared tree");
        let blob = tree
            .get_name("page.html")
            .expect("prepared file")
            .id()
            .to_string();
        drop(tree);
        drop(commit);
        drop(repository);
        let object = root.join("objects").join(&blob[..2]).join(&blob[2..]);
        assert!(object.is_file(), "fixture uses a real loose blob");
        if corrupt {
            fs::set_permissions(&object, fs::Permissions::from_mode(0o600))
                .expect("make only the disposable loose-object fixture writable");
            fs::write(&object, b"corrupt object canary")
                .expect("deliberately corrupt disposable blob");
        } else {
            fs::remove_file(&object).expect("deliberately remove disposable blob");
        }
        let before = snapshot(&root, true);
        let error = Git
            .verify_commit(
                &root,
                &report.commit,
                report.output,
                &metadata,
                RevisionLimits::default(),
            )
            .expect_err("complete read-back cannot ignore corrupt or missing file data");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert!(!error.to_string().contains("canary"));
        assert_eq!(snapshot(&root, true), before);
    }
}
