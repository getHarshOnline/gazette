//! Synchronize existing native output without adopting creation or cleanup ownership.

use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

use gazette_adapter_fs::FileSystem;
use gazette_core::{
    release::WorktreeOutputSyncOptions,
    source::{Inventory, Limits},
};
use gazette_ports::release::{WorktreeOutputSync, WorktreeReader};
use tempfile::TempDir;

struct Fixture {
    parent: TempDir,
    root: PathBuf,
    expected: Inventory,
}

fn private_directory(path: &Path) {
    fs::create_dir(path).expect("create disposable directory");
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).expect("private directory");
}

fn private_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write disposable file");
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).expect("private file");
}

impl Fixture {
    fn new() -> Self {
        let parent = tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir()
            .expect("private parent");
        let root = parent.path().join("deployment");
        private_directory(&root);
        private_directory(&root.join("nested"));
        private_file(&root.join("nested/page"), b"exact\r\n\0\xff");
        private_file(&root.join(".gitignore"), b"*.private\n");
        private_file(&root.join("ignored.private"), b"still output");
        private_directory(&root.join(".git"));
        private_file(&root.join(".git/private"), b"Git owns these bytes");
        let expected = FileSystem
            .read_worktree(&root, &[], Limits::default())
            .expect("native expected output");
        Self {
            parent,
            root,
            expected,
        }
    }

    fn synchronize(
        &self,
        options: WorktreeOutputSyncOptions,
    ) -> Result<Inventory, gazette_core::release::WorktreeSyncError> {
        FileSystem.synchronize_worktree_output(&self.root, &self.expected, &[], options)
    }
}

fn options() -> WorktreeOutputSyncOptions {
    WorktreeOutputSyncOptions {
        allow_synchronize: true,
        ..Default::default()
    }
}

#[test]
fn exact_private_output_synchronizes_repeatedly_without_altering_or_adopting_any_bytes() {
    let fixture = Fixture::new();
    let before = fs::metadata(fixture.root.join("nested/page")).expect("native identity");
    for _ in 0..2 {
        assert_eq!(
            fixture
                .synchronize(options())
                .expect("real native synchronization"),
            fixture.expected
        );
    }
    let after = fs::metadata(fixture.root.join("nested/page")).expect("same native identity");
    assert_eq!(
        (before.dev(), before.ino(), before.mode()),
        (after.dev(), after.ino(), after.mode())
    );
    assert_eq!(
        fs::read(fixture.root.join("nested/page")).expect("original binary payload"),
        b"exact\r\n\0\xff"
    );
    assert_eq!(
        fs::read(fixture.root.join(".git/private")).expect("Git metadata untouched"),
        b"Git owns these bytes"
    );
    assert_eq!(
        fs::read(fixture.root.join("ignored.private")).expect("ignored output retained"),
        b"still output"
    );
}

#[test]
fn permission_inventory_and_limits_refuse_before_any_flush_without_repair() {
    let fixture = Fixture::new();
    assert!(
        !fixture
            .synchronize(Default::default())
            .expect_err("explicit permission required")
            .attempted
    );
    for limits in [
        Limits {
            entries: 0,
            ..Default::default()
        },
        Limits {
            depth: 1,
            ..Default::default()
        },
        Limits {
            file_bytes: 1,
            ..Default::default()
        },
        Limits {
            total_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(
            !fixture
                .synchronize(WorktreeOutputSyncOptions {
                    limits,
                    ..options()
                })
                .expect_err("bounded before synchronization")
                .attempted
        );
    }
    fs::write(fixture.root.join("nested/page"), b"changed").expect("disposable content drift");
    assert!(
        !fixture
            .synchronize(options())
            .expect_err("complete inventory mismatch")
            .attempted
    );
    assert_eq!(
        fs::read(fixture.root.join("nested/page")).expect("not repaired"),
        b"changed"
    );
}

#[test]
fn private_modes_hardlinks_links_and_special_files_are_not_adopted_or_fixed() {
    for case in 0..7 {
        let fixture = Fixture::new();
        let entry = fixture.root.join("nested/page");
        match case {
            0 => fs::set_permissions(&entry, fs::Permissions::from_mode(0o644))
                .expect("nonprivate file"),
            1 => fs::set_permissions(
                fixture.root.join("nested"),
                fs::Permissions::from_mode(0o755),
            )
            .expect("nonprivate directory"),
            2 => fs::set_permissions(&fixture.root, fs::Permissions::from_mode(0o755))
                .expect("nonprivate root"),
            3 => fs::set_permissions(fixture.parent.path(), fs::Permissions::from_mode(0o755))
                .expect("nonprivate parent"),
            4 => fs::hard_link(&entry, fixture.parent.path().join("external-hardlink"))
                .expect("shared output inode"),
            5 => symlink("nested/page", fixture.root.join("link")).expect("output symlink"),
            _ => {
                let _socket = std::os::unix::net::UnixListener::bind(fixture.root.join("socket"))
                    .expect("disposable special-file socket");
            }
        }
        assert!(
            !fixture
                .synchronize(options())
                .expect_err("unsafe output refused before flush")
                .attempted,
            "case {case}"
        );
        assert_eq!(
            fs::read(&entry).expect("preserved original"),
            b"exact\r\n\0\xff"
        );
        assert!(fixture.root.join(".git/private").exists());
    }
}

#[test]
fn metadata_must_be_literal_real_root_directory_and_nested_metadata_is_never_ignored() {
    for case in 0..4 {
        let fixture = Fixture::new();
        let git = fixture.root.join(".git");
        match case {
            0 => {
                fs::rename(&git, fixture.parent.path().join("saved-metadata"))
                    .expect("remove exact metadata directory from fixture");
            }
            1 => {
                fs::rename(&git, fixture.parent.path().join("saved-metadata"))
                    .expect("hold metadata");
                private_file(&git, b"gitdir: elsewhere");
            }
            2 => {
                let saved = fixture.parent.path().join("saved-metadata");
                fs::rename(&git, &saved).expect("hold metadata");
                symlink(&saved, &git).expect("metadata link");
            }
            _ => private_directory(&fixture.root.join("nested/.git")),
        }
        assert!(
            !fixture
                .synchronize(options())
                .expect_err("unsafe metadata namespace")
                .attempted
        );
        assert_eq!(
            fs::read(fixture.root.join("nested/page")).expect("output not cleaned"),
            b"exact\r\n\0\xff"
        );
    }
}

#[test]
fn protected_source_overlap_and_root_alias_fail_before_synchronization() {
    let fixture = Fixture::new();
    for source in [
        &fixture.root,
        &fixture.root.join("nested"),
        &fixture.parent.path().to_owned(),
    ] {
        let error = FileSystem
            .synchronize_worktree_output(
                &fixture.root,
                &fixture.expected,
                std::slice::from_ref(source),
                options(),
            )
            .expect_err("protected namespace overlap");
        assert!(!error.attempted);
    }
    let alias = fixture.parent.path().join("alias");
    symlink(&fixture.root, &alias).expect("disposable root alias");
    assert!(
        !FileSystem
            .synchronize_worktree_output(&alias, &fixture.expected, &[], options())
            .expect_err("root alias refused")
            .attempted
    );
    assert_eq!(
        FileSystem
            .read_worktree(&fixture.root, &[], Limits::default())
            .expect("unaltered output"),
        fixture.expected
    );
}
