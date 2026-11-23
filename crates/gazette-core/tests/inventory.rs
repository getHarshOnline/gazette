//! Structural source contracts, independent of filesystem or Git mechanics.

use gazette_core::source::{Content, EntryPath, Error, InventoryBuilder, Limits, Sha256Digest};

fn file(bytes: u64) -> Content {
    Content::File {
        bytes,
        sha256: [0xab; 32].into(),
    }
}

#[test]
fn entry_paths_preserve_literal_names_but_reject_ambiguous_structure() {
    for invalid in [
        "",
        "/absolute",
        "a/",
        "a//b",
        "a/../b",
        "./a",
        "a\\b",
        "a\0b",
        "a\nb",
    ] {
        assert!(invalid.parse::<EntryPath>().is_err(), "{invalid:?}");
    }
    for literal in ["a%2fb", "a:b", "release notes/हर्ष.md"] {
        let path: EntryPath = literal.parse().expect("literal observed name");
        assert_eq!(path.as_str(), literal);
        assert_eq!(serde_json::to_value(path).expect("JSON"), literal);
    }
}

#[test]
fn fixed_width_digest_has_one_lowercase_text_projection() {
    let mut bytes = [0; 32];
    bytes[31] = 0xaf;
    let digest = Sha256Digest::from(bytes);
    let expected = format!("{}af", "0".repeat(62));
    assert_eq!(digest.to_string(), expected);
    assert_eq!(serde_json::to_value(digest).expect("JSON"), expected);
}

#[test]
fn ordering_and_totals_are_derived_once_for_all_projections() {
    let build = |reverse: bool| {
        let mut builder = InventoryBuilder::new(Limits::default());
        builder
            .insert("docs".parse().expect("path"), Content::Directory)
            .expect("directory");
        let names = if reverse {
            ["docs/z.md", "docs/a.md"]
        } else {
            ["docs/a.md", "docs/z.md"]
        };
        for name in names {
            builder
                .insert(name.parse().expect("path"), file(3))
                .expect("file");
        }
        builder.finish()
    };
    let first = build(false);
    let second = build(true);
    assert_eq!(first, second);
    assert_eq!(first.total_bytes(), 6);
    assert_eq!(
        first
            .entries()
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        ["docs", "docs/a.md", "docs/z.md"]
    );
    let json = serde_json::to_value(&first).expect("JSON");
    assert_eq!(json, serde_json::to_value(&second).expect("reverse JSON"));
    assert_eq!(json["total_bytes"], 6);
    assert_eq!(json["entries"][0]["kind"], "directory");
    assert_eq!(json["entries"][1]["bytes"], 3);
    assert_eq!(json["entries"][1]["sha256"], "ab".repeat(32));
}

#[test]
fn duplicates_missing_parents_and_file_parents_cannot_enter_an_inventory() {
    let mut builder = InventoryBuilder::new(Limits::default());
    assert!(matches!(
        builder.insert("docs/file".parse().expect("path"), file(3)),
        Err(Error::InvalidInventory { .. })
    ));
    builder
        .insert("docs".parse().expect("path"), file(2))
        .expect("root file");
    assert!(matches!(
        builder.insert("docs/file".parse().expect("path"), file(3)),
        Err(Error::InvalidInventory { .. })
    ));
    assert!(matches!(
        builder.insert("docs".parse().expect("path"), Content::Directory),
        Err(Error::InvalidInventory { .. })
    ));
    let inventory = builder.finish();
    assert_eq!(inventory.entries().len(), 1);
    assert_eq!(inventory.total_bytes(), 2);
    assert!(matches!(
        inventory.entries()[0].content,
        Content::File { bytes: 2, .. }
    ));
}

#[test]
fn all_limits_apply_before_insertion_and_rejection_preserves_state() {
    for limits in [
        Limits {
            entries: 0,
            ..Limits::default()
        },
        Limits {
            depth: 0,
            ..Limits::default()
        },
        Limits {
            file_bytes: 2,
            ..Limits::default()
        },
        Limits {
            total_bytes: 2,
            ..Limits::default()
        },
    ] {
        let mut builder = InventoryBuilder::new(limits);
        assert!(matches!(
            builder.insert("file".parse().expect("path"), file(3)),
            Err(Error::Limit { .. })
        ));
        let inventory = builder.finish();
        assert!(inventory.entries().is_empty());
        assert_eq!(inventory.total_bytes(), 0);
    }
    let mut builder = InventoryBuilder::new(Limits {
        file_bytes: u64::MAX,
        total_bytes: u64::MAX,
        ..Limits::default()
    });
    builder
        .insert("first".parse().expect("path"), file(u64::MAX))
        .expect("exact ceiling");
    assert_eq!(builder.file_ceiling(), 0);
    assert!(matches!(
        builder.insert("overflow".parse().expect("path"), file(1)),
        Err(Error::Limit { .. })
    ));
    assert_eq!(builder.finish().total_bytes(), u64::MAX);
}

#[test]
fn directory_depth_and_cumulative_file_budget_are_shared_rules() {
    let mut builder = InventoryBuilder::new(Limits {
        depth: 1,
        file_bytes: 5,
        total_bytes: 7,
        ..Limits::default()
    });
    builder
        .insert("docs".parse().expect("path"), Content::Directory)
        .expect("directory");
    assert!(matches!(
        builder.check_path(&"docs/child".parse().expect("path")),
        Err(Error::Limit { .. })
    ));
    builder
        .insert("first".parse().expect("path"), file(5))
        .expect("first file");
    assert_eq!(builder.file_ceiling(), 2);
    assert!(matches!(
        builder.insert("second".parse().expect("path"), file(3)),
        Err(Error::Limit { .. })
    ));
    builder
        .insert("second".parse().expect("path"), file(2))
        .expect("remaining allowance");
    assert_eq!(builder.finish().total_bytes(), 7);
}

#[test]
fn remaining_capacity_preserves_file_ceiling_and_refusals_do_not_consume_it() {
    let mut builder = InventoryBuilder::new(Limits {
        entries: 3,
        depth: 2,
        file_bytes: 5,
        total_bytes: 7,
    });
    builder
        .insert(
            "docs".parse().expect("valid directory path"),
            Content::Directory,
        )
        .expect("directory fits the selected limits");
    builder
        .insert("first".parse().expect("valid first path"), file(5))
        .expect("first file fits the selected limits");
    let remaining = builder.remaining_limits();
    assert_eq!(remaining.entries, 1);
    assert_eq!(remaining.total_bytes, 2);
    assert_eq!(remaining.file_bytes, 5);
    assert_eq!(remaining.depth, 2);
    assert_eq!(builder.file_ceiling(), 2);
    assert!(
        builder
            .insert("second".parse().expect("valid second path"), file(3))
            .is_err()
    );
    assert_eq!(builder.remaining_limits(), remaining);
    builder
        .insert("second".parse().expect("valid second path"), file(2))
        .expect("second file fits the remaining allowance");
    assert_eq!(builder.remaining_limits().entries, 0);
    assert_eq!(builder.remaining_limits().total_bytes, 0);
    assert_eq!(builder.remaining_limits().file_bytes, 5);
}
