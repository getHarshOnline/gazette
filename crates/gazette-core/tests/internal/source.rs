use super::*;

#[test]
fn entry_names_are_decoded_without_lossy_conversion() {
    for name in [
        b"bad\xff".as_slice(),
        b".",
        b"..",
        b"",
        b"a/b",
        b"a\\b",
        b"a\0b",
    ] {
        assert!(matches!(
            entry_name(name, Path::new("docs")),
            Err(Error::InvalidName(_))
        ));
    }
    assert_eq!(
        entry_name(b"valid.md", Path::new("docs")).expect("name"),
        "valid.md"
    );
}
