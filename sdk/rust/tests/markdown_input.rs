//! One complete pinned capture feeds inspection even after live files change.

use gazette_sdk::{markdown, source};
use std::fs;

fn selection(root: &std::path::Path, document: &str) -> markdown::input::Tree {
    markdown::input::Tree {
        root: root.into(),
        identity: source::inspect(root, None, Default::default())
            .expect("independent inventory")
            .identity(),
        document: document.parse().expect("entry"),
        limits: Default::default(),
    }
}

#[test]
fn pinned_markdown_uses_retained_bytes_and_original_evidence_after_live_change() {
    let root = tempfile::tempdir().expect("native root");
    fs::write(root.path().join("page.md"), "# Original é\n").expect("document");
    fs::write(root.path().join(".asset"), "original asset").expect("hidden asset");
    let input = selection(root.path(), "page.md");
    let identity = input.identity;
    let loaded = input.load(true).expect("single complete capture");
    fs::write(root.path().join("page.md"), "# Later\n").expect("live change");
    fs::write(root.path().join(".asset"), "later asset").expect("asset change");
    let report = markdown::inspect(
        loaded.text().expect("retained UTF-8"),
        Default::default(),
        true,
    )
    .expect("native parser");
    assert!(report.html.expect("fragment").contains("Original é"));
    assert_eq!(loaded.evidence().expect("original pin").identity, identity);
    assert_eq!(
        fs::read_to_string(root.path().join("page.md")).expect("live untouched"),
        "# Later\n"
    );
}

#[test]
fn permission_precedes_native_access_and_pins_cover_non_document_assets() {
    let root = tempfile::tempdir().expect("native root");
    fs::write(root.path().join("page.md"), "# Page\n").expect("document");
    fs::write(root.path().join("asset.bin"), b"one").expect("asset");
    let mut denied = selection(root.path(), "page.md");
    denied.root = root.path().join("absent");
    assert_eq!(
        denied.load(false).err().expect("denied before open").code,
        "markdown_read_permission"
    );
    let selected = selection(root.path(), "page.md");
    fs::write(root.path().join("asset.bin"), b"two").expect("only asset changed");
    assert!(
        selected.load(true).is_err(),
        "unchanged Markdown cannot hide a changed tree"
    );
}

#[test]
fn selected_document_must_be_regular_utf8_and_confined() {
    let root = tempfile::tempdir().expect("native root");
    fs::write(root.path().join("binary"), b"\xff").expect("native non-UTF-8 bytes");
    fs::create_dir(root.path().join("directory")).expect("directory");
    for (document, code) in [
        ("binary", "markdown_input"),
        ("directory", "markdown_document"),
        ("absent", "markdown_document"),
    ] {
        let loaded = selection(root.path(), document)
            .load(true)
            .expect("whole valid tree");
        assert_eq!(loaded.text().expect_err("not a Markdown file").code, code);
    }
    assert!("../binary".parse::<source::EntryPath>().is_err());
    assert!("/binary".parse::<source::EntryPath>().is_err());
}
