//! Exact commit-recipe transport stays shared without becoming native object evidence.

use gazette_core::release::{CommitEffect, CommitMetadata};
use serde_json::{Value, json};

fn recipe() -> Value {
    json!({
        "parent": null,
        "author": {"name":"Author", "email":"author@example.invalid", "seconds":100, "offset_minutes":330},
        "committer": {"name":"Committer", "email":"committer@example.invalid", "seconds":101, "offset_minutes":-60},
        "message":"Exact café\r\nsecond line\n"
    })
}

#[test]
fn complete_recipe_preserves_explicit_parent_attribution_and_message_bytes() {
    let mut value = recipe();
    for parent in [Value::Null, json!("a".repeat(40))] {
        value["parent"] = parent;
        let bytes = serde_json::to_vec_pretty(&value).expect("fixture JSON");
        let parsed = CommitMetadata::parse_json(&bytes, bytes.len()).expect("exact bounded recipe");
        assert_eq!(
            serde_json::to_value(&parsed).expect("native projection"),
            value
        );
        assert_eq!(
            parsed.message.as_bytes(),
            "Exact café\r\nsecond line\n".as_bytes()
        );
        parsed.validate().expect("separate native recipe validity");
    }
}

#[test]
fn incomplete_positional_duplicate_and_unknown_recipe_shapes_are_refused() {
    let value = recipe();
    let encoded = serde_json::to_string(&value).expect("fixture JSON");
    let mut cases = vec![
        b"[]".to_vec(),
        b"null".to_vec(),
        b"{}".to_vec(),
        vec![0xff],
        format!("{encoded} true").into_bytes(),
        encoded
            .replace("\"parent\":null", "\"parent\":null,\"par\\u0065nt\":null")
            .into_bytes(),
        encoded
            .replace(
                "\"name\":\"Author\"",
                "\"name\":\"Author\",\"na\\u006de\":\"Author\"",
            )
            .into_bytes(),
    ];
    let mut missing = value.clone();
    missing.as_object_mut().expect("object").remove("parent");
    let mut unknown = value.clone();
    unknown["extra"] = json!(true);
    let mut nested_unknown = value.clone();
    nested_unknown["author"]["extra"] = json!(true);
    let mut positional = value.clone();
    positional["author"] = json!(["Author", "author@example.invalid", 100, 330]);
    let mut fractional = value.clone();
    fractional["author"]["seconds"] = json!(100.5);
    for changed in [missing, unknown, nested_unknown, positional, fractional] {
        cases.push(serde_json::to_vec(&changed).expect("invalid fixture shape"));
    }
    cases.push(
        serde_json::to_vec(&json!([
            value["parent"],
            value["author"],
            value["committer"],
            value["message"]
        ]))
        .expect("positional recipe"),
    );
    for bytes in cases {
        let error =
            CommitMetadata::parse_json(&bytes, bytes.len()).expect_err("closed native shape");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
        assert!(!error.to_string().contains("example.invalid"));
    }
}

#[test]
fn byte_ceiling_precedes_decoding_and_shape_does_not_establish_recipe_validity() {
    let bytes = serde_json::to_vec(&recipe()).expect("fixture JSON");
    for input in [&bytes[..], &[0xff][..]] {
        let error = CommitMetadata::parse_json(input, input.len() - 1).expect_err("byte ceiling");
        assert_eq!(error.reason, "metadata input exceeds byte ceiling");
        assert_eq!(error.effect, CommitEffect::NotAttempted);
    }
    let mut invalid = recipe();
    invalid["message"] = json!("");
    let encoded = serde_json::to_vec(&invalid).expect("complete shape");
    let parsed = CommitMetadata::parse_json(&encoded, encoded.len()).expect("representation only");
    assert!(
        parsed.validate().is_err(),
        "native recipe validation remains required"
    );
}
