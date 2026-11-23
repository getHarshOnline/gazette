//! Structured failures retain native diagnostics without fabricating editor ranges.

use gazette_sdk::{toml, workspace};
use serde_json::json;

#[test]
fn actual_parser_and_semantic_errors_keep_their_original_diagnostics() {
    for source in [
        "[unfinished",
        "",
        "[[sites]]\nid='site:one'\npublisher='person:absent'\ndeployable=false\n[[sites]]\nid='site:two'\npublisher='person:also-absent'\ndeployable=false\n",
    ] {
        let error = workspace::ParsedWorkspace::parse(source, toml::Limits::default())
            .expect_err("invalid complete workspace");
        let value = serde_json::to_value(workspace::Failure::from(&error)).expect("failure JSON");
        assert_eq!(value["code"], error.code());
        assert_eq!(value["message"], error.to_string());
        match &error {
            workspace::Error::Invalid(diagnostics) => {
                if source.starts_with("[[sites]]") {
                    assert_eq!(
                        diagnostics.len(),
                        2,
                        "both missing publishers remain diagnosed"
                    );
                    assert!(
                        diagnostics
                            .iter()
                            .all(|diagnostic| diagnostic.code == "workspace_reference")
                    );
                }
                assert_eq!(value["diagnostics"], json!(diagnostics));
                assert!(value.get("span").is_none());
            }
            workspace::Error::Toml(toml::Error::Invalid { span, .. }) => {
                assert_eq!(value.get("span").cloned(), span.map(|span| json!(span)));
                assert!(value.get("diagnostics").is_none());
            }
            other => panic!("unexpected error: {other}"),
        }
    }
}

#[test]
fn limits_and_origin_failures_do_not_receive_invented_locations() {
    let error = workspace::compile(
        "# private",
        toml::Limits {
            source_bytes: 1,
            ..toml::Limits::default()
        },
    )
    .expect_err("source limit");
    let value = serde_json::to_value(workspace::Failure::from(&error)).expect("failure JSON");
    assert_eq!(value["code"], "toml_limit");
    assert!(value.get("span").is_none());
    assert!(value.get("diagnostics").is_none());
    for (error, code) in [
        (
            workspace::OriginError::UnknownIdentity,
            "origin_unknown_identity",
        ),
        (
            workspace::OriginError::NoExplicitField,
            "origin_no_explicit_field",
        ),
    ] {
        let value = serde_json::to_value(workspace::Failure::from(error)).expect("origin JSON");
        assert_eq!(value, json!({ "code": code, "message": error.to_string() }));
    }
}
