//! Synchronous native output transitions, not durable ownership or crash recovery.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use gazette_core::{
    destination::{
        Error, OutputAdministrationBinding, OutputBinding, OutputChange, OutputIdentity,
        OutputMutation, OutputObservation,
    },
    source::{CapturedSource, Content, Inventory, InventoryBuilder, Limits},
};
use gazette_ports::{
    destination::{OutputReader, OutputWriter},
    source::SourceCapture,
};
use tempfile::{TempDir, tempdir};

use crate::FileSystem;

fn tree(files: &[(&str, &[u8])], directories: &[&str]) -> (TempDir, CapturedSource) {
    let root = tempdir().expect("disposable native tree");
    for directory in directories {
        fs::create_dir_all(root.path().join(directory)).expect("explicit directory");
    }
    for (path, bytes) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().expect("file parent")).expect("native parents");
        fs::write(path, bytes).expect("native fixture bytes");
    }
    let capture = FileSystem
        .capture(root.path(), None, Limits::default())
        .expect("real confined retained capture");
    (root, capture)
}

fn content(inventory: &Inventory, path: &str) -> Option<Content> {
    inventory
        .entries()
        .iter()
        .find(|entry| entry.path.as_str() == path)
        .map(|entry| entry.content)
}

// Adversarial core/writer tests may supply a separately captured inventory.
// Compose actual native identities rather than inventing output-reader success.
fn bound_capture(root: &Path, captured: &CapturedSource) -> OutputObservation {
    let metadata = fs::metadata(root).expect("actual captured root identity");
    let identity = |metadata: &fs::Metadata| OutputIdentity::new(metadata.dev(), metadata.ino());
    let administration = fs::symlink_metadata(root.join(".git"))
        .ok()
        .map(|metadata| {
            if metadata.is_dir() {
                OutputAdministrationBinding::Directory(identity(&metadata))
            } else {
                assert!(
                    metadata.is_file(),
                    "test's administrative entry is a regular pointer"
                );
                OutputAdministrationBinding::File(identity(&metadata))
            }
        });
    let binding = OutputBinding::new(
        root.canonicalize().expect("native canonical root"),
        identity(&metadata),
        administration,
    )
    .expect("actual native facts");
    let mut inventory = InventoryBuilder::new(Limits::default());
    for entry in captured.inventory().entries() {
        inventory
            .insert(entry.path.clone(), entry.content)
            .expect("checked captured inventory");
    }
    OutputObservation::new(inventory.finish(), binding)
}

fn transitions(observed: &Inventory, expected: &Inventory, paths: &[&str]) -> Vec<OutputChange> {
    paths
        .iter()
        .map(|path| OutputChange {
            path: path.parse().expect("explicit transition path"),
            before: content(observed, path),
            after: content(expected, path),
        })
        .collect()
}

fn assert_no_temporary_entries(directory: &Path) {
    assert!(
        fs::read_dir(directory)
            .expect("actual output namespace")
            .all(|entry| !entry
                .expect("native entry")
                .file_name()
                .to_string_lossy()
                .starts_with(".gazette-write-")),
        "successful synchronous writes leave no temporary residue"
    );
}

struct PermissionRestore {
    path: PathBuf,
    original: fs::Permissions,
}

impl PermissionRestore {
    fn readonly(path: &Path) -> Self {
        let original = fs::metadata(path)
            .expect("native permission fixture")
            .permissions();
        fs::set_permissions(path, fs::Permissions::from_mode(0o500))
            .expect("read/search only, no directory writes");
        Self {
            path: path.to_owned(),
            original,
        }
    }
}

impl Drop for PermissionRestore {
    fn drop(&mut self) {
        // Restore even during an assertion unwind, before TempDir cleanup.
        let _ = fs::set_permissions(&self.path, self.original.clone());
    }
}

fn assert_git_output_root_refused(selected: &Path, actual: &Path) {
    // Source custody is deliberately independent of configured-output namespace policy.
    let observed = FileSystem
        .capture(actual, None, Limits::default())
        .expect("ordinary source capture inside the selected namespace remains supported");
    let selected_capture = FileSystem
        .capture(selected, None, Limits::default())
        .expect("source capture retains the exact selected input even through an ancestor alias");
    assert_eq!(selected_capture.inventory(), observed.inventory());
    let canonical = actual
        .canonicalize()
        .expect("actual native root coordinate");
    let assert_cause = |error: &Error| {
        let Error::Observation(crate::Error::InvalidInventory { path, reason }) = error else {
            panic!("expected the configured-output namespace invariant, got {error}");
        };
        assert_eq!(
            *reason,
            "configured output must not be inside the Git administrative namespace"
        );
        assert!(
            path == selected || path == &canonical,
            "native refusal identifies the selected spelling or its verified canonical root"
        );
    };
    let error = FileSystem
        .read_output(selected, &[], Limits::default())
        .expect_err("configured output cannot be an administrative namespace root");
    assert_cause(&error);

    let (_expected_root, expected) = tree(
        &[
            ("private.bin", b"untouched\0\xff"),
            ("index.html", b"retained output"),
        ],
        &[],
    );
    let (_generated_root, generated) = tree(&[("index.html", b"retained output")], &[]);
    let changes = transitions(observed.inventory(), expected.inventory(), &["index.html"]);
    let observation = bound_capture(actual, &observed);
    let mutation = OutputMutation::new(
        selected,
        &[],
        &observation,
        expected.inventory(),
        generated.as_input(),
        &changes,
        Limits::default(),
    )
    .expect("valid captured transition does not grant a native output-root capability");
    let error = FileSystem
        .write_output(mutation, true)
        .expect_err("writer repeats namespace admission without relying on OutputReader");
    assert_cause(&error.cause);
    assert!(error.attempted.is_empty());
    assert!(error.residue.is_none());
    assert!(!actual.join("index.html").exists());
    assert_eq!(
        fs::read(actual.join("private.bin")).expect("source bytes were not modified"),
        b"untouched\0\xff"
    );
    assert_eq!(
        FileSystem
            .capture(actual, None, Limits::default())
            .expect("independent unchanged source readback")
            .inventory(),
        observed.inventory()
    );
    assert_no_temporary_entries(actual);
}

#[test]
fn configured_output_git_namespace_roots_refuse_without_restricting_source_capture() {
    for relative in [
        ".git",
        ".Git",
        ".GIT",
        ".git/objects",
        ".Git/refs",
        ".git/worktrees/linked",
        ".git/",
        ".git/.",
        ".git/../ordinary",
    ] {
        // Separate roots avoid colliding .git/.Git entries on case-insensitive volumes.
        let owner = tempdir().expect("disposable namespace owner");
        // Native traversal of `.git/../ordinary` still requires `.git` to exist.
        if relative == ".git/../ordinary" {
            fs::create_dir(owner.path().join(".git")).expect("actual lexical ancestor");
        }
        let selected = owner.path().join(relative);
        let fixture = match relative {
            ".git/" | ".git/." => owner.path().join(".git"),
            ".git/../ordinary" => owner.path().join("ordinary"),
            _ => selected.clone(),
        };
        fs::create_dir_all(&fixture).expect("actual selected directory and ancestors");
        fs::write(selected.join("private.bin"), b"untouched\0\xff")
            .expect("retained private input bytes");
        let actual = selected.canonicalize().expect("actual selected root");
        assert_git_output_root_refused(&selected, &actual);
    }
}

#[test]
fn configured_output_ancestor_alias_into_git_namespace_refuses_before_any_effect() {
    let owner = tempdir().expect("disposable administrative namespace");
    let actual = owner.path().join("repository/.git/objects");
    fs::create_dir_all(&actual).expect("actual administrative descendant");
    fs::write(actual.join("private.bin"), b"untouched\0\xff")
        .expect("retained private input bytes");
    let alias = owner.path().join("alias");
    symlink(owner.path().join("repository/.git"), &alias).expect("ancestor namespace alias");
    let selected = alias.join("objects");
    assert!(
        !selected
            .components()
            .any(|component| component.as_os_str() == std::ffi::OsStr::new(".git")),
        "the selected spelling alone does not reveal its actual namespace"
    );
    assert_git_output_root_refused(&selected, &actual);
    assert_eq!(
        fs::read_link(&alias).expect("alias was neither removed nor replaced"),
        owner.path().join("repository/.git")
    );
}

#[test]
fn configured_output_git_like_names_and_ordinary_git_shaped_data_remain_usable() {
    for name in [".gitignore", ".git-data", "git", "admin", "bare.git"] {
        let owner = tempdir().expect("ordinary namespace owner");
        let selected = owner.path().join(name);
        fs::create_dir_all(selected.join("objects")).expect("ordinary object-named directory");
        fs::create_dir(selected.join("refs")).expect("ordinary reference-named directory");
        fs::write(selected.join("HEAD"), b"ref: refs/heads/main\n")
            .expect("ordinary HEAD-named bytes are not classification authority");
        fs::write(selected.join("objects/user.bin"), b"user object-named data")
            .expect("unclaimed user bytes");
        fs::write(selected.join("refs/user.txt"), b"user reference-named data")
            .expect("unclaimed user bytes");
        let observed = FileSystem
            .read_output(&selected, &[], Limits::default())
            .expect("only the exact administrative namespace component is reserved");
        let (_expected_root, expected) = tree(
            &[
                ("HEAD", b"ref: refs/heads/main\n"),
                ("objects/user.bin", b"user object-named data"),
                ("refs/user.txt", b"user reference-named data"),
                ("index.html", b"retained output"),
            ],
            &[],
        );
        let (_generated_root, generated) = tree(&[("index.html", b"retained output")], &[]);
        let changes = transitions(observed.inventory(), expected.inventory(), &["index.html"]);
        let mutation = OutputMutation::new(
            &selected,
            &[],
            &observed,
            expected.inventory(),
            generated.as_input(),
            &changes,
            Limits::default(),
        )
        .expect("exact fresh-output transition preserves all ordinary input bytes");
        assert_eq!(
            FileSystem
                .write_output(mutation, true)
                .expect("actual native writer does not heuristic-classify a repository")
                .inventory(),
            expected.inventory()
        );
        assert_eq!(
            FileSystem
                .capture(&selected, None, Limits::default())
                .expect("independent exact native byte readback")
                .inventory(),
            expected.inventory()
        );
        assert_no_temporary_entries(&selected);
    }
}

#[test]
fn removal_of_observed_git_file_is_refused_before_core_transition_replay() {
    let (root, observed) = tree(&[(".git", b"gitdir: ../private-administration\n")], &[]);
    let (_expected_root, empty) = tree(&[], &[]);
    let changes = transitions(observed.inventory(), empty.inventory(), &[".git"]);
    assert!(changes[0].before.is_some());
    assert!(changes[0].after.is_none());
    let observation = bound_capture(root.path(), &observed);
    let error = OutputMutation::new(
        root.path(),
        &[],
        &observation,
        empty.inventory(),
        empty.as_input(),
        &changes,
        Limits::default(),
    )
    .err()
    .expect("reserved path refused even when replay would remove it from final tree");
    assert!(matches!(
        error,
        Error::Observation(crate::Error::InvalidInventory { .. })
    ));
    assert_eq!(
        fs::read(root.path().join(".git")).expect("administration untouched"),
        b"gitdir: ../private-administration\n"
    );
    assert_eq!(
        fs::read_dir(root.path())
            .expect("no native effects")
            .count(),
        1
    );
}

#[test]
fn readonly_root_failure_reports_attempt_without_claiming_uncreated_residue() {
    // These native permission proofs run as the ordinary, non-privileged caller.
    let (root, _) = tree(&[("user.txt", b"untouched")], &[]);
    let observed = FileSystem
        .read_output(root.path(), &[], Limits::default())
        .expect("baseline");
    let (_expected_root, expected) = tree(
        &[("user.txt", b"untouched"), ("new.html", b"retained")],
        &[],
    );
    let (_generated_root, generated) = tree(&[("new.html", b"retained")], &[]);
    let changes = transitions(observed.inventory(), expected.inventory(), &["new.html"]);
    let mutation = OutputMutation::new(
        root.path(),
        &[],
        &observed,
        expected.inventory(),
        generated.as_input(),
        &changes,
        Limits::default(),
    )
    .expect("exact fresh-file transition");
    let permissions = PermissionRestore::readonly(root.path());
    let original_mode = permissions.original.mode();
    assert_eq!(
        FileSystem
            .read_output(root.path(), &[], Limits::default())
            .expect("read-only root remains observable"),
        observed
    );
    let error = FileSystem
        .write_output(mutation, true)
        .expect_err("native create lacks directory-write permission");
    assert_eq!(
        error
            .attempted
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["new.html"]
    );
    assert!(
        error.residue.is_none(),
        "failed EXCL open acquired no temporary entry"
    );
    assert!(
        matches!(error.cause, Error::Observation(crate::Error::Io { ref source, .. }) if source.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert!(!root.path().join("new.html").exists());
    assert_eq!(
        FileSystem
            .read_output(root.path(), &[], Limits::default())
            .expect("unchanged tree"),
        observed
    );
    assert_eq!(
        fs::read(root.path().join("user.txt")).expect("user untouched"),
        b"untouched"
    );
    assert_no_temporary_entries(root.path());
    drop(permissions);
    assert_eq!(
        fs::metadata(root.path())
            .expect("restored root permissions")
            .permissions()
            .mode(),
        original_mode
    );
}

#[test]
fn later_readonly_child_failure_retains_first_effect_and_exact_partial_evidence() {
    let (root, _) = tree(&[("user.txt", b"untouched user")], &["z-locked"]);
    fs::create_dir(root.path().join(".git")).expect("Git administration");
    fs::write(
        root.path().join(".git/private"),
        b"untouched administration",
    )
    .expect("Git bytes");
    let permissions = PermissionRestore::readonly(&root.path().join("z-locked"));
    let original_mode = permissions.original.mode();
    let observed = FileSystem
        .read_output(root.path(), &[], Limits::default())
        .expect("read-only child is valid observed content");
    let (_expected_root, expected) = tree(
        &[
            ("user.txt", b"untouched user"),
            ("a-first.html", b"first exact retained bytes"),
            ("z-locked/second.html", b"second retained bytes"),
        ],
        &[],
    );
    let (_generated_root, generated) = tree(
        &[
            ("a-first.html", b"first exact retained bytes"),
            ("z-locked/second.html", b"second retained bytes"),
        ],
        &[],
    );
    let changes = transitions(
        observed.inventory(),
        expected.inventory(),
        &["a-first.html", "z-locked/second.html"],
    );
    let mutation = OutputMutation::new(
        root.path(),
        &[],
        &observed,
        expected.inventory(),
        generated.as_input(),
        &changes,
        Limits::default(),
    )
    .expect("complete ordered transition");
    let error = FileSystem
        .write_output(mutation, true)
        .expect_err("later native directory permission failure is not success or rollback");
    assert_eq!(
        error
            .attempted
            .iter()
            .map(|path| path.as_str())
            .collect::<Vec<_>>(),
        ["a-first.html", "z-locked/second.html"]
    );
    assert!(
        error.residue.is_none(),
        "first temporary settled; failed second open acquired none"
    );
    assert!(
        matches!(error.cause, Error::Observation(crate::Error::Io { ref source, .. }) if source.kind() == std::io::ErrorKind::PermissionDenied)
    );
    assert_eq!(
        fs::read(root.path().join("a-first.html")).expect("earlier effect remains exact"),
        b"first exact retained bytes"
    );
    assert!(!root.path().join("z-locked/second.html").exists());
    assert_eq!(
        fs::read(root.path().join("user.txt")).expect("user bytes untouched"),
        b"untouched user"
    );
    assert_eq!(
        fs::read(root.path().join(".git/private")).expect("administration untouched"),
        b"untouched administration"
    );
    let after = FileSystem
        .read_output(root.path(), &[], Limits::default())
        .expect("actual partial tree");
    assert_ne!(
        after.inventory(),
        expected.inventory(),
        "failure cannot report the planned full tree"
    );
    let (_partial_root, partial) = tree(
        &[
            ("user.txt", b"untouched user"),
            ("a-first.html", b"first exact retained bytes"),
        ],
        &["z-locked"],
    );
    assert_eq!(
        after.inventory(),
        partial.inventory(),
        "exact partial effect, no hidden rollback or second file"
    );
    assert_no_temporary_entries(root.path());
    assert_no_temporary_entries(&root.path().join("z-locked"));
    drop(permissions);
    assert_eq!(
        fs::metadata(root.path().join("z-locked"))
            .expect("restored child permissions")
            .permissions()
            .mode(),
        original_mode
    );
}

#[test]
fn fresh_output_and_exact_repeat_preserve_user_files_and_git_directory_or_pointer() {
    for pointer in [false, true] {
        let (root, _) = tree(
            &[
                ("user.txt", b"existing user bytes"),
                (".gazette-owned", b"not ownership"),
            ],
            &["user-empty"],
        );
        let administration = root.path().join(".git");
        if pointer {
            fs::write(&administration, b"gitdir: ../absent-admin\n").expect("root Git pointer");
        } else {
            fs::create_dir(&administration).expect("root Git directory");
            fs::write(administration.join("private"), b"untouched Git bytes")
                .expect("administrative bytes");
            symlink("missing", administration.join("link")).expect("untraversed administration");
        }
        let observed = FileSystem
            .read_output(root.path(), &[], Limits::default())
            .expect("exact existing user tree");
        let user_directory =
            fs::metadata(root.path().join("user-empty")).expect("existing user directory identity");
        let (_expected_root, expected) = tree(
            &[
                ("user.txt", b"existing user bytes"),
                (".gazette-owned", b"not ownership"),
                ("article/index.html", b"retained HTML\n"),
                ("asset.bin", b"\0\xff\r\n"),
            ],
            &["user-empty"],
        );
        let (generated_root, generated) = tree(
            &[
                ("article/index.html", b"retained HTML\n"),
                ("asset.bin", b"\0\xff\r\n"),
            ],
            &[],
        );
        let protected = [generated_root.path().to_owned()];
        let changes = transitions(
            observed.inventory(),
            expected.inventory(),
            &["article", "article/index.html", "asset.bin"],
        );
        let mutation = OutputMutation::new(
            root.path(),
            &protected,
            &observed,
            expected.inventory(),
            generated.as_input(),
            &changes,
            Limits::default(),
        )
        .expect("explicit valid creation transition");
        let written = FileSystem
            .write_output(mutation, true)
            .expect("actual synchronous materialization");
        assert_eq!(
            written.binding(),
            observed.binding(),
            "root/admin identity survives native output effects without binding mutable root timestamps"
        );
        assert_eq!(written.inventory(), expected.inventory());
        assert_eq!(
            FileSystem
                .read_output(root.path(), &protected, Limits::default())
                .expect("independent native readback"),
            written
        );
        assert_eq!(
            fs::read(root.path().join("article/index.html")).expect("exact HTML"),
            b"retained HTML\n"
        );
        assert_eq!(
            fs::read(root.path().join("asset.bin")).expect("exact binary"),
            b"\0\xff\r\n"
        );
        let before =
            fs::metadata(root.path().join("article/index.html")).expect("written identity");
        let unchanged = OutputMutation::new(
            root.path(),
            &protected,
            &written,
            written.inventory(),
            generated.as_input(),
            &[],
            Limits::default(),
        )
        .expect("explicit empty transition");
        assert_eq!(
            FileSystem
                .write_output(unchanged, true)
                .expect("exact repeat no-op"),
            written
        );
        let after = fs::metadata(root.path().join("article/index.html")).expect("same file");
        assert_eq!(
            (
                before.dev(),
                before.ino(),
                before.mtime(),
                before.mtime_nsec()
            ),
            (after.dev(), after.ino(), after.mtime(), after.mtime_nsec())
        );
        assert_eq!(
            fs::read(root.path().join("user.txt")).expect("user bytes preserved"),
            b"existing user bytes"
        );
        assert_eq!(
            fs::read(root.path().join(".gazette-owned")).expect("marker remains user content"),
            b"not ownership"
        );
        assert!(root.path().join("user-empty").is_dir());
        let user_after =
            fs::metadata(root.path().join("user-empty")).expect("preserved user directory");
        assert_eq!(
            (user_directory.dev(), user_directory.ino()),
            (user_after.dev(), user_after.ino())
        );
        assert_no_temporary_entries(root.path());
        assert_no_temporary_entries(&root.path().join("article"));
        if pointer {
            assert_eq!(
                fs::read(administration).expect("pointer preserved"),
                b"gitdir: ../absent-admin\n"
            );
        } else {
            assert_eq!(
                fs::read(administration.join("private")).expect("Git bytes preserved"),
                b"untouched Git bytes"
            );
            assert_eq!(
                fs::read_link(administration.join("link")).expect("administrative link preserved"),
                Path::new("missing")
            );
        }
    }
}

#[test]
fn explicit_replacement_and_removal_preserve_user_directories_and_other_hardlinks() {
    let (root, _) = tree(
        &[
            ("shared/index.html", b"old retained bytes"),
            ("obsolete.html", b"explicitly removable"),
            ("shared/user.txt", b"user sibling"),
        ],
        &["user-empty"],
    );
    let external = tempdir().expect("independent hardlink holder");
    let saved = external.path().join("saved-old");
    fs::hard_link(root.path().join("shared/index.html"), &saved)
        .expect("old file also owned outside output");
    let old = fs::metadata(&saved).expect("shared inode");
    let shared_directory =
        fs::metadata(root.path().join("shared")).expect("existing parent identity");
    let observed = FileSystem
        .read_output(root.path(), &[], Limits::default())
        .expect("complete pre-effect observation");
    let (_expected_root, expected) = tree(
        &[
            ("shared/index.html", b"new retained bytes"),
            ("shared/user.txt", b"user sibling"),
        ],
        &["user-empty"],
    );
    let (_generated_root, generated) = tree(&[("shared/index.html", b"new retained bytes")], &[]);
    let changes = transitions(
        observed.inventory(),
        expected.inventory(),
        &["obsolete.html", "shared/index.html"],
    );
    let mutation = OutputMutation::new(
        root.path(),
        &[],
        &observed,
        expected.inventory(),
        generated.as_input(),
        &changes,
        Limits::default(),
    )
    .expect("explicit replacement/removal only");
    assert_eq!(
        FileSystem
            .write_output(mutation, true)
            .expect("actual replacement and removal")
            .inventory(),
        expected.inventory()
    );
    assert!(!root.path().join("obsolete.html").exists());
    assert_eq!(
        fs::read(root.path().join("shared/index.html")).expect("replaced bytes"),
        b"new retained bytes"
    );
    assert_eq!(
        fs::read(&saved).expect("other hardlink not truncated"),
        b"old retained bytes"
    );
    assert_eq!(
        fs::metadata(&saved).expect("original inode retained").ino(),
        old.ino()
    );
    assert_ne!(
        fs::metadata(root.path().join("shared/index.html"))
            .expect("replacement inode")
            .ino(),
        old.ino()
    );
    assert_eq!(
        fs::read(root.path().join("shared/user.txt")).expect("unselected sibling"),
        b"user sibling"
    );
    assert!(root.path().join("shared").is_dir());
    assert!(root.path().join("user-empty").is_dir());
    assert_eq!(
        fs::metadata(root.path().join("shared"))
            .expect("preserved parent")
            .ino(),
        shared_directory.ino()
    );
    assert_no_temporary_entries(root.path());
    assert_no_temporary_entries(&root.path().join("shared"));
}

#[test]
fn permission_and_changed_baseline_refuse_without_effects_or_residue() {
    for permitted in [false, true] {
        let (root, _) = tree(&[("user.txt", b"original")], &[]);
        let observed = FileSystem
            .read_output(root.path(), &[], Limits::default())
            .expect("baseline");
        let (_expected_root, expected) =
            tree(&[("user.txt", b"original"), ("new.html", b"new")], &[]);
        let (_generated_root, generated) = tree(&[("new.html", b"new")], &[]);
        let changes = transitions(observed.inventory(), expected.inventory(), &["new.html"]);
        let mutation = OutputMutation::new(
            root.path(),
            &[],
            &observed,
            expected.inventory(),
            generated.as_input(),
            &changes,
            Limits::default(),
        )
        .expect("valid transition before native refusal");
        if permitted {
            fs::write(root.path().join("user.txt"), b"changed after observation")
                .expect("native baseline drift");
        }
        let error = FileSystem
            .write_output(mutation, permitted)
            .expect_err("permission or stale baseline must fail before mutation");
        assert!(error.attempted.is_empty());
        assert!(error.residue.is_none());
        assert!(!root.path().join("new.html").exists());
        assert_eq!(
            fs::read_dir(root.path())
                .expect("no temporary residue")
                .count(),
            1
        );
        assert_eq!(
            fs::read(root.path().join("user.txt")).expect("never repaired"),
            if permitted {
                b"changed after observation".as_slice()
            } else {
                b"original".as_slice()
            }
        );
    }
}

#[test]
fn protected_source_overlap_and_symlink_state_refuse_before_any_output_effect() {
    for case in ["overlap", "root", "administration", "descendant"] {
        let (root, _) = tree(&[("user.txt", b"unchanged")], &[]);
        let observed = FileSystem
            .read_output(root.path(), &[], Limits::default())
            .expect("safe baseline");
        let (_expected_root, expected) =
            tree(&[("user.txt", b"unchanged"), ("new.html", b"new")], &[]);
        let (_generated_root, generated) = tree(&[("new.html", b"new")], &[]);
        let changes = transitions(observed.inventory(), expected.inventory(), &["new.html"]);
        let aliases = tempdir().expect("alias holder");
        let selected = if case == "root" {
            let alias = aliases.path().join("root");
            symlink(root.path(), &alias).expect("root alias");
            alias
        } else {
            root.path().to_owned()
        };
        let protected = if case == "overlap" {
            vec![root.path().to_owned()]
        } else {
            vec![]
        };
        if case == "administration" {
            symlink(aliases.path(), root.path().join(".git")).expect("unsafe admin link");
        }
        if case == "descendant" {
            symlink(aliases.path(), root.path().join("user-link")).expect("unsafe user link");
        }
        let mutation = OutputMutation::new(
            &selected,
            &protected,
            &observed,
            expected.inventory(),
            generated.as_input(),
            &changes,
            Limits::default(),
        )
        .expect("native boundaries remain writer obligations");
        let error = FileSystem
            .write_output(mutation, true)
            .expect_err("unsafe native state");
        assert!(error.attempted.is_empty(), "{case}: {error}");
        assert!(error.residue.is_none(), "{case}: {error}");
        if case == "overlap" {
            assert!(matches!(error.cause, Error::SourceOverlap { .. }));
        }
        assert!(!root.path().join("new.html").exists());
        assert_eq!(
            fs::read(root.path().join("user.txt")).expect("unchanged user file"),
            b"unchanged"
        );
        assert_eq!(
            fs::read_dir(aliases.path())
                .expect("no redirected effects")
                .count(),
            usize::from(case == "root")
        );
    }
}

#[test]
fn malformed_core_transitions_never_authorize_native_mutation() {
    let (root, observed) = tree(&[("page.html", b"old")], &["user-empty"]);
    let (_expected_root, expected) = tree(&[("page.html", b"new")], &["user-empty"]);
    let (_generated_root, generated) = tree(&[("page.html", b"new")], &[]);
    let observation = bound_capture(root.path(), &observed);
    let invalid = [
        vec![OutputChange {
            path: "page.html".parse().expect("path"),
            before: None,
            after: content(expected.inventory(), "page.html"),
        }],
        transitions(
            observation.inventory(),
            expected.inventory(),
            &["page.html", "page.html"],
        ),
        vec![OutputChange {
            path: "user-empty".parse().expect("directory"),
            before: Some(Content::Directory),
            after: None,
        }],
        vec![],
    ];
    for changes in &invalid {
        let error = OutputMutation::new(
            root.path(),
            &[],
            &observation,
            expected.inventory(),
            generated.as_input(),
            changes,
            Limits::default(),
        )
        .err()
        .expect("invalid transition refused by owning core contract");
        assert!(matches!(
            error,
            Error::Observation(crate::Error::InvalidInventory { .. })
        ));
    }
    let (_git_root, git) = tree(&[(".git", b"never output")], &[]);
    let changes = transitions(observed.inventory(), git.inventory(), &[".git"]);
    assert!(
        OutputMutation::new(
            root.path(),
            &[],
            &observation,
            git.inventory(),
            git.as_input(),
            &changes,
            Limits::default()
        )
        .is_err(),
        "Git administration is not a generated transition"
    );
    assert_eq!(
        fs::read(root.path().join("page.html")).expect("unchanged original"),
        b"old"
    );
    assert!(root.path().join("user-empty").is_dir());
    assert!(!root.path().join(".git").exists());
}

#[test]
fn identical_inventory_at_a_replaced_native_root_refuses_even_a_noop_without_effects() {
    for administration in ["absent", "directory", "pointer"] {
        for create in [false, true] {
            let owner = tempdir().expect("disjoint test-owned roots");
            let root = owner.path().join("output");
            fs::create_dir(&root).expect("original output root");
            fs::write(root.join("user.txt"), b"unchanged user").expect("user bytes");
            let install_admin = |path: &Path| match administration {
                "directory" => {
                    fs::create_dir(path.join(".git")).expect("actual administration");
                    fs::write(path.join(".git/private"), b"unchanged admin").expect("admin bytes");
                }
                "pointer" => {
                    fs::write(path.join(".git"), b"gitdir: ../private\n").expect("pointer");
                }
                _ => (),
            };
            install_admin(&root);
            let observed = FileSystem
                .read_output(&root, &[], Limits::default())
                .expect("bound baseline");
            let (_expected_root, expected) = if create {
                tree(
                    &[("user.txt", b"unchanged user"), ("new.html", b"generated")],
                    &[],
                )
            } else {
                tree(&[("user.txt", b"unchanged user")], &[])
            };
            let (_generated_root, generated) = if create {
                tree(&[("new.html", b"generated")], &[])
            } else {
                tree(&[], &[])
            };
            let changes = if create {
                transitions(observed.inventory(), expected.inventory(), &["new.html"])
            } else {
                Vec::new()
            };
            let mutation = OutputMutation::new(
                &root,
                &[],
                &observed,
                expected.inventory(),
                generated.as_input(),
                &changes,
                Limits::default(),
            )
            .expect("valid complete transition");
            let original = owner.path().join("held-original");
            fs::rename(&root, &original)
                .expect("preserve original inode instead of permitting reuse");
            fs::create_dir(&root).expect("identical replacement at the selected coordinate");
            fs::write(root.join("user.txt"), b"unchanged user").expect("identical user bytes");
            install_admin(&root);
            let replacement = FileSystem
                .read_output(&root, &[], Limits::default())
                .expect("safe independent replacement observation");
            assert_eq!(replacement.inventory(), observed.inventory());
            assert_eq!(
                replacement.binding().canonical_root(),
                observed.binding().canonical_root()
            );
            assert_ne!(replacement.binding().root(), observed.binding().root());
            let error = FileSystem
                .write_output(mutation, true)
                .expect_err("byte identity is not native root continuity");
            assert!(
                matches!(error.cause, Error::Observation(crate::Error::Changed(ref path)) if path == &root)
            );
            assert!(error.attempted.is_empty());
            assert!(error.residue.is_none());
            assert_eq!(
                FileSystem
                    .read_output(&root, &[], Limits::default())
                    .expect("replacement untouched"),
                replacement
            );
            assert_eq!(
                fs::read(original.join("user.txt")).expect("original root untouched"),
                b"unchanged user"
            );
            assert!(!root.join("new.html").exists());
            assert!(!original.join("new.html").exists());
            assert_no_temporary_entries(&root);
            assert_no_temporary_entries(&original);
        }
    }
}

#[test]
fn changed_git_administrative_identity_or_presence_refuses_identical_output_before_effects() {
    for case in [
        "directory-replacement",
        "pointer-replacement",
        "appearance",
        "disappearance",
        "kind-change",
    ] {
        let owner = tempdir().expect("owned output and retired administration");
        let root = owner.path().join("output");
        fs::create_dir(&root).expect("output");
        fs::write(root.join("user.txt"), b"unchanged").expect("user bytes");
        let administration = root.join(".git");
        if case == "directory-replacement" {
            fs::create_dir(&administration).expect("old administrative directory");
            fs::write(administration.join("private"), b"unchanged admin").expect("old admin bytes");
        } else if case != "appearance" {
            fs::write(&administration, b"gitdir: ../private\n").expect("original pointer");
        }
        let observed = FileSystem
            .read_output(&root, &[], Limits::default())
            .expect("bound original admin");
        let (_expected_root, expected) =
            tree(&[("user.txt", b"unchanged"), ("new.html", b"new")], &[]);
        let (_generated_root, generated) = tree(&[("new.html", b"new")], &[]);
        let changes = transitions(observed.inventory(), expected.inventory(), &["new.html"]);
        let mutation = OutputMutation::new(
            &root,
            &[],
            &observed,
            expected.inventory(),
            generated.as_input(),
            &changes,
            Limits::default(),
        )
        .expect("valid transition");
        if case != "appearance" {
            fs::rename(&administration, owner.path().join("retired-administration"))
                .expect("retain original administrative inode");
        }
        match case {
            "directory-replacement" | "kind-change" => {
                fs::create_dir(&administration).expect("different administrative directory");
                fs::write(administration.join("private"), b"unchanged admin")
                    .expect("identical admin bytes");
            }
            "pointer-replacement" | "appearance" => {
                fs::write(&administration, b"gitdir: ../private\n").expect("new pointer inode");
            }
            _ => (),
        }
        let changed = FileSystem
            .read_output(&root, &[], Limits::default())
            .expect("independent current observation");
        assert_eq!(changed.inventory(), observed.inventory());
        assert_eq!(changed.binding().root(), observed.binding().root());
        assert_ne!(
            changed.binding().administration(),
            observed.binding().administration()
        );
        let error = FileSystem
            .write_output(mutation, true)
            .expect_err("Git custody change is not hidden by excluded inventory bytes");
        assert!(
            matches!(error.cause, Error::Observation(crate::Error::Changed(ref path)) if path == &root),
            "{case}: {error}"
        );
        assert!(error.attempted.is_empty());
        assert!(error.residue.is_none());
        assert_eq!(
            FileSystem
                .read_output(&root, &[], Limits::default())
                .expect("no mutation"),
            changed
        );
        assert!(!root.join("new.html").exists());
        assert_eq!(
            fs::read(root.join("user.txt")).expect("user preserved"),
            b"unchanged"
        );
        assert_no_temporary_entries(&root);
    }
}
