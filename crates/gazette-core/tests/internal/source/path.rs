use super::*;

#[test]
fn rejects_nul_before_native_path_conversion() {
    // The CLI cannot receive a NUL argument; the public parser still must
    // reject it before passing a component to a native directory operation.
    assert!("docs/\0private".parse::<Subdirectory>().is_err());
}
