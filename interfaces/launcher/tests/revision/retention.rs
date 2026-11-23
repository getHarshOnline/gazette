//! Synchronized retention through the real SDK; generated evidence never grants cleanup.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

use gazette_sdk::source::{self, Limits, StageError};
use tempfile::tempdir;

use super::{Fixture, context, limits, request};

#[test]
fn retained_native_stage_keeps_generated_evidence_and_survives_value_drop() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("sources");
    let context = context(&prepared);
    let authored = tempdir().expect("authored");
    fs::create_dir_all(authored.path().join("nested/empty")).expect("empty descendants");
    fs::write(authored.path().join("nested/page.md"), b"authored bytes").expect("authored page");
    let parent = tempdir().expect("staging");
    let before =
        source::inspect(authored.path(), None, Limits::default()).expect("authored before");
    let staged = source::stage(
        &prepared,
        &context,
        authored.path(),
        source::StageLocation::Fresh(parent.path()),
        Limits::default(),
    )
    .expect("stage");
    let pin = super::materialization::manifest_pin(&staged, "project");
    let expected = serde_json::to_value(staged.inventory()).expect("expected tree");
    let path = staged.path().to_owned();
    let retained = staged.retain().expect("synchronized retention");
    assert_eq!(retained.path(), path);
    assert_eq!(
        serde_json::to_value(retained.inventory()).expect("retained tree"),
        expected
    );
    drop(retained);
    assert!(path.join("nested/empty").is_dir());
    assert_eq!(
        serde_json::to_value(source::inspect(&path, None, Limits::default()).expect("after drop"))
            .expect("read-back"),
        expected
    );
    let verified = source::verify_generated_tree(
        &path.join("project"),
        &context
            .binding(&"project".parse().expect("target"))
            .expect("binding"),
        pin,
        Limits::default(),
    )
    .expect("independently pinned docs");
    assert_eq!(
        verified.source().inventory(),
        prepared.placements()[0].source().inventory()
    );
    assert_eq!(
        source::inspect(authored.path(), None, Limits::default()).expect("authored after"),
        before
    );
    let later = source::stage(
        &prepared,
        &context,
        authored.path(),
        source::StageLocation::Fresh(parent.path()),
        Limits::default(),
    )
    .expect("independent later stage");
    assert_ne!(later.path(), path);
    later.close().expect("only later stage cleanup");
    assert_eq!(
        serde_json::to_value(
            source::inspect(&path, None, Limits::default()).expect("still retained")
        )
        .expect("tree"),
        expected
    );
}

#[test]
fn failed_retention_preserves_changed_and_unowned_entries_without_cleanup() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("sources");
    let context = context(&prepared);
    for mutation in [
        "bytes",
        "extra",
        "file replacement",
        "root replacement",
        "file mode",
        "directory mode",
        "root mode",
        "hard link",
        "symlink",
    ] {
        let authored = tempdir().expect("authored");
        let parent = tempdir().expect("staging");
        let staged = source::stage(
            &prepared,
            &context,
            authored.path(),
            source::StageLocation::Fresh(parent.path()),
            Limits::default(),
        )
        .expect("stage");
        let path = staged.path().to_owned();
        let file = path.join("project/readme.md");
        match mutation {
            "bytes" => fs::write(&file, b"xyz").expect("changed bytes"),
            "extra" => fs::write(path.join("unowned"), b"keep").expect("extra"),
            "file replacement" => {
                fs::rename(&file, parent.path().join("original")).expect("move owned inode");
                fs::write(&file, b"abc").expect("same bytes new inode");
            }
            "root replacement" => {
                fs::rename(&path, parent.path().join("original")).expect("move original");
                fs::create_dir(&path).expect("replacement");
                fs::write(path.join("unowned"), b"keep").expect("unowned");
            }
            "file mode" => fs::set_permissions(&file, fs::Permissions::from_mode(0o644))
                .expect("nonprivate file"),
            "directory mode" => {
                fs::set_permissions(path.join("project"), fs::Permissions::from_mode(0o755))
                    .expect("nonprivate directory")
            }
            "root mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("nonprivate root"),
            "hard link" => {
                fs::hard_link(&file, parent.path().join("alias")).expect("external alias")
            }
            _ => {
                fs::rename(&file, parent.path().join("original")).expect("move original");
                symlink(parent.path().join("original"), &file).expect("symlink");
            }
        }
        let result = staged.retain();
        assert!(
            matches!(result, Err(StageError::Residue { path: ref reported, cleanup: None, .. }) if reported == &path),
            "{mutation}: {result:?}"
        );
        assert!(
            path.is_dir(),
            "{mutation}: retention failure must not invoke Drop cleanup"
        );
        if mutation == "root replacement" {
            assert_eq!(
                fs::read(path.join("unowned")).expect("unowned preserved"),
                b"keep"
            );
            assert_eq!(
                fs::read(parent.path().join("original/project/readme.md"))
                    .expect("original preserved"),
                b"abc"
            );
        } else {
            assert_eq!(
                fs::read(&file).expect("file preserved"),
                if mutation == "bytes" { b"xyz" } else { b"abc" }
            );
            assert!(
                path.join("project")
                    .join(source::GENERATED_TREE_MANIFEST)
                    .is_file()
            );
        }
    }
}
