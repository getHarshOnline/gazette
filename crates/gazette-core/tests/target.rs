//! URL-path normalization before logical placement or destination work.

use gazette_core::source::TargetPath;

#[test]
fn target_paths_normalize_safe_escapes_and_optional_outer_slashes() {
    for spelling in [
        "docs/release%20notes",
        "/docs/release notes/",
        "/%64ocs/release%20notes",
    ] {
        let target: TargetPath = spelling.parse().expect("target");
        assert_eq!(target.as_str(), "docs/release notes");
        assert_eq!(
            serde_json::to_value(target).expect("JSON"),
            "docs/release notes"
        );
    }
    assert_eq!(
        "/%E0%A4%B9%E0%A4%B0%E0%A5%8D%E0%A4%B7"
            .parse::<TargetPath>()
            .expect("Unicode")
            .as_str(),
        "हर्ष"
    );
}

#[test]
fn ambiguous_targets_are_rejected_without_path_cleanup() {
    for value in [
        "",
        "/",
        "//a",
        "a//",
        "a//b",
        ".",
        "a/..",
        "/%2e%2e/",
        "a/%2f/b",
        "a%5cb",
        "a\\b",
        "C:/docs",
        "%43%3a/docs",
        "a/%00",
        "a\nb",
        "a/%252e%252e",
        "a/%25",
        "a/%",
        "a/%2",
        "a/%gg",
        "a/%ff",
        "a?query",
        "a#fragment",
    ] {
        assert!(value.parse::<TargetPath>().is_err(), "{value:?}");
    }
}

#[test]
fn logical_paths_preserve_case_and_unicode_without_claiming_filesystem_equivalence() {
    assert_ne!(
        "Docs".parse::<TargetPath>().expect("upper"),
        "docs".parse::<TargetPath>().expect("lower")
    );
    assert_ne!(
        "café".parse::<TargetPath>().expect("composed"),
        "cafe\u{301}".parse::<TargetPath>().expect("decomposed")
    );
}
