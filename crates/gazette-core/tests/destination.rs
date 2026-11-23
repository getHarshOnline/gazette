//! Literal declaration validation does not infer a provider or authorize delivery.

use gazette_core::release::{DestinationError, PublicationDestination, validate_git_destination};
use serde_json::json;

#[test]
fn selected_coordinates_round_trip_unchanged_without_provider_inference() {
    for (remote, branch) in [
        ("https://EXAMPLE.invalid/a%2Fb.git", "main"),
        ("git@example.invalid:Repository.git", "publication/é"),
        ("/not-opened/local path.git", "release/v1"),
        ("--literal-not-executed", "HEAD"),
    ] {
        let value = json!({"kind":"git", "remote":remote, "branch":branch});
        let destination: PublicationDestination =
            serde_json::from_value(value.clone()).expect("explicit shape");
        destination.validate().expect("existing literal grammar");
        validate_git_destination(remote, branch).expect("borrowed verifier entrypoint");
        assert_eq!(
            serde_json::to_value(destination).expect("exact serialization"),
            value
        );
    }
}

#[test]
fn malformed_literal_branches_and_remotes_fail_with_redacted_errors() {
    for branch in [
        "", " main", "main ", "-main", "main.", "a..b", "a@{b", "@", "/a", "a/", "a//b", ".a",
        "a/.b", "a.lock", "a/b.lock", "a b", "a\tb", "a\nb", "a\u{7f}b", "a~b", "a^b", "a:b",
        "a?b", "a*b", "a[b", "a\\b",
    ] {
        assert_eq!(
            validate_git_destination("PRIVATE-REMOTE", branch),
            Err(DestinationError),
            "{branch:?}"
        );
    }
    for remote in [
        "",
        " padded",
        "padded ",
        "line\nbreak",
        "zero\0byte",
        "control\u{7f}",
    ] {
        assert_eq!(
            validate_git_destination(remote, "main"),
            Err(DestinationError)
        );
    }
    let error =
        validate_git_destination("PRIVATE-REMOTE", "PRIVATE invalid").expect_err("invalid literal");
    assert_eq!(error.to_string(), "invalid publication destination");
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn tagged_shape_has_no_defaults_unknown_fields_or_speculative_provider_variants() {
    for value in [
        json!({}),
        json!({"kind":"git"}),
        json!({"kind":"github","remote":"x","branch":"main"}),
        json!({"kind":"Git","remote":"x","branch":"main"}),
        json!({"kind":"git","remote":"x","branch":"main","force":false}),
        json!({"kind":"git","remote":12,"branch":"main"}),
        json!({"kind":"git","remote":"x","branch":null}),
        json!(["git", "x", "main"]),
        json!(null),
    ] {
        assert!(
            serde_json::from_value::<PublicationDestination>(value.clone()).is_err(),
            "unexpected destination shape: {value}"
        );
    }
    let shape: PublicationDestination =
        serde_json::from_value(json!({"kind":"git","remote":"x","branch":"../main"}))
            .expect("shape is not semantic validation");
    assert_eq!(shape.validate(), Err(DestinationError));
    assert!(
        serde_json::from_str::<PublicationDestination>(
            r#"{"kind":"git","remote":"x","remote":"y","branch":"main"}"#
        )
        .is_err()
    );
    let schema = serde_json::to_value(schemars::schema_for!(PublicationDestination))
        .expect("native public shape schema");
    assert!(schema.to_string().contains("additionalProperties"));
}
