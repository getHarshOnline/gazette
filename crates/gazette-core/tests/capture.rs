//! Payload checks at the retained-source boundary, not native-source assertions.

use gazette_core::source::{
    CapturedSource, Content, EntryPath, Error, InventoryBuilder, Limits, Sha256Digest, TreeInput,
};
use std::collections::BTreeMap;

fn inventory() -> gazette_core::source::Inventory {
    let mut builder = InventoryBuilder::new(Limits::default());
    builder
        .insert("docs".parse().expect("path"), Content::Directory)
        .expect("directory");
    builder
        .insert(
            "docs/a".parse().expect("path"),
            Content::File {
                bytes: 3,
                sha256: Sha256Digest::of(b"abc"),
            },
        )
        .expect("file");
    builder.finish()
}

#[test]
fn only_the_exact_file_set_lengths_and_digests_can_be_retained() {
    for mutation in ["missing", "extra", "directory", "length", "same-size bytes"] {
        let mut files: BTreeMap<EntryPath, Vec<u8>> =
            BTreeMap::from([("docs/a".parse().expect("path"), b"abc".to_vec())]);
        match mutation {
            "missing" => files.clear(),
            "extra" => {
                files.insert("extra".parse().expect("path"), b"abc".to_vec());
            }
            "directory" => {
                files.insert("docs".parse().expect("path"), Vec::new());
            }
            "length" => {
                files.insert("docs/a".parse().expect("path"), b"abcd".to_vec());
            }
            _ => {
                files.insert("docs/a".parse().expect("path"), b"xyz".to_vec());
            }
        }
        let inventory = inventory();
        let borrowed = files
            .iter()
            .map(|(path, bytes)| (path.clone(), bytes.as_slice()))
            .collect();
        assert!(
            matches!(
                TreeInput::new(&inventory, borrowed),
                Err(Error::InvalidInventory { .. })
            ),
            "borrowed {mutation}"
        );
        assert!(
            matches!(
                CapturedSource::from_parts(inventory, files),
                Err(Error::InvalidInventory { .. })
            ),
            "{mutation}"
        );
    }
}

#[test]
fn checked_tree_borrows_original_bytes_and_includes_empty_directories() {
    let inventory = inventory();
    let bytes = b"abc".to_vec();
    let path = "docs/a".parse().expect("path");
    let input =
        TreeInput::new(&inventory, BTreeMap::from([(path, bytes.as_slice())])).expect("tree");
    assert_eq!(input.inventory(), &inventory);
    let file = input.file(&"docs/a".parse().expect("path")).expect("file");
    assert_eq!(file, b"abc");
    assert_eq!(file.as_ptr(), bytes.as_ptr(), "no second payload copy");
    assert!(input.file(&"docs".parse().expect("directory")).is_none());
    assert!(input.file(&"missing".parse().expect("absent")).is_none());
}

#[test]
fn checked_payloads_are_readable_without_printing_contents_in_debug() {
    let files = BTreeMap::from([("docs/a".parse().expect("path"), b"abc".to_vec())]);
    let captured = CapturedSource::from_parts(inventory(), files).expect("capture");
    assert_eq!(captured.inventory(), &inventory());
    assert_eq!(
        captured.file(&"docs/a".parse().expect("path")),
        Some(b"abc".as_slice())
    );
    let address = captured
        .file(&"docs/a".parse().expect("path"))
        .expect("file")
        .as_ptr();
    let input = captured.as_input();
    assert!(std::ptr::eq(input.inventory(), captured.inventory()));
    assert_eq!(
        input
            .file(&"docs/a".parse().expect("path"))
            .expect("file")
            .as_ptr(),
        address
    );
    assert_eq!(input.file(&"docs".parse().expect("directory")), None);
    assert_eq!(input.file(&"missing".parse().expect("absent")), None);
    let (_, contents) = captured.into_parts();
    assert_eq!(
        contents.get(&"docs/a".parse().expect("path")),
        Some(b"abc".as_slice())
    );
    assert!(contents.get(&"docs".parse().expect("path")).is_none());
    assert_eq!(format!("{contents:?}"), "FileContents { file_count: 1 }");
    let mut files = contents.into_files();
    let file = files.get_mut("docs/a").expect("owned file");
    assert_eq!(
        file.as_ptr(),
        address,
        "ownership transfer must not copy payloads"
    );
    file[0] = b'x';
    assert!(matches!(
        CapturedSource::from_parts(inventory(), files),
        Err(Error::InvalidInventory { .. })
    ));
}
