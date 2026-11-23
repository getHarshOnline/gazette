//! Parent pin/source selection is shared by native preparation and durable history.

use gazette_core::{
    release::{ParentSource, WorktreeEffect, validate_worktree_parent},
    source::CommitId,
};

#[test]
fn root_and_parent_selection_are_explicit_without_source_inference() {
    let parent: CommitId = "1234567890123456789012345678901234567890"
        .parse()
        .expect("exact commit");
    let source = ParentSource::Bare {
        path: "/private/fixture/history".into(),
    };
    validate_worktree_parent(None, None).expect("explicit root");
    validate_worktree_parent(Some(&parent), Some(&source)).expect("explicit parent source");
    for (pin, selected) in [(None, Some(&source)), (Some(&parent), None)] {
        let error = validate_worktree_parent(pin, selected).expect_err("mismatched selection");
        assert_eq!(error.effect, WorktreeEffect::NotAttempted);
    }
}

#[test]
fn durable_source_locator_preserves_kind_and_exact_path_without_cleanup() {
    for kind in ["bare", "worktree"] {
        let encoded = format!(r#"{{"kind":"{kind}","path":"/private/fixture/हर्ष"}}"#);
        let selected: ParentSource = serde_json::from_str(&encoded).expect("typed source");
        selected.validate().expect("exact normal path");
        assert_eq!(selected.path().to_str(), Some("/private/fixture/हर्ष"));
        assert_eq!(
            serde_json::to_string(&selected).expect("exact encoding"),
            encoded
        );
    }
    for path in [
        "",
        "relative",
        "/",
        "/private/../history",
        "/private/./history",
        "/private//history",
        "/private/history/",
        "/private/a\0b",
    ] {
        let source = ParentSource::Bare { path: path.into() };
        assert!(
            source.validate().is_err(),
            "accepted ambiguous path {path:?}"
        );
    }
    for input in [
        r#"{"kind":"remote","path":"/private/history"}"#,
        r#"{"kind":"bare","path":"/private/history","branch":"main"}"#,
        r#"{"kind":"bare"}"#,
    ] {
        assert!(serde_json::from_str::<ParentSource>(input).is_err());
    }
}
