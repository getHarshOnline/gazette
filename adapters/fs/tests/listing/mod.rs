use std::{fs, os::unix::fs::symlink};

use gazette_adapter_fs::FileSystem;
use gazette_core::source::{Error, Limits, ListedEntry, ListingBuilder, ListingLimits};
use gazette_ports::source::{SourceCapture, SourceListing};

#[test]
fn directory_and_exclusion_listing_keep_hidden_entries_and_confine_subdirectories() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("pages/.hidden"), "a").unwrap();
    symlink("pages", root.path().join("alias")).unwrap();
    let subdirectory = "pages".parse().unwrap();
    let listing = FileSystem
        .list(root.path(), Some(&subdirectory), ListingLimits::default())
        .unwrap();
    assert_eq!(
        listing
            .entries()
            .keys()
            .map(|p| p.as_str())
            .collect::<Vec<_>>(),
        [".hidden"]
    );
    let listing = FileSystem
        .list_excluding(
            root.path(),
            &["alias".parse().unwrap()],
            ListingLimits::default(),
        )
        .unwrap();
    assert_eq!(listing.entries().len(), 2);
    assert!(
        FileSystem
            .list(
                root.path(),
                Some(&"alias".parse().unwrap()),
                ListingLimits::default()
            )
            .is_err()
    );
    assert!(
        FileSystem
            .list(&root.path().join("alias/"), None, ListingLimits::default())
            .is_err()
    );
    assert!(
        FileSystem
            .list(root.path(), None, ListingLimits::default())
            .is_err()
    );
    assert!(
        FileSystem
            .list_excluding(root.path(), &[], ListingLimits::default())
            .is_err()
    );
}

#[test]
fn discovery_catalogs_large_assets_without_capturing_their_payloads() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("pages/.hidden"), "visible to discovery").unwrap();
    let asset = fs::File::create(root.path().join("pages/film.mp4")).unwrap();
    asset.set_len(64_u64 << 30).unwrap();
    symlink("missing", root.path().join("unselected-link")).unwrap();
    let selected = ["pages".parse().unwrap()];
    let listing = FileSystem
        .list_including(root.path(), &selected, ListingLimits::default())
        .unwrap();
    assert_eq!(listing.entries().len(), 3);
    assert_eq!(
        listing.entries().get("pages/film.mp4"),
        Some(&ListedEntry::File {
            observed_bytes: 64_u64 << 30
        })
    );
    assert!(listing.entries().contains_key("pages/.hidden"));
    assert!(matches!(
        FileSystem.capture_including(root.path(), &selected, Limits::default()),
        Err(Error::Limit { .. })
    ));
}

#[test]
fn metadata_listing_requires_exact_selections_and_rejects_selected_links() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("pages/a.md"), "a").unwrap();
    symlink("a.md", root.path().join("pages/link.md")).unwrap();
    let list = |paths: &[&str]| {
        FileSystem.list_including(
            root.path(),
            &paths.iter().map(|p| p.parse().unwrap()).collect::<Vec<_>>(),
            ListingLimits::default(),
        )
    };
    assert!(list(&["pages"]).is_err());
    assert!(list(&["pages/link.md"]).is_err());
    assert!(list(&["missing"]).is_err());
    assert!(list(&["pages/a.md", "pages/a.md"]).is_err());
    assert!(list(&[]).is_err());
    let listing = list(&["pages/a.md"]).unwrap();
    assert_eq!(listing.entries().len(), 2);
    assert_eq!(
        listing.entries().get("pages"),
        Some(&ListedEntry::Directory)
    );
}

#[test]
fn listing_bounds_metadata_instead_of_asset_bytes() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("pages")).unwrap();
    fs::write(root.path().join("pages/a.md"), "a").unwrap();
    let selected = ["pages".parse().unwrap()];
    for limits in [
        ListingLimits {
            entries: 1,
            ..ListingLimits::default()
        },
        ListingLimits {
            depth: 1,
            ..ListingLimits::default()
        },
        ListingLimits {
            path_bytes: 5,
            ..ListingLimits::default()
        },
    ] {
        assert!(matches!(
            FileSystem.list_including(root.path(), &selected, limits),
            Err(Error::Limit { .. })
        ));
    }
    assert!(
        FileSystem
            .list_including(
                root.path(),
                &selected,
                ListingLimits {
                    entries: 2,
                    depth: 2,
                    path_bytes: 15
                }
            )
            .is_ok()
    );
}

#[test]
fn listing_builder_rejects_inconsistent_structure_without_losing_capacity() {
    let mut builder = ListingBuilder::new(ListingLimits {
        entries: 2,
        depth: 2,
        path_bytes: 15,
    });
    assert!(
        builder
            .insert(
                "pages/a.md".parse().unwrap(),
                ListedEntry::File { observed_bytes: 0 }
            )
            .is_err()
    );
    builder
        .insert("pages".parse().unwrap(), ListedEntry::Directory)
        .unwrap();
    assert!(
        builder
            .insert("pages".parse().unwrap(), ListedEntry::Directory)
            .is_err()
    );
    builder
        .insert(
            "pages/a.md".parse().unwrap(),
            ListedEntry::File {
                observed_bytes: u64::MAX,
            },
        )
        .unwrap();
    assert_eq!(builder.finish().entries().len(), 2);
    let mut builder = ListingBuilder::new(ListingLimits::default());
    builder
        .insert(
            "file".parse().unwrap(),
            ListedEntry::File { observed_bytes: 0 },
        )
        .unwrap();
    assert!(
        builder
            .insert("file/child".parse().unwrap(), ListedEntry::Directory)
            .is_err()
    );
}

#[test]
fn filtered_directory_entry_does_not_enumerate_unselected_children() {
    use gazette_core::source::EntryPath;
    use gazette_ports::source::SourceFilter;

    struct DirectoryEntry;
    impl SourceFilter for DirectoryEntry {
        fn allows(&self, path: &EntryPath) -> Result<bool, Error> {
            assert_eq!(
                path.as_str(),
                "archive",
                "children must not even reach the filter"
            );
            Ok(true)
        }
        fn descend(&self, _: &EntryPath) -> bool {
            false
        }
    }
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("archive");
    fs::create_dir(&archive).unwrap();
    fs::write(archive.join("unselected\nchild"), b"invalid child name").unwrap();
    fs::write(archive.join("large.bin"), vec![b'x'; 8192]).unwrap();
    symlink("absent", archive.join("unselected-link")).unwrap();
    let listing = FileSystem
        .list_filtered(
            root.path(),
            None,
            &DirectoryEntry,
            ListingLimits {
                entries: 1,
                depth: 1,
                path_bytes: "archive".len(),
            },
        )
        .expect("the existing directory alone fits without examining any child");
    assert_eq!(listing.entries().len(), 1);
    assert_eq!(listing.entries()["archive"], ListedEntry::Directory);
}
