//! Qualification of the library actually linked into the native Git adapter.

#[test]
fn linked_git_provider_matches_the_qualified_local_only_build() {
    let native = git2::Version::get();
    assert_eq!(native.libgit2_version(), (1, 9, 7), "{native:?}");
    assert!(native.vendored(), "system substitution: {native:?}");
    assert!(!native.https(), "unexpected HTTPS support: {native:?}");
    assert!(!native.ssh(), "unexpected SSH support: {native:?}");
}
