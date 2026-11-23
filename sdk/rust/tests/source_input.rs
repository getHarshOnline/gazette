//! Shared named placement decoding does not acquire sources or infer bindings.

use gazette_sdk::source::{SourceSelection, input};
use serde_json::json;

fn placement() -> serde_json::Value {
    json!({"target":"/docs/project/","project":"project:one","deployment":"deployment:one","source":{"kind":"repository","root":"/does-not-exist","commit":"1111111111111111111111111111111111111111","subdirectory":"docs"}})
}

#[test]
fn explicit_native_choices_and_identity_bind_once_without_reading_paths() {
    let mut filesystem = placement();
    filesystem["target"] = json!("local");
    filesystem["source"] =
        json!({"kind":"filesystem","root":"/plain-absent","subdirectory":"authored"});
    let mut submodule = placement();
    submodule["target"] = json!("other");
    submodule["source"] = json!({"kind":"submodule","parent":"/also-absent","parent_commit":"2222222222222222222222222222222222222222","submodule_path":"child","subdirectory":"docs"});
    let (context, requests) = input::bind(
        "workspace:one".into(),
        "site:one".into(),
        vec![
            input::parse(&filesystem.to_string()).expect("filesystem"),
            input::parse(&placement().to_string()).expect("repository"),
            input::parse(&submodule.to_string()).expect("gitlink"),
        ],
    )
    .expect("pure binding");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].target.as_str(), "docs/project");
    assert_eq!(
        context
            .binding(&requests[1].target)
            .expect("binding")
            .project,
        "project:one"
    );
    assert!(
        matches!(&requests[0].source, SourceSelection::Filesystem { root, subdirectory: Some(subdirectory) } if root.to_str() == Some("/plain-absent") && subdirectory.to_string() == "authored")
    );
    assert!(
        matches!(&requests[1].source, SourceSelection::Repository { root, commit, subdirectory } if root.to_str() == Some("/does-not-exist") && commit.as_str() == "1111111111111111111111111111111111111111" && subdirectory.to_string() == "docs")
    );
    assert!(
        matches!(&requests[2].source, SourceSelection::Submodule { parent, parent_commit, submodule_path, subdirectory } if parent.to_str() == Some("/also-absent") && parent_commit.as_str() == "2222222222222222222222222222222222222222" && submodule_path.to_string() == "child" && subdirectory.to_string() == "docs")
    );
}

#[test]
fn malformed_duplicate_positional_and_oversized_inputs_fail_without_repair() {
    let valid = placement().to_string();
    for input in [
        valid.replacen("{", "{\"target\":\"other\",", 1),
        valid.replace("\"kind\":", "\"root\":\"duplicate\",\"kind\":"),
        valid.replacen("{", "{\"unexpected\":true,", 1),
        valid.replace("1111111111111111111111111111111111111111", "HEAD"),
        valid.replace("\"subdirectory\":\"docs\"", "\"subdirectory\":\"../docs\""),
        format!("{valid} false"),
        json!([
            "docs",
            "project:one",
            "deployment:one",
            placement()["source"]
        ])
        .to_string(),
        " ".repeat(16 * 1024 + 1),
    ] {
        assert!(input::parse(&input).is_err(), "{input}");
    }
    for source in [
        json!(["filesystem", "/missing", "docs"]),
        json!([
            "repository",
            "/missing",
            "1111111111111111111111111111111111111111",
            "docs"
        ]),
        json!([
            "submodule",
            "/missing",
            "1111111111111111111111111111111111111111",
            "child",
            "docs"
        ]),
    ] {
        let mut nested = placement();
        nested["source"] = source;
        let encoded = nested.to_string();
        assert!(input::parse(&encoded).is_err());
        // Protocol callers embed the same decoder rather than use CLI text parsing.
        let mut decoder = serde_json::Deserializer::from_str(&encoded);
        assert!(input::object(&mut decoder).is_err());
    }
}

#[test]
fn complete_binding_rejects_normalized_duplicates_and_bad_identities() {
    let first = input::parse(&placement().to_string()).expect("placement");
    let mut same = placement();
    same["target"] = json!("docs/project");
    assert!(
        input::bind(
            "workspace:one".into(),
            "site:one".into(),
            vec![
                first.clone(),
                input::parse(&same.to_string()).expect("same target")
            ]
        )
        .is_err()
    );
    for identity in ["", "bad\nidentity"] {
        assert!(input::bind(identity.into(), "site:one".into(), vec![first.clone()]).is_err());
    }
    let (context, requests) = input::bind("workspace:one".into(), "site:one".into(), vec![])
        .expect("explicit authored-only set");
    assert!(context.placements.is_empty());
    assert!(requests.is_empty());
}
