//! Exact native staging locators remain separate from the retained content identity.

use std::{
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::Path,
    process::Command,
};

use gazette_sdk::source::{self, Limits, StageLocation};
use serde_json::Value;
use tempfile::tempdir;

use super::{children, limits};

fn command(mode: &str, authored: &Path, parent: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gazette"));
    command.arg(mode).arg(authored).arg(parent).args([
        "--workspace",
        "fixture-workspace",
        "--site",
        "fixture-site",
        "--authored-only",
    ]);
    command
}

#[test]
fn named_cli_and_sdk_stages_retain_exact_bytes_without_putting_the_locator_in_identity() {
    let authored = tempdir().expect("authored input");
    fs::create_dir_all(authored.path().join("nested/empty")).expect("authored directories");
    fs::write(
        authored.path().join("nested/page.md"),
        b"# Exact retained bytes\r\n",
    )
    .expect("page");
    fs::write(authored.path().join("asset.bin"), [0, 255, 1]).expect("binary asset");
    let before = source::inspect(authored.path(), None, Limits::default()).expect("source before");
    let parent = tempdir().expect("explicit parent");
    fs::write(parent.path().join("unrelated"), b"preserve").expect("unrelated entry");
    let prepared = source::prepare(Vec::new(), limits()).expect("explicit empty project set");
    let context = super::super::context(&prepared);
    let sdk = source::stage(
        &prepared,
        &context,
        authored.path(),
        StageLocation::Named {
            parent: parent.path(),
            name: "sdk-exact",
        },
        Limits::default(),
    )
    .expect("actual SDK named creation");
    let expected = serde_json::to_value(sdk.inventory()).expect("SDK inventory");
    assert_eq!(
        sdk.path(),
        parent
            .path()
            .canonicalize()
            .expect("parent")
            .join("sdk-exact")
    );
    let sdk = sdk.retain().expect("synchronized SDK retention");
    let sdk_path = sdk.path().to_owned();
    drop(sdk);
    assert_eq!(
        source::inspect(&sdk_path, None, Limits::default()).expect("retained SDK read-back"),
        before
    );
    for name in ["first-exact", "second-exact"] {
        let output = command("stage", authored.path(), parent.path())
            .args(["--create-stage", "--name", name])
            .output()
            .expect("direct CLI named stage");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let receipt: Value = serde_json::from_slice(&output.stdout).expect("retained receipt");
        assert_eq!(receipt["status"], "retained");
        let exact = parent.path().canonicalize().expect("parent").join(name);
        assert_eq!(receipt["directory"], exact.to_str().expect("exact locator"));
        assert_eq!(receipt["inventory"], expected);
        let observed = Command::new(env!("CARGO_BIN_EXE_gazette"))
            .arg("inspect-source")
            .arg(&exact)
            .output()
            .expect("independent CLI read-back");
        assert!(observed.status.success(), "{observed:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&observed.stdout).expect("read-back inventory"),
            expected
        );
        assert_eq!(
            fs::read(exact.join("nested/page.md")).expect("unchanged newline bytes"),
            b"# Exact retained bytes\r\n"
        );
    }
    assert_eq!(
        children(parent.path()).len(),
        4,
        "three exact stages plus unrelated entry"
    );
    assert_eq!(
        fs::read(parent.path().join("unrelated")).expect("unrelated preserved"),
        b"preserve"
    );
    assert_eq!(
        source::inspect(authored.path(), None, Limits::default()).expect("source after"),
        before
    );
}

#[test]
fn existing_named_file_directory_and_symlink_are_not_adopted_or_replaced() {
    let authored = tempdir().expect("source");
    fs::write(authored.path().join("page.md"), b"source unchanged").expect("page");
    let before_source =
        source::inspect(authored.path(), None, Limits::default()).expect("source before");
    for kind in ["file", "directory", "symlink"] {
        let parent = tempdir().expect("parent");
        let exact = parent.path().join("claimed");
        match kind {
            "file" => fs::write(&exact, b"existing file").expect("existing file"),
            "directory" => {
                fs::create_dir(&exact).expect("existing directory");
                fs::write(exact.join("keep"), b"existing contents").expect("existing contents");
            }
            _ => symlink(authored.path(), &exact).expect("existing symlink"),
        }
        let metadata = fs::symlink_metadata(&exact).expect("original inode");
        let before = children(parent.path());
        let output = command("stage", authored.path(), parent.path())
            .args(["--create-stage", "--name", "claimed"])
            .output()
            .expect("collision attempt");
        assert_eq!(output.status.code(), Some(1), "{kind}: {output:?}");
        assert!(output.stdout.is_empty(), "no success receipt");
        assert_eq!(
            children(parent.path()),
            before,
            "no randomly named fallback"
        );
        let after = fs::symlink_metadata(&exact).expect("original remains");
        assert_eq!((after.dev(), after.ino()), (metadata.dev(), metadata.ino()));
        match kind {
            "file" => assert_eq!(fs::read(&exact).expect("preserved file"), b"existing file"),
            "directory" => assert_eq!(
                fs::read(exact.join("keep")).expect("preserved contents"),
                b"existing contents"
            ),
            _ => assert_eq!(
                fs::read_link(&exact).expect("preserved link"),
                authored.path()
            ),
        }
    }
    assert_eq!(
        source::inspect(authored.path(), None, Limits::default()).expect("source after"),
        before_source
    );
}

#[test]
fn invalid_names_and_check_only_named_requests_create_nothing() {
    let authored = tempdir().expect("source");
    fs::write(authored.path().join("page.md"), b"source").expect("page");
    let owner = tempdir().expect("bounded invalid-locator fixture");
    let parent = tempfile::tempdir_in(owner.path()).expect("parent");
    let absolute = owner.path().join("absolute");
    let before_owner = children(owner.path());
    let before = source::inspect(authored.path(), None, Limits::default()).expect("source before");
    for name in [
        "",
        ".",
        "..",
        "../escape",
        "nested/child",
        absolute.to_str().expect("absolute fixture path"),
        "back\\slash",
    ] {
        let output = command("stage", authored.path(), parent.path())
            .args(["--create-stage", "--name", name])
            .output()
            .expect("invalid name");
        assert!(!output.status.success(), "{name:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            children(parent.path()).is_empty(),
            "{name:?}: no stage or parent creation"
        );
        assert_eq!(
            children(owner.path()),
            before_owner,
            "no escaped named stage"
        );
    }
    let output = command("check-stage", authored.path(), parent.path())
        .args(["--name", "must-not-exist"])
        .output()
        .expect("check-only rejection");
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(children(parent.path()).is_empty());
    assert_eq!(
        source::inspect(authored.path(), None, Limits::default()).expect("source after"),
        before
    );
}
