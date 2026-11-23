//! Native stage failure boundaries without widening the production API for tests.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};

use gazette_core::source::{
    Content, EntryPath, Inventory, InventoryBuilder, Limits, Sha256Digest, TreeInput,
};
use tempfile::tempdir;

use super::{DestinationRoots, Error, FileSystem, Location, OwnedTree, SourceError, StageWriter};

#[test]
fn bounded_named_stage_checks_exact_native_bytes_before_creation() {
    let parent = tempdir().expect("private fixture");
    let inventory = InventoryBuilder::new(Limits::default()).finish();
    let name = "é-日本-stage";
    let expected = parent
        .path()
        .canonicalize()
        .expect("canonical parent")
        .join(name);
    let exact = expected.as_os_str().as_encoded_bytes().len();
    assert!(exact > expected.to_str().expect("Unicode fixture").chars().count());
    for bound in [0, exact - 1, (1 << 20) + 1] {
        let error = FileSystem
            .stage(
                Location::NamedBounded {
                    parent: parent.path(),
                    name,
                    directory_bytes: bound,
                },
                &[],
                TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
                Limits::default(),
            )
            .expect_err("invalid or exhausted native bound refuses before mkdir");
        assert!(matches!(error, Error::Preflight(_)), "{error:?}");
        assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 0);
        assert!(!expected.exists());
    }
    let stage = FileSystem
        .stage(
            Location::NamedBounded {
                parent: parent.path(),
                name,
                directory_bytes: exact,
            },
            &[],
            TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
            Limits::default(),
        )
        .expect("exact native byte boundary");
    assert_eq!(stage.path(), expected);
    assert_eq!(stage.path().as_os_str().as_encoded_bytes().len(), exact);
    assert_eq!(stage.inventory(), &inventory);
    stage.close().expect("explicit owned cleanup");
    assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 0);
}

#[test]
fn bounded_named_collision_preserves_existing_tree_without_fallback() {
    let parent = tempdir().expect("private fixture");
    let destination = parent.path().join("selected");
    fs::create_dir(&destination).expect("existing unowned tree");
    fs::write(destination.join("owner.txt"), b"must remain").expect("existing bytes");
    let identity = fs::metadata(&destination).expect("existing identity").ino();
    let inventory = InventoryBuilder::new(Limits::default()).finish();
    for location in [
        Location::Named {
            parent: parent.path(),
            name: "selected",
        },
        Location::NamedBounded {
            parent: parent.path(),
            name: "selected",
            directory_bytes: 1 << 20,
        },
    ] {
        let error = FileSystem
            .stage(
                location,
                &[],
                TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
                Limits::default(),
            )
            .expect_err("named collision never falls back");
        assert!(matches!(error, Error::Preflight(_)), "{error:?}");
        assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 1);
        assert_eq!(
            fs::metadata(&destination).expect("same identity").ino(),
            identity
        );
        assert_eq!(
            fs::read(destination.join("owner.txt")).expect("preserved bytes"),
            b"must remain"
        );
    }
    for name in ["", ".", "..", "nested/child", "../escape"] {
        assert!(
            FileSystem
                .stage(
                    Location::NamedBounded {
                        parent: parent.path(),
                        name,
                        directory_bytes: 1 << 20
                    },
                    &[],
                    TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
                    Limits::default(),
                )
                .is_err()
        );
        assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 1);
    }
}

#[test]
fn bounded_named_stage_measures_canonical_parent_not_short_ancestor_alias() {
    let owner = tempdir().expect("private fixture");
    let real = owner
        .path()
        .join("a-much-longer-canonical-parent-than-the-supplied-alias");
    fs::create_dir(&real).expect("canonical ancestor");
    fs::create_dir(real.join("leaf")).expect("real terminal directory");
    let alias = owner.path().join("a");
    symlink(&real, &alias).expect("fixture ancestor alias");
    let selected = alias.join("leaf");
    // Only an ancestor is aliased; terminal symlinks retain their existing refusal.
    let expected = selected
        .canonicalize()
        .expect("canonical selected parent")
        .join("named");
    let short = selected.join("named").as_os_str().as_encoded_bytes().len();
    let exact = expected.as_os_str().as_encoded_bytes().len();
    assert!(
        exact > short,
        "fixture must distinguish native from caller spelling"
    );
    let inventory = InventoryBuilder::new(Limits::default()).finish();
    for bound in [short, exact - 1] {
        let error = FileSystem
            .stage(
                Location::NamedBounded {
                    parent: &selected,
                    name: "named",
                    directory_bytes: bound,
                },
                &[],
                TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
                Limits::default(),
            )
            .expect_err("canonical expansion refuses before mkdir");
        assert!(matches!(error, Error::Preflight(_)), "{error:?}");
        assert!(!expected.exists());
        assert_eq!(
            fs::read_dir(&selected).expect("unchanged parent").count(),
            0
        );
    }
    let stage = FileSystem
        .stage(
            Location::NamedBounded {
                parent: &selected,
                name: "named",
                directory_bytes: exact,
            },
            &[],
            TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
            Limits::default(),
        )
        .expect("actual canonical boundary");
    assert_eq!(stage.path(), expected);
    stage.close().expect("explicit owned cleanup");
}

#[test]
fn bounded_fresh_stage_checks_canonical_locator_before_creation() {
    let owner = tempdir().expect("private fixture");
    let real = owner.path().join("a-longer-canonical-parent");
    fs::create_dir(&real).expect("canonical parent");
    // Native staging rejects a terminal symlink. The host's temporary-root
    // spelling may still canonicalize through ancestors (for example /var).
    let alias = real.clone();
    let inventory = InventoryBuilder::new(Limits::default()).finish();
    let stage = FileSystem
        .stage(
            Location::Fresh(&alias),
            &[],
            TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
            Limits::default(),
        )
        .expect("native stage");
    let exact = stage.path().as_os_str().as_encoded_bytes().len();
    stage.close().expect("explicit owned cleanup");
    for bound in [0, exact - 1, (1 << 20) + 1] {
        let error = FileSystem
            .stage(
                Location::FreshBounded {
                    parent: &alias,
                    directory_bytes: bound,
                },
                &[],
                TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
                Limits::default(),
            )
            .expect_err("bounded refusal");
        assert!(matches!(error, Error::Preflight(_)), "{error:?}");
        assert_eq!(fs::read_dir(&real).expect("unchanged parent").count(), 0);
    }
    let stage = FileSystem
        .stage(
            Location::FreshBounded {
                parent: &alias,
                directory_bytes: exact,
            },
            &[],
            TreeInput::new(&inventory, BTreeMap::new()).expect("empty input"),
            Limits::default(),
        )
        .expect("exact bound");
    assert_eq!(stage.path().as_os_str().as_encoded_bytes().len(), exact);
    stage.close().expect("explicit owned cleanup");
}

#[test]
fn partial_write_failure_preserves_an_unowned_conflicting_entry() {
    let parent = tempdir().expect("fixture");
    let mut tree = OwnedTree::create(
        DestinationRoots::observe(parent.path(), &[]).expect("roots"),
        Limits::default(),
        None,
        None,
    )
    .expect("private tree");
    let mut builder = InventoryBuilder::new(Limits::default());
    let files: BTreeMap<EntryPath, &[u8]> = ["a", "b"]
        .into_iter()
        .map(|path| {
            let path: EntryPath = path.parse().expect("path");
            builder
                .insert(
                    path.clone(),
                    Content::File {
                        bytes: 3,
                        sha256: Sha256Digest::of(b"abc"),
                    },
                )
                .expect("file");
            (path, b"abc".as_slice())
        })
        .collect();
    let inventory = builder.finish();
    let input = TreeInput::new(&inventory, files).expect("checked input");
    fs::write(tree.path.join("b"), b"foreign").expect("interference after preflight");
    assert!(matches!(tree.write(&input), Err(SourceError::Io { .. })));
    assert_eq!(tree.nodes.len(), 1, "the first write really happened");
    assert_eq!(fs::read(tree.path.join("a")).expect("owned bytes"), b"abc");
    assert!(tree.cleanup(None).is_err(), "unknown entry blocks deletion");
    assert_eq!(
        fs::read(tree.path.join("b")).expect("foreign preserved"),
        b"foreign"
    );
    // Only the fixture owner can remove its injected obstruction. Once removed,
    // the writer can reconcile and remove exactly its own partial tree.
    fs::remove_file(tree.path.join("b")).expect("fixture removes its own obstruction");
    tree.cleanup(None).expect("owned partial tree cleanup");
    assert!(!tree.path.exists());
}

#[test]
fn writer_root_replacement_fails_before_the_first_entry_write() {
    let parent = tempdir().expect("fixture");
    let mut tree = OwnedTree::create(
        DestinationRoots::observe(parent.path(), &[]).expect("roots"),
        Limits::default(),
        None,
        None,
    )
    .expect("private tree");
    let mut builder = InventoryBuilder::new(Limits::default());
    let path: EntryPath = "entry".parse().expect("path");
    builder
        .insert(
            path.clone(),
            Content::File {
                bytes: 3,
                sha256: Sha256Digest::of(b"abc"),
            },
        )
        .expect("entry");
    let inventory = builder.finish();
    let input =
        TreeInput::new(&inventory, BTreeMap::from([(path, b"abc".as_slice())])).expect("input");
    fs::rename(&tree.path, parent.path().join("original")).expect("move original");
    fs::create_dir(&tree.path).expect("replacement");
    assert!(matches!(tree.write(&input), Err(SourceError::Changed(_))));
    assert!(tree.nodes.is_empty());
    assert!(tree.cleanup(None).is_err());
    assert!(
        fs::read_dir(&tree.path)
            .expect("replacement untouched")
            .next()
            .is_none()
    );
    assert!(
        fs::read_dir(parent.path().join("original"))
            .expect("original untouched")
            .next()
            .is_none()
    );
}

#[test]
fn readback_failure_and_cleanup_outcomes_retain_the_original_cause() {
    for extra in [false, true] {
        let parent = tempdir().expect("fixture");
        let mut tree = OwnedTree::create(
            DestinationRoots::observe(parent.path(), &[]).expect("roots"),
            Limits::default(),
            None,
            None,
        )
        .expect("private tree");
        let mut builder = InventoryBuilder::new(Limits::default());
        let path: EntryPath = "entry".parse().expect("path");
        builder
            .insert(
                path.clone(),
                Content::File {
                    bytes: 3,
                    sha256: Sha256Digest::of(b"abc"),
                },
            )
            .expect("entry");
        let inventory = builder.finish();
        let input =
            TreeInput::new(&inventory, BTreeMap::from([(path, b"abc".as_slice())])).expect("input");
        tree.write(&input).expect("real native writes");
        assert_eq!(
            tree.verify(&inventory).expect("initial readback"),
            inventory
        );
        fs::write(tree.path.join("entry"), b"xyz").expect("same-size changed readback");
        let cause = tree
            .verify(&inventory)
            .expect_err("readback must compare actual bytes");
        assert!(matches!(cause, SourceError::ContentMismatch(_)));
        if extra {
            fs::write(tree.path.join("unknown"), b"keep").expect("unknown data");
        }
        let path = tree.path.clone();
        let error = tree.failure(cause);
        if extra {
            assert!(matches!(
                error,
                Error::Residue {
                    cause: SourceError::ContentMismatch(_),
                    cleanup: Some(_),
                    ..
                }
            ));
            assert_eq!(
                fs::read(path.join("unknown")).expect("preserved unknown"),
                b"keep"
            );
            assert_eq!(
                fs::read(path.join("entry")).expect("preserved partial tree"),
                b"xyz"
            );
        } else {
            assert!(matches!(
                error,
                Error::Operation(SourceError::ContentMismatch(_))
            ));
            assert!(
                !path.exists(),
                "same owned entries reconciled on failed operation"
            );
        }
    }
}

#[test]
fn adapter_checks_its_own_limits_before_any_effect_and_accepts_an_empty_tree() {
    let parent = tempdir().expect("fixture");
    let mut builder = InventoryBuilder::new(Limits::default());
    let path: EntryPath = "entry".parse().expect("path");
    builder
        .insert(
            path.clone(),
            Content::File {
                bytes: 3,
                sha256: Sha256Digest::of(b"abc"),
            },
        )
        .expect("entry");
    let inventory = builder.finish();
    let input =
        TreeInput::new(&inventory, BTreeMap::from([(path, b"abc".as_slice())])).expect("input");
    let error = FileSystem
        .stage(
            Location::Fresh(parent.path()),
            &[],
            input,
            Limits {
                total_bytes: 2,
                ..Limits::default()
            },
        )
        .expect_err("adapter bounds");
    assert!(matches!(error, Error::Preflight(_)));
    assert!(
        fs::read_dir(parent.path())
            .expect("unchanged parent")
            .next()
            .is_none()
    );
    let empty = InventoryBuilder::new(Limits::default()).finish();
    let input = TreeInput::new(&empty, BTreeMap::new()).expect("empty input");
    let tree = FileSystem
        .stage(
            Location::Fresh(parent.path()),
            &[],
            input,
            Limits {
                entries: 0,
                total_bytes: 0,
                ..Limits::default()
            },
        )
        .expect("empty tree");
    assert_eq!(tree.inventory(), &empty);
    tree.close().expect("remove only created empty stage");
    assert!(
        fs::read_dir(parent.path())
            .expect("clean parent")
            .next()
            .is_none()
    );
}

fn named_input() -> (Inventory, BTreeMap<EntryPath, &'static [u8]>) {
    let mut builder = InventoryBuilder::new(Limits::default());
    let path: EntryPath = "entry".parse().expect("path");
    builder
        .insert(
            path.clone(),
            Content::File {
                bytes: 3,
                sha256: Sha256Digest::of(b"abc"),
            },
        )
        .expect("entry");
    (
        builder.finish(),
        BTreeMap::from([(path, b"abc".as_slice())]),
    )
}

#[test]
fn named_stage_uses_the_same_readback_and_explicit_retention() {
    let parent = tempdir().expect("fixture");
    let (inventory, files) = named_input();
    let tree = FileSystem
        .stage(
            Location::Named {
                parent: parent.path(),
                name: ".prepared-001",
            },
            &[],
            TreeInput::new(&inventory, files.clone()).expect("input"),
            Limits::default(),
        )
        .expect("exact named stage");
    let path = parent
        .path()
        .canonicalize()
        .expect("parent")
        .join(".prepared-001");
    assert_eq!(tree.path(), path);
    assert_eq!(tree.inventory(), &inventory);
    assert_eq!(fs::read(path.join("entry")).expect("actual bytes"), b"abc");
    assert_eq!(
        fs::metadata(&path).expect("root mode").mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(path.join("entry")).expect("file mode").mode() & 0o777,
        0o600
    );
    let fresh = FileSystem
        .stage(
            Location::Fresh(parent.path()),
            &[],
            TreeInput::new(&inventory, files).expect("same input"),
            Limits::default(),
        )
        .expect("fresh stage");
    assert_eq!(
        fresh.inventory(),
        tree.inventory(),
        "locator is not byte identity"
    );
    fresh.close().expect("close random stage");
    let retained = tree.retain().expect("synchronize existing owned tree");
    assert_eq!(retained.path(), path);
    assert_eq!(retained.inventory(), &inventory);
    drop(retained);
    assert_eq!(
        fs::read(path.join("entry")).expect("retention survives drop"),
        b"abc"
    );
    assert_eq!(fs::read_dir(parent.path()).expect("parent").count(), 1);
}

#[test]
fn named_collision_never_adopts_replaces_deletes_or_chooses_another_child() {
    for kind in ["file", "directory", "symlink"] {
        let parent = tempdir().expect("parent");
        let external = tempdir().expect("external fixture");
        let foreign = external.path().join("foreign");
        fs::write(&foreign, b"untouched").expect("external file");
        let path = parent.path().join("prepared");
        match kind {
            "file" => fs::write(&path, b"untouched").expect("existing file"),
            "directory" => {
                fs::create_dir(&path).expect("existing directory");
                fs::write(path.join("keep"), b"untouched").expect("existing content");
            }
            "symlink" => symlink(&foreign, &path).expect("existing symlink"),
            _ => unreachable!(),
        }
        let before = fs::symlink_metadata(&path).expect("existing identity");
        let (inventory, files) = named_input();
        let error = FileSystem
            .stage(
                Location::Named {
                    parent: parent.path(),
                    name: "prepared",
                },
                &[],
                TreeInput::new(&inventory, files).expect("input"),
                Limits::default(),
            )
            .expect_err("existing target refuses creation");
        assert!(matches!(
            error,
            Error::Preflight(gazette_core::destination::Error::Observation(
                SourceError::Io { .. }
            ))
        ));
        assert!(!error.to_string().contains("no stage remains"));
        let after = fs::symlink_metadata(&path).expect("foreign target remains");
        assert_eq!(
            (before.dev(), before.ino(), before.mode()),
            (after.dev(), after.ino(), after.mode())
        );
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("no alternate stage")
                .count(),
            1
        );
        assert_eq!(
            fs::read(&foreign).expect("external target untouched"),
            b"untouched"
        );
        if kind == "directory" {
            assert_eq!(
                fs::read(path.join("keep")).expect("old directory content"),
                b"untouched"
            );
        } else {
            assert_eq!(fs::read(&path).expect("old bytes"), b"untouched");
        }
    }
}

#[test]
fn invalid_named_child_is_rejected_before_root_access_or_creation() {
    let parent = tempdir().expect("parent");
    let missing = parent.path().join("missing-parent");
    for root in [parent.path(), missing.as_path()] {
        for name in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "/absolute",
            "C:prefix",
            "./a",
            "../a",
            "a\n",
            "a\t",
            "a\0",
            "a\u{85}",
            "%2e%2e",
            "%2f",
            "%252f",
        ] {
            let (inventory, files) = named_input();
            let error = FileSystem
                .stage(
                    Location::Named { parent: root, name },
                    &[],
                    TreeInput::new(&inventory, files).expect("input"),
                    Limits::default(),
                )
                .expect_err("invalid literal child");
            assert!(
                matches!(
                    error,
                    Error::Preflight(gazette_core::destination::Error::Observation(
                        SourceError::InvalidName(_)
                    ))
                ),
                "name {name:?}: {error:?}"
            );
            assert_eq!(
                fs::read_dir(parent.path())
                    .expect("unchanged parent")
                    .count(),
                0
            );
        }
    }
}

#[test]
fn named_stage_preserves_limits_separation_and_complete_tree_guard() {
    let parent = tempdir().expect("parent");
    let (inventory, files) = named_input();
    let location = Location::Named {
        parent: parent.path(),
        name: "prepared",
    };
    let limited = FileSystem.stage(
        location,
        &[],
        TreeInput::new(&inventory, files.clone()).expect("input"),
        Limits {
            total_bytes: 2,
            ..Limits::default()
        },
    );
    assert!(matches!(limited, Err(Error::Preflight(_))));
    let overlapping = FileSystem.stage(
        location,
        &[parent.path().to_owned()],
        TreeInput::new(&inventory, files.clone()).expect("input"),
        Limits::default(),
    );
    assert!(matches!(overlapping, Err(Error::Preflight(_))));
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("unchanged parent")
            .count(),
        0
    );
    let tree = FileSystem
        .stage(
            location,
            &[],
            TreeInput::new(&inventory, files).expect("input"),
            Limits::default(),
        )
        .expect("named stage");
    let path = tree.path().to_owned();
    fs::write(path.join(".git"), b"foreign metadata").expect("fixture interference");
    assert!(
        matches!(tree.close(), Err(Error::Residue { .. })),
        "named staging does not ignore Git metadata"
    );
    assert_eq!(
        fs::read(path.join("entry")).expect("owned bytes preserved"),
        b"abc"
    );
    assert_eq!(
        fs::read(path.join(".git")).expect("foreign entry preserved"),
        b"foreign metadata"
    );
}
