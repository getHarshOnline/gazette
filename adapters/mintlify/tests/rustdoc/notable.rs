//! Installed Rustdoc tooltips become static reference sections with working links.

use super::*;

#[test]
fn installed_notable_traits_keep_the_return_type_contract() {
    let directory = tempfile::tempdir().unwrap();
    let source = "//! Native byte-view reference.\n/// A retained byte view.\npub struct Bytes;\nimpl Bytes {\n/// Borrow the original bytes.\npub fn bytes(&self) -> &[u8] { &[] }\n}\n";
    let program = std::env::var_os("RUSTDOC").unwrap_or_else(|| "rustdoc".into());
    generate_source(directory.path(), Path::new(&program), source);
    let native = directory.path().join("html");
    let html = fs::read_to_string(native.join("reference_fixture/struct.Bytes.html")).unwrap();
    assert!(html.contains("notable-traits-data"));
    let input = capture(&native);
    let output = assemble(&input).expect("actual notable-trait data is preserved statically");
    let observed = observe_markdown(page(&output, "fixture/rust/struct.Bytes.md"));
    for text in [
        "Borrow the original bytes.",
        "Notable traits for",
        "impl",
        "Read",
        "&[u8]",
    ] {
        assert!(
            observed.text.contains(text),
            "missing {text}: {}",
            observed.text
        );
    }
    assert!(observed.anchors.contains("rustdoc-notable-traits-0"));
    assert!(
        observed
            .links
            .contains(&"/fixture/rust/struct.Bytes#rustdoc-notable-traits-0".into())
    );
    assert!(
        observed
            .links
            .iter()
            .any(|link| link.ends_with("std/io/trait.Read.html"))
    );
    assert!(!observed.tags.contains("script"));
    assert_eq!(
        output.inventory().identity(),
        assemble(&input).unwrap().inventory().identity()
    );
}
