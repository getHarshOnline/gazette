//! Includes constrain reads and keep missing or unsafe selections observable.

use std::{fs, os::unix::fs::symlink};

use gazette_adapter_fs::FileSystem;
use gazette_core::source::{EntryPath, Error, Limits};
use gazette_ports::source::SourceCapture;

fn paths(values: &[&str]) -> Vec<EntryPath> {
    values.iter().map(|value| value.parse().unwrap()).collect()
}

#[test]
fn included_trees_keep_relative_paths_and_ignore_new_unselected_siblings() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("manual/docs/empty")).unwrap();
    fs::write(root.path().join("manual/docs/.hidden"), "kept").unwrap();
    fs::write(root.path().join("README.md"), "readme").unwrap();
    let selected = paths(&["manual/docs", "README.md"]);
    let before = FileSystem
        .capture_including(root.path(), &selected, Limits::default())
        .unwrap();
    fs::write(root.path().join("manual/private.md"), "unselected").unwrap();
    fs::create_dir(root.path().join("docs-other")).unwrap();
    symlink("absent", root.path().join("docs-other/link")).unwrap();
    symlink("absent", root.path().join("private-link")).unwrap();
    let after = FileSystem
        .capture_including(root.path(), &selected, Limits::default())
        .unwrap();
    assert_eq!(before.inventory(), after.inventory());
    assert_eq!(
        after
            .inventory()
            .entries()
            .iter()
            .map(|entry| entry.path.as_str())
            .collect::<Vec<_>>(),
        [
            "README.md",
            "manual",
            "manual/docs",
            "manual/docs/.hidden",
            "manual/docs/empty"
        ]
    );
    assert_eq!(
        after.file(&"manual/docs/.hidden".parse().unwrap()),
        Some(b"kept".as_slice())
    );
    let reordered = FileSystem
        .capture_including(
            root.path(),
            &paths(&["README.md", "manual/docs"]),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(after.inventory(), reordered.inventory());
}

#[test]
fn missing_or_wrong_case_selection_fails_even_with_other_present_inputs() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("README.md"), "readme").unwrap();
    for missing in ["absent", "Docs"] {
        let error = FileSystem
            .capture_including(
                root.path(),
                &paths(&["README.md", missing]),
                Limits::default(),
            )
            .unwrap_err();
        assert!(
            matches!(error, Error::InvalidInventory { path, reason: "required source selection is missing" } if path.to_str() == Some(missing))
        );
    }
    assert!(matches!(
        FileSystem.capture_including(
            root.path(),
            &paths(&["README.md/child"]),
            Limits {
                file_bytes: 0,
                ..Limits::default()
            }
        ),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn selections_must_be_nonempty_unique_and_bounded() {
    let root = tempfile::tempdir().unwrap();
    for invalid in [
        vec![],
        paths(&["docs", "docs"]),
        (0..129)
            .map(|i| format!("file-{i}").parse().unwrap())
            .collect(),
    ] {
        assert!(matches!(
            FileSystem.capture_including(root.path(), &invalid, Limits::default()),
            Err(Error::InvalidInventory { .. })
        ));
    }
}

#[test]
fn selected_symlinks_and_symlinked_ancestors_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.md"), "private").unwrap();
    symlink(outside.path(), root.path().join("docs")).unwrap();
    for included in ["docs", "docs/private.md"] {
        assert!(matches!(
            FileSystem.capture_including(root.path(), &paths(&[included]), Limits::default()),
            Err(Error::Unsupported(_))
        ));
    }
}

#[test]
fn includes_preserve_all_capture_limits_and_reject_nested_links() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("docs/page.md"), "abc").unwrap();
    fs::write(root.path().join("README.md"), "abc").unwrap();
    let selected = paths(&["docs", "README.md"]);
    for limits in [
        Limits {
            entries: 2,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
        Limits {
            file_bytes: 2,
            ..Limits::default()
        },
        Limits {
            total_bytes: 5,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            FileSystem.capture_including(root.path(), &selected, limits),
            Err(Error::Limit { .. })
        ));
    }
    symlink("missing", root.path().join("docs/link")).unwrap();
    assert!(matches!(
        FileSystem.capture_including(root.path(), &selected, Limits::default()),
        Err(Error::Unsupported(_))
    ));
}
