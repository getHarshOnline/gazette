//! Physical destination checks through real SDK operations and native fixtures.

use std::fs;
use std::os::unix::fs::symlink;

use gazette_sdk::source::{self, DestinationError, Limits};
use tempfile::{tempdir, tempdir_in};

use super::{Fixture, limits, request};

#[test]
fn disjoint_destination_returns_complete_proposed_inventory_without_writes() {
    let fixture = Fixture::new();
    let content = tempdir().expect("content");
    fs::create_dir(content.path().join("docs")).expect("shared ancestor");
    fs::write(content.path().join("docs/intro.md"), b"intro").expect("existing page");
    let prepared = source::prepare(
        vec![
            request("docs/api", &fixture),
            request("docs/reference", &fixture),
        ],
        limits(),
    )
    .expect("sources");
    let before = source::inspect(content.path(), None, Limits::default()).expect("before");
    let checked = source::check_destination(
        &prepared,
        &super::context(&prepared),
        content.path(),
        Limits::default(),
    )
    .expect("check");
    let paths: Vec<_> = checked
        .inventory()
        .entries()
        .iter()
        .map(|entry| entry.path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "docs",
            "docs/api",
            "docs/api/.gazette-source.json",
            "docs/api/readme.md",
            "docs/intro.md",
            "docs/reference",
            "docs/reference/.gazette-source.json",
            "docs/reference/readme.md"
        ]
    );
    let metadata_bytes: u64 = checked
        .inventory()
        .entries()
        .iter()
        .filter_map(|entry| match entry.content {
            source::Content::File { bytes, .. }
                if entry
                    .path
                    .as_str()
                    .ends_with(source::GENERATED_TREE_MANIFEST) =>
            {
                Some(bytes)
            }
            _ => None,
        })
        .sum();
    assert!(metadata_bytes > 0);
    assert_eq!(checked.inventory().total_bytes(), 11 + metadata_bytes);
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            content.path(),
            Limits {
                total_bytes: 10,
                ..Limits::default()
            }
        ),
        Err(DestinationError::Observation(source::Error::Limit { .. }))
    ));
    assert_eq!(
        checked.inventory(),
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            content.path(),
            Limits::default()
        )
        .expect("repeat observation")
        .inventory()
    );
    assert_eq!(
        before,
        source::inspect(content.path(), None, Limits::default()).expect("after")
    );
    assert!(!content.path().join("docs/api").exists());
}

#[test]
fn equal_descendant_and_ancestor_destinations_cannot_overlap_source_roots() {
    let outer = tempdir().expect("outer");
    let root = tempdir_in(outer.path()).expect("source");
    let commit = super::super::seed_repo(root.path());
    let fixture = Fixture { root, commit };
    let prepared = source::prepare(vec![request("project", &fixture)], limits()).expect("sources");
    for destination in [
        fixture.root.path(),
        &fixture.root.path().join("docs"),
        outer.path(),
    ] {
        assert!(
            matches!(
                source::check_destination(
                    &prepared,
                    &super::context(&prepared),
                    destination,
                    Limits::default()
                ),
                Err(DestinationError::SourceOverlap { .. })
            ),
            "{destination:?}"
        );
    }
}

#[test]
fn aliased_ancestor_spelling_cannot_hide_a_source_overlap() {
    let fixture = Fixture::new();
    let aliases = tempdir().expect("aliases");
    symlink(fixture.root.path(), aliases.path().join("alias")).expect("alias");
    let prepared = source::prepare(vec![request("project", &fixture)], limits()).expect("sources");
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            &aliases.path().join("alias/docs"),
            Limits::default()
        ),
        Err(DestinationError::SourceOverlap { .. })
    ));
}

#[test]
fn parent_gitlink_selection_protects_the_whole_parent_not_only_child_docs() {
    let fixture = super::ParentFixture::new();
    let prepared = source::prepare(
        vec![source::PlacementRequest {
            target: "project".parse().expect("target"),
            source: source::SourceSelection::Submodule {
                parent: fixture.root.path().to_owned(),
                parent_commit: fixture.parent.to_string().parse().expect("parent pin"),
                submodule_path: "packages/tool".parse().expect("gitlink"),
                subdirectory: "docs".parse().expect("docs"),
            },
        }],
        limits(),
    )
    .expect("child source");
    let destination = fixture.root.path().join("unselected-sibling");
    fs::create_dir(&destination).expect("disposable sibling");
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            &destination,
            Limits::default()
        ),
        Err(DestinationError::SourceOverlap { .. })
    ));
}

#[test]
fn filesystem_selection_protects_its_explicit_root_not_only_the_selected_subtree() {
    let root = tempdir().expect("filesystem source");
    fs::create_dir(root.path().join("authored")).expect("selected subtree");
    fs::write(root.path().join("authored/readme.md"), b"abc").expect("source file");
    let prepared = source::prepare(
        vec![source::PlacementRequest {
            target: "project".parse().expect("target"),
            source: source::SourceSelection::Filesystem {
                root: root.path().to_owned(),
                subdirectory: Some("authored".parse().expect("subdirectory")),
            },
        }],
        limits(),
    )
    .expect("filesystem source");
    let destination = root.path().join("unselected-sibling");
    fs::create_dir(&destination).expect("sibling");
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            &destination,
            Limits::default()
        ),
        Err(DestinationError::SourceOverlap { .. })
    ));
}

#[test]
fn existing_targets_and_file_ancestors_are_never_overwritten_by_marker_presence() {
    let fixture = Fixture::new();
    for kind in ["directory", "marked directory", "file ancestor"] {
        let content = tempdir().expect("content");
        if kind == "file ancestor" {
            fs::write(content.path().join("docs"), b"manual").expect("manual file");
        } else {
            fs::create_dir_all(content.path().join("docs/api")).expect("manual target");
            if kind == "marked directory" {
                fs::write(
                    content.path().join("docs/api/.zola-project-docs"),
                    b"legacy marker",
                )
                .expect("historical marker fixture");
            }
        }
        let before = source::inspect(content.path(), None, Limits::default()).expect("before");
        let prepared =
            source::prepare(vec![request("docs/api", &fixture)], limits()).expect("sources");
        assert!(
            matches!(
                source::check_destination(
                    &prepared,
                    &super::context(&prepared),
                    content.path(),
                    Limits::default()
                ),
                Err(DestinationError::Collision { .. })
            ),
            "{kind}"
        );
        assert_eq!(
            before,
            source::inspect(content.path(), None, Limits::default()).expect("after")
        );
    }
}

#[test]
fn case_and_unicode_aliases_fail_without_rewriting_the_requested_spelling() {
    let fixture = Fixture::new();
    let content = tempdir().expect("content");
    for targets in [
        ["Docs/a", "docs/b"],
        ["café/a", "cafe\u{301}/b"],
        ["Straße/a", "STRASSE/b"],
    ] {
        let prepared = source::prepare(
            targets
                .iter()
                .map(|target| request(target, &fixture))
                .collect(),
            limits(),
        )
        .expect("logical sources");
        assert!(
            matches!(
                source::check_destination(
                    &prepared,
                    &super::context(&prepared),
                    content.path(),
                    Limits::default()
                ),
                Err(DestinationError::Collision { .. })
            ),
            "{targets:?}"
        );
        let mut preserved: Vec<_> = prepared
            .placements()
            .iter()
            .map(|placement| placement.request().target.as_str())
            .collect();
        preserved.sort_unstable();
        let mut expected = targets;
        expected.sort_unstable();
        assert_eq!(preserved, expected);
    }
    assert!(
        fs::read_dir(content.path())
            .expect("unchanged directory")
            .next()
            .is_none()
    );
}

#[test]
fn existing_case_aliases_and_symlinks_fail_closed() {
    let fixture = Fixture::new();
    let prepared = source::prepare(vec![request("docs/api", &fixture)], limits()).expect("sources");
    let content = tempdir().expect("content");
    fs::create_dir(content.path().join("Docs")).expect("different spelling");
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            content.path(),
            Limits::default()
        ),
        Err(DestinationError::Collision { .. })
    ));
    let links = tempdir().expect("links");
    symlink(content.path(), links.path().join("root")).expect("root link");
    for suffix in ["root", "root/", "root/."] {
        assert!(
            source::check_destination(
                &prepared,
                &super::context(&prepared),
                &links.path().join(suffix),
                Limits::default()
            )
            .is_err()
        );
    }
    symlink(content.path(), links.path().join("docs")).expect("descendant link");
    assert!(matches!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            links.path(),
            Limits::default()
        ),
        Err(DestinationError::Observation(source::Error::Unsupported(_)))
    ));
}

#[test]
fn missing_destination_is_not_created_and_complete_tree_limits_are_enforced() {
    let fixture = Fixture::new();
    let prepared = source::prepare(vec![request("docs/api", &fixture)], limits()).expect("sources");
    let outer = tempdir().expect("outer");
    let missing = outer.path().join("missing");
    assert!(
        source::check_destination(
            &prepared,
            &super::context(&prepared),
            &missing,
            Limits::default()
        )
        .is_err()
    );
    assert!(!missing.exists());
    let content = tempdir().expect("content");
    let proposed = source::check_destination(
        &prepared,
        &super::context(&prepared),
        content.path(),
        Limits::default(),
    )
    .expect("proposed");
    let metadata_bytes = proposed.inventory().total_bytes() - 3;
    let exact = Limits {
        entries: 4,
        depth: 3,
        file_bytes: metadata_bytes,
        total_bytes: metadata_bytes + 3,
    };
    assert_eq!(
        source::check_destination(&prepared, &super::context(&prepared), content.path(), exact)
            .expect("exact allowance")
            .inventory()
            .entries()
            .len(),
        4
    );
    for limit in [
        Limits {
            entries: 3,
            ..exact
        },
        Limits { depth: 2, ..exact },
        Limits {
            file_bytes: metadata_bytes - 1,
            ..exact
        },
        Limits {
            total_bytes: metadata_bytes + 2,
            ..exact
        },
    ] {
        assert!(
            matches!(
                source::check_destination(
                    &prepared,
                    &super::context(&prepared),
                    content.path(),
                    limit
                ),
                Err(DestinationError::Observation(source::Error::Limit { .. }))
            ),
            "{limit:?}"
        );
    }
    assert!(
        fs::read_dir(content.path())
            .expect("unchanged")
            .next()
            .is_none()
    );
}
