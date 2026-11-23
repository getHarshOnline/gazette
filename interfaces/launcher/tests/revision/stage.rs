//! Real SDK staging from authored bytes and exact native Git fixtures.

#[path = "stage_named.rs"]
mod named;

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use gazette_sdk::source::{self, Limits, PlacementRequest, SourceSelection, StageError};
use tempfile::tempdir;

use super::{Fixture, ParentFixture, limits, request};

fn children(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root)
        .expect("children")
        .map(|entry| entry.expect("entry").path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn staging_check_distinguishes_overlapping_inputs_from_the_actual_output() {
    let outer = tempdir().expect("source parent");
    let root = tempfile::tempdir_in(outer.path()).expect("repository");
    let commit = super::super::seed_repo(root.path());
    let repo = Fixture { root, commit };
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    let context = super::context(&prepared);
    let aliases = tempdir().expect("aliases");
    symlink(repo.root.path(), aliases.path().join("alias")).expect("ancestor alias");
    let staging = tempdir().expect("separate stage parent");
    for authored in [
        repo.root.path(),
        &repo.root.path().join("docs"),
        outer.path(),
        &aliases.path().join("alias/docs"),
    ] {
        let before = source::inspect(authored, None, Limits::default()).expect("fixture before");
        assert!(matches!(
            source::check_destination(&prepared, &context, authored, Limits::default()),
            Err(source::DestinationError::SourceOverlap { .. })
        ));
        let checked = source::check_stage(
            &prepared,
            &context,
            authored,
            staging.path(),
            Limits::default(),
        )
        .expect("overlapping inputs are not overlapping output");
        assert!(
            children(staging.path()).is_empty(),
            "check performs no writes"
        );
        let tree = source::stage(
            &prepared,
            &context,
            authored,
            source::StageLocation::Fresh(staging.path()),
            Limits::default(),
        )
        .expect("stage overlapping inputs to a separate output");
        assert_eq!(tree.inventory(), &checked);
        tree.close().expect("cleanup");
        assert!(children(staging.path()).is_empty());
        assert_eq!(
            source::inspect(authored, None, Limits::default()).expect("fixture after"),
            before
        );
    }
}

#[test]
fn authored_input_may_share_a_gitlink_parent_but_output_may_not() {
    let parent = ParentFixture::new();
    let authored = parent.root.path().join("unselected-sibling");
    fs::create_dir(&authored).expect("sibling");
    fs::write(authored.join("page.md"), b"authored").expect("page");
    let prepared = source::prepare(
        vec![PlacementRequest {
            target: "project".parse().expect("target"),
            source: SourceSelection::Submodule {
                parent: parent.root.path().to_owned(),
                parent_commit: parent.parent.to_string().parse().expect("parent pin"),
                submodule_path: "packages/tool".parse().expect("gitlink"),
                subdirectory: "docs".parse().expect("docs"),
            },
        }],
        limits(),
    )
    .expect("prepare child");
    let context = super::context(&prepared);
    let staging = tempdir().expect("stage parent");
    assert!(matches!(
        source::check_destination(&prepared, &context, &authored, Limits::default()),
        Err(source::DestinationError::SourceOverlap { .. })
    ));
    let checked = source::check_stage(
        &prepared,
        &context,
        &authored,
        staging.path(),
        Limits::default(),
    )
    .expect("shared-parent inputs");
    assert!(children(staging.path()).is_empty());
    let tree = source::stage(
        &prepared,
        &context,
        &authored,
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("stage shared-parent inputs");
    assert_eq!(tree.inventory(), &checked);
    tree.close().expect("cleanup");
    assert!(children(staging.path()).is_empty());
    assert_eq!(
        fs::read(authored.join("page.md")).expect("preserved"),
        b"authored"
    );
    for output in [
        parent.root.path(),
        &parent.root.path().join("packages"),
        &authored,
    ] {
        let before = source::inspect(output, None, Limits::default()).expect("before");
        for error in [
            source::check_stage(&prepared, &context, &authored, output, Limits::default())
                .expect_err("check rejects output inside gitlink parent"),
            source::stage(
                &prepared,
                &context,
                &authored,
                source::StageLocation::Fresh(output),
                Limits::default(),
            )
            .expect_err("stage rejects output inside gitlink parent"),
        ] {
            assert!(matches!(
                error,
                StageError::Preflight(source::DestinationError::SourceOverlap { .. })
            ));
        }
        assert_eq!(
            source::inspect(output, None, Limits::default()).expect("after"),
            before
        );
    }
}

#[test]
fn complete_mixed_stage_matches_check_and_readback_without_source_writes() {
    let repo = Fixture::new();
    let parent = ParentFixture::new();
    let authored = tempdir().expect("authored");
    let staging = tempdir().expect("staging");
    fs::write(authored.path().join("index.md"), b"authored").expect("base");
    fs::create_dir(authored.path().join("empty")).expect("empty directory");
    fs::write(staging.path().join("unrelated"), b"preserve").expect("unrelated");
    let prepared = source::prepare(
        vec![
            request("projects/repo", &repo),
            PlacementRequest {
                target: "projects/child".parse().expect("target"),
                source: SourceSelection::Submodule {
                    parent: parent.root.path().to_owned(),
                    parent_commit: parent.parent.to_string().parse().expect("parent pin"),
                    submodule_path: "packages/tool".parse().expect("gitlink"),
                    subdirectory: "docs".parse().expect("docs"),
                },
            },
        ],
        limits(),
    )
    .expect("complete preparation");
    let snapshots: Vec<_> = [authored.path(), repo.root.path(), parent.root.path()]
        .iter()
        .map(|path| source::inspect(path, None, Limits::default()).expect("before"))
        .collect();
    let checked = source::check_stage(
        &prepared,
        &super::context(&prepared),
        authored.path(),
        staging.path(),
        Limits::default(),
    )
    .expect("check");
    let first = source::stage(
        &prepared,
        &super::context(&prepared),
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("first stage");
    let second = source::stage(
        &prepared,
        &super::context(&prepared),
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("second stage");
    assert_ne!(first.path(), second.path(), "never reuse an existing stage");
    for placement in prepared.placements() {
        let target = &placement.request().target;
        let binding = super::context(&prepared).binding(target).expect("binding");
        let pin = super::materialization::manifest_pin(&first, target.as_str());
        assert_eq!(
            pin,
            super::materialization::manifest_pin(&second, target.as_str())
        );
        let verified = source::verify_generated_tree(
            &first.path().join(target.as_str()),
            &binding,
            pin,
            Limits::default(),
        )
        .expect("pinned generated tree");
        assert_eq!(
            verified.source().inventory(),
            placement.source().inventory()
        );
        if target.as_str() == "projects/child" {
            assert_eq!(
                verified.manifest().source(),
                &source::GeneratedSource::Submodule {
                    parent_commit: parent.parent.to_string(),
                    submodule_path: "packages/tool".into(),
                    commit: parent.child.to_string(),
                    docs_path: "docs".into(),
                }
            );
        }
    }
    assert_eq!(first.inventory(), &checked);
    assert_eq!(
        first.inventory(),
        second.inventory(),
        "random paths do not enter identity"
    );
    assert_eq!(
        source::inspect(first.path(), None, Limits::default()).expect("independent readback"),
        *first.inventory()
    );
    for (path, bytes) in [
        ("index.md", b"authored".as_slice()),
        ("projects/repo/readme.md", b"abc"),
        ("projects/child/readme.md", b"abc"),
    ] {
        assert_eq!(
            fs::read(first.path().join(path)).expect("staged bytes"),
            bytes
        );
        assert_eq!(
            fs::metadata(first.path().join(path))
                .expect("file mode")
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    assert!(first.path().join("empty").is_dir());
    assert_eq!(
        fs::metadata(first.path())
            .expect("root mode")
            .permissions()
            .mode()
            & 0o077,
        0
    );
    for (path, before) in [authored.path(), repo.root.path(), parent.root.path()]
        .iter()
        .zip(snapshots)
    {
        assert_eq!(
            source::inspect(path, None, Limits::default()).expect("after"),
            before
        );
    }
    let first_path = first.path().to_owned();
    let second_path = second.path().to_owned();
    first.close().expect("observable cleanup");
    assert!(!first_path.exists());
    assert!(second_path.is_dir());
    drop(second);
    assert!(!second_path.exists());
    assert_eq!(
        children(staging.path()),
        vec![staging.path().join("unrelated")]
    );
    assert_eq!(
        fs::read(staging.path().join("unrelated")).expect("untouched"),
        b"preserve"
    );
}

#[test]
fn captured_projects_are_not_reopened_while_authored_base_is_fresh() {
    let repo = Fixture::new();
    let authored = tempdir().expect("authored");
    let staging = tempdir().expect("staging");
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    fs::write(
        repo.root.path().join("docs/readme.md"),
        b"changed live source",
    )
    .expect("later edit");
    fs::write(authored.path().join("latest.md"), b"fresh base").expect("base edit");
    let tree = source::stage(
        &prepared,
        &super::context(&prepared),
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("retained stage");
    assert_eq!(
        fs::read(tree.path().join("project/readme.md")).expect("retained"),
        b"abc"
    );
    assert_eq!(
        fs::read(tree.path().join("latest.md")).expect("fresh"),
        b"fresh base"
    );
    assert_eq!(
        fs::read(repo.root.path().join("docs/readme.md")).expect("live"),
        b"changed live source"
    );
    tree.close().expect("cleanup");
}

#[test]
fn staging_check_does_not_scan_or_adopt_unrelated_parent_children() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    let context = super::context(&prepared);
    let authored = tempdir().expect("authored");
    let staging = tempdir().expect("staging");
    fs::write(authored.path().join("page.md"), b"page").expect("page");
    // Such a link would be rejected if parent children were treated as inputs.
    let unrelated = staging.path().join("unrelated-link");
    symlink(repo.root.path(), &unrelated).expect("unrelated link");
    let before = children(staging.path());
    let checked = source::check_stage(
        &prepared,
        &context,
        authored.path(),
        staging.path(),
        Limits::default(),
    )
    .expect("check ignores unrelated children");
    assert_eq!(children(staging.path()), before);
    let tree = source::stage(
        &prepared,
        &context,
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("stage ignores unrelated children");
    assert_eq!(tree.inventory(), &checked);
    assert!(!tree.path().join("unrelated-link").exists());
    tree.close().expect("cleanup only own stage");
    assert_eq!(children(staging.path()), before);
    assert_eq!(
        fs::read_link(unrelated).expect("untouched link"),
        repo.root.path()
    );
}

#[test]
fn staging_rechecks_authored_bytes_and_layout_after_an_earlier_check() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    let context = super::context(&prepared);
    let authored = tempdir().expect("authored");
    let staging = tempdir().expect("staging");
    fs::write(authored.path().join("page.md"), b"before").expect("page");
    let earlier = source::check_stage(
        &prepared,
        &context,
        authored.path(),
        staging.path(),
        Limits::default(),
    )
    .expect("initial check");
    fs::write(authored.path().join("page.md"), b"after").expect("later authored edit");
    let tree = source::stage(
        &prepared,
        &context,
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("stage current bytes");
    assert_ne!(tree.inventory(), &earlier);
    assert_eq!(
        fs::read(tree.path().join("page.md")).expect("current output"),
        b"after"
    );
    tree.close().expect("cleanup");
    fs::create_dir(authored.path().join("project")).expect("later collision");
    assert!(matches!(
        source::stage(
            &prepared,
            &context,
            authored.path(),
            source::StageLocation::Fresh(staging.path()),
            Limits::default()
        ),
        Err(StageError::Preflight(
            source::DestinationError::Collision { .. }
        ))
    ));
    assert!(children(staging.path()).is_empty());
    assert!(authored.path().join("project").is_dir());
}

#[test]
fn staging_rechecks_the_actual_parent_after_an_earlier_check() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    let context = super::context(&prepared);
    let authored = tempdir().expect("authored");
    let outer = tempdir().expect("stage owner");
    let staging = outer.path().join("staging");
    let original = outer.path().join("original");
    fs::create_dir(&staging).expect("staging");
    source::check_stage(
        &prepared,
        &context,
        authored.path(),
        &staging,
        Limits::default(),
    )
    .expect("earlier check");
    fs::rename(&staging, &original).expect("move original");
    symlink(authored.path(), &staging).expect("unsafe replacement");
    assert!(matches!(
        source::stage(
            &prepared,
            &context,
            authored.path(),
            source::StageLocation::Fresh(&staging),
            Limits::default()
        ),
        Err(StageError::Preflight(_))
    ));
    assert!(children(&original).is_empty());
    assert!(children(authored.path()).is_empty());
    assert_eq!(
        fs::read_link(&staging).expect("replacement untouched"),
        authored.path()
    );
}

#[test]
fn preflight_collision_limits_and_missing_parent_create_nothing() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    for failure in ["target", "case alias", "aggregate limit", "missing parent"] {
        let authored = tempdir().expect("authored");
        let staging = tempdir().expect("staging");
        fs::write(staging.path().join("unrelated"), b"keep").expect("unrelated");
        let mut bound = Limits::default();
        match failure {
            "target" => fs::create_dir(authored.path().join("project")).expect("collision"),
            "case alias" => fs::create_dir(authored.path().join("PROJECT")).expect("alias"),
            "aggregate limit" => bound.total_bytes = 2,
            _ => (),
        }
        let before =
            source::inspect(authored.path(), None, Limits::default()).expect("authored before");
        let parent = if failure == "missing parent" {
            staging.path().join("absent")
        } else {
            staging.path().to_owned()
        };
        let checked_error = source::check_stage(
            &prepared,
            &super::context(&prepared),
            authored.path(),
            &parent,
            bound,
        )
        .expect_err(failure);
        let error = source::stage(
            &prepared,
            &super::context(&prepared),
            authored.path(),
            source::StageLocation::Fresh(&parent),
            bound,
        )
        .expect_err(failure);
        assert_eq!(error.to_string(), checked_error.to_string(), "{failure}");
        assert!(
            matches!(error, StageError::Preflight(_)),
            "{failure}: {error:?}"
        );
        assert_eq!(
            children(staging.path()),
            vec![staging.path().join("unrelated")]
        );
        assert_eq!(
            source::inspect(authored.path(), None, Limits::default()).expect("unchanged"),
            before
        );
    }
}

#[test]
fn staging_rejects_native_source_overlap_and_symlinks_without_writes() {
    let outer = tempdir().expect("fixture");
    let authored = outer.path().join("authored");
    let staging = outer.path().join("staging");
    fs::create_dir(&authored).expect("authored");
    fs::create_dir(&staging).expect("staging");
    fs::create_dir(authored.join("inside")).expect("inside");
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    for parent in [
        outer.path(),
        &authored,
        &authored.join("inside"),
        repo.root.path(),
        &repo.root.path().join("docs"),
    ] {
        let before = children(parent);
        assert!(matches!(
            source::check_stage(
                &prepared,
                &super::context(&prepared),
                &authored,
                parent,
                Limits::default()
            ),
            Err(StageError::Preflight(
                source::DestinationError::SourceOverlap { .. }
            ))
        ));
        let error = source::stage(
            &prepared,
            &super::context(&prepared),
            &authored,
            source::StageLocation::Fresh(parent),
            Limits::default(),
        )
        .expect_err("overlap");
        assert!(
            matches!(
                error,
                StageError::Preflight(source::DestinationError::SourceOverlap { .. })
            ),
            "{error:?}"
        );
        assert_eq!(children(parent), before);
    }
    symlink(&staging, outer.path().join("linked-stage")).expect("parent link");
    assert!(matches!(
        source::check_stage(
            &prepared,
            &super::context(&prepared),
            &authored,
            &outer.path().join("linked-stage"),
            Limits::default()
        ),
        Err(StageError::Preflight(_))
    ));
    assert!(matches!(
        source::stage(
            &prepared,
            &super::context(&prepared),
            &authored,
            source::StageLocation::Fresh(&outer.path().join("linked-stage")),
            Limits::default()
        ),
        Err(StageError::Preflight(_))
    ));
    symlink(
        repo.root.path().join("docs/readme.md"),
        authored.join("linked-source"),
    )
    .expect("authored link");
    assert!(matches!(
        source::check_stage(
            &prepared,
            &super::context(&prepared),
            &authored,
            &staging,
            Limits::default()
        ),
        Err(StageError::Preflight(_))
    ));
    assert!(matches!(
        source::stage(
            &prepared,
            &super::context(&prepared),
            &authored,
            source::StageLocation::Fresh(&staging),
            Limits::default()
        ),
        Err(StageError::Preflight(_))
    ));
    assert!(children(&staging).is_empty());
}

#[test]
fn changed_or_unknown_stage_content_is_preserved_and_reported_as_residue() {
    let repo = Fixture::new();
    let prepared = source::prepare(vec![request("project", &repo)], limits()).expect("prepare");
    for mutation in ["bytes", "extra", "replacement", "symlink"] {
        let authored = tempdir().expect("authored");
        let staging = tempdir().expect("staging");
        let tree = source::stage(
            &prepared,
            &super::context(&prepared),
            authored.path(),
            source::StageLocation::Fresh(staging.path()),
            Limits::default(),
        )
        .expect("stage");
        let path = tree.path().to_owned();
        let file = path.join("project/readme.md");
        match mutation {
            "bytes" => fs::write(&file, b"xyz").expect("changed"),
            "extra" => fs::write(path.join("unknown"), b"preserve").expect("extra"),
            "replacement" => {
                fs::rename(&file, staging.path().join("original-file"))
                    .expect("keep original inode");
                fs::write(&file, b"abc").expect("identical replacement bytes");
            }
            _ => {
                fs::rename(&file, staging.path().join("original-file")).expect("keep original");
                symlink(staging.path().join("original-file"), &file).expect("replacement link");
            }
        }
        let error = tree
            .close()
            .expect_err("must preserve changed/unknown tree");
        assert!(
            matches!(error, StageError::Residue { path: ref residue, .. } if residue == &path),
            "{error:?}"
        );
        assert!(path.is_dir());
        assert!(
            fs::symlink_metadata(&file).is_ok(),
            "cleanup must not start on preflight failure"
        );
        let next = source::stage(
            &prepared,
            &super::context(&prepared),
            authored.path(),
            source::StageLocation::Fresh(staging.path()),
            Limits::default(),
        )
        .expect("new independent stage");
        assert_ne!(next.path(), path);
        next.close().expect("new stage cleanup");
        assert!(path.is_dir(), "a later run never adopts or deletes residue");
    }
}

#[test]
fn stage_root_replacement_is_not_cleanup_authority() {
    let prepared = source::prepare(Vec::new(), limits()).expect("empty set");
    let authored = tempdir().expect("authored");
    let staging = tempdir().expect("staging");
    fs::write(authored.path().join("base"), b"base").expect("base");
    let tree = source::stage(
        &prepared,
        &super::context(&prepared),
        authored.path(),
        source::StageLocation::Fresh(staging.path()),
        Limits::default(),
    )
    .expect("stage");
    let path = tree.path().to_owned();
    let moved = staging.path().join("moved");
    fs::rename(&path, &moved).expect("move original");
    fs::create_dir(&path).expect("replacement");
    fs::write(path.join("unknown"), b"keep").expect("foreign content");
    assert!(matches!(tree.close(), Err(StageError::Residue { .. })));
    assert_eq!(fs::read(path.join("unknown")).expect("preserved"), b"keep");
    assert_eq!(
        fs::read(moved.join("base")).expect("original preserved"),
        b"base"
    );
}
