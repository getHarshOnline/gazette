//! Held native planning crosses the intent boundary without recreating its observations.

use gazette_adapter_fs::FileSystem;
use gazette_core::{
    source::{CapturedSource, Content, InventoryBuilder, Limits, Sha256Digest},
    stage::{Error, RetainedStage},
};
use gazette_ports::stage::{StagePlanner, StageRetention, StageTarget};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::Path,
};

fn input() -> CapturedSource {
    let path = "entry".parse().expect("literal fixture path");
    let mut inventory = InventoryBuilder::new(Limits::default());
    inventory
        .insert(
            path,
            Content::File {
                bytes: 3,
                sha256: Sha256Digest::of(b"abc"),
            },
        )
        .expect("fixture inventory");
    CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([("entry".parse().expect("payload path"), b"abc".to_vec())]),
    )
    .expect("retained fixture input")
}

fn entries(parent: &Path) -> usize {
    fs::read_dir(parent)
        .expect("inspect fixture parent")
        .count()
}

#[test]
fn planning_is_read_only_and_retention_reuses_exact_native_stage() {
    let parent = tempfile::tempdir().expect("stage parent");
    let source = tempfile::tempdir().expect("protected source");
    fs::write(source.path().join("original"), b"source").expect("protected original");
    let before = fs::metadata(parent.path()).expect("parent identity");
    let target = FileSystem
        .plan_stage(parent.path(), ".prepared-001", &[source.path().to_owned()])
        .expect("held stage target");
    assert_eq!(
        target.address().parent,
        parent.path().canonicalize().expect("canonical parent")
    );
    assert_eq!(target.address().name, ".prepared-001");
    let address = target.address().clone();
    assert_eq!(entries(parent.path()), 0);
    let after = fs::metadata(parent.path()).expect("unchanged parent identity");
    assert_eq!(
        (
            before.dev(),
            before.ino(),
            before.mtime(),
            before.mtime_nsec()
        ),
        (after.dev(), after.ino(), after.mtime(), after.mtime_nsec())
    );
    let capture = input();
    let stage = target
        .create(capture.as_input(), Limits::default())
        .expect("create through held target");
    assert_eq!(stage.path(), address.path());
    assert_eq!(stage.inventory(), capture.inventory());
    let retained: RetainedStage =
        StageRetention::retain(stage).expect("same synchronized retention owner");
    assert_eq!(retained.path(), address.path());
    assert_eq!(retained.inventory(), capture.inventory());
    drop(retained);
    assert_eq!(
        fs::read(address.path().join("entry")).expect("retained bytes"),
        b"abc"
    );
    assert_eq!(
        fs::read(source.path().join("original")).expect("source unchanged"),
        b"source"
    );
}

#[test]
fn invalid_names_and_overlaps_refuse_during_planning_without_effects() {
    let parent = tempfile::tempdir().expect("stage parent");
    let missing = parent.path().join("absent");
    for name in [
        "",
        ".",
        "..",
        "nested/child",
        "a\\b",
        "/absolute",
        "C:drive",
        "x\n",
        "x\0",
        "%2e%2e",
        "%252f",
    ] {
        let error = FileSystem
            .plan_stage(&missing, name, &[])
            .expect_err("name rejected before missing-parent access");
        assert!(matches!(
            error,
            Error::Preflight(gazette_core::destination::Error::Observation(
                gazette_core::source::Error::InvalidName(_)
            ))
        ));
    }
    assert!(
        FileSystem
            .plan_stage(parent.path(), "prepared", &[parent.path().to_owned()])
            .is_err()
    );
    assert_eq!(entries(parent.path()), 0);
}

#[test]
fn replaced_parent_or_source_identity_blocks_creation_using_the_held_observation() {
    for changed in ["parent", "source"] {
        let fixture = tempfile::tempdir().expect("fixture roots");
        let parent = fixture.path().join("parent");
        let source = fixture.path().join("source");
        fs::create_dir(&parent).expect("stage parent");
        fs::create_dir(&source).expect("protected root");
        let target = FileSystem
            .plan_stage(&parent, "prepared", std::slice::from_ref(&source))
            .expect("initial native roots");
        let moved = fixture.path().join("old");
        fs::rename(fixture.path().join(changed), &moved).expect("replace selected native root");
        fs::create_dir(fixture.path().join(changed)).expect("new unrelated root identity");
        let capture = input();
        assert!(matches!(
            target.create(capture.as_input(), Limits::default()),
            Err(Error::Preflight(_))
        ));
        assert_eq!(entries(&parent), 0);
        assert_eq!(entries(&source), 0);
        assert_eq!(
            entries(&moved),
            0,
            "held descriptor was not used to write into displaced root"
        );
    }
}

#[test]
fn existing_or_intervening_collisions_are_not_adopted_or_given_a_fallback() {
    for kind in ["file", "directory", "symlink"] {
        let parent = tempfile::tempdir().expect("stage parent");
        let external = tempfile::tempdir().expect("external source");
        let external_file = external.path().join("keep");
        fs::write(&external_file, b"untouched").expect("external bytes");
        let target = FileSystem
            .plan_stage(parent.path(), "prepared", &[])
            .expect("initially absent target");
        let path = target.address().path();
        match kind {
            "file" => fs::write(&path, b"untouched").expect("intervening file"),
            "directory" => fs::create_dir(&path).expect("intervening directory"),
            "symlink" => symlink(&external_file, &path).expect("intervening symlink"),
            _ => unreachable!(),
        }
        let before = fs::symlink_metadata(&path).expect("foreign identity");
        let existing = FileSystem
            .plan_stage(parent.path(), "prepared", &[])
            .expect("planning an existing locator does not adopt or mutate it");
        assert_eq!(existing.address().path(), path);
        drop(existing);
        let capture = input();
        assert!(matches!(
            target.create(capture.as_input(), Limits::default()),
            Err(Error::Preflight(_))
        ));
        let after = fs::symlink_metadata(&path).expect("foreign object remains");
        assert_eq!(
            (before.dev(), before.ino(), before.mode()),
            (after.dev(), after.ino(), after.mode())
        );
        assert_eq!(entries(parent.path()), 1, "no random fallback");
        assert_eq!(
            fs::read(external_file).expect("external content remains"),
            b"untouched"
        );
    }
}

#[test]
fn planned_creation_preserves_limits_cleanup_and_failed_retention_residue() {
    let parent = tempfile::tempdir().expect("stage parent");
    let capture = input();
    let target = FileSystem
        .plan_stage(parent.path(), "prepared", &[])
        .expect("bounded target");
    assert!(matches!(
        target.create(
            capture.as_input(),
            Limits {
                total_bytes: 2,
                ..Default::default()
            }
        ),
        Err(Error::Preflight(_))
    ));
    assert_eq!(entries(parent.path()), 0);
    let target = FileSystem
        .plan_stage(parent.path(), "prepared", &[])
        .expect("cleanup target");
    let stage = target
        .create(capture.as_input(), Limits::default())
        .expect("complete stage");
    stage.close().expect("existing identity checked cleanup");
    assert_eq!(entries(parent.path()), 0);
    let target = FileSystem
        .plan_stage(parent.path(), "prepared", &[])
        .expect("retention target");
    let stage = target
        .create(capture.as_input(), Limits::default())
        .expect("complete stage");
    let path = stage.path().to_owned();
    fs::write(path.join("entry"), b"changed").expect("native interference fixture");
    assert!(matches!(
        StageRetention::retain(stage),
        Err(Error::Residue { .. })
    ));
    assert_eq!(
        fs::read(path.join("entry")).expect("failed retention preserves residue"),
        b"changed"
    );
}
