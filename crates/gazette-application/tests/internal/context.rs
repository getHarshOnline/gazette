//! Actual template materialization keeps evidence independent of JSON map features.

use super::{context, encode};
use gazette_core::render::Error;
use serde::Serialize;
use serde_json::Value;

#[test]
fn context_sorts_only_objects_and_returns_exact_bytes_of_the_provider_value() {
    let original: Value = serde_json::from_str(
        r#"{"z":{"z":2,"a":1},"a":[{"z":"retained","a":184467440737095516160000001},3,1]}"#,
    )
    .expect("exact native JSON");
    let equivalent: Value = serde_json::from_str(
        r#"{"a":[{"a":184467440737095516160000001,"z":"retained"},3,1],"z":{"a":1,"z":2}}"#,
    )
    .expect("different object order, identical meaning");
    let expected =
        br#"{"a":[{"a":184467440737095516160000001,"z":"retained"},3,1],"z":{"a":1,"z":2}}"#;
    let (provider, bytes) =
        context(original.clone(), expected.len(), "context bytes").expect("exact ceiling");
    assert_eq!(bytes, expected);
    assert_eq!(
        serde_json::to_vec(&provider).expect("actual provider bytes"),
        bytes
    );
    assert_eq!(
        provider, original,
        "ordering cannot change meaning or exact numbers"
    );
    assert_eq!(provider["a"], original["a"], "array order stays authored");
    assert_eq!(
        context(equivalent, expected.len(), "context bytes").expect("same context"),
        (provider, bytes)
    );
    assert!(matches!(
        context(original.clone(), expected.len() - 1, "context bytes"),
        Err(Error::Limit("context bytes"))
    ));
    assert_eq!(
        context(original, expected.len(), "context bytes")
            .expect("reuse after refusal")
            .1,
        expected
    );
}

#[test]
fn ordinary_encoding_keeps_its_native_order_without_context_normalization() {
    #[derive(Serialize)]
    struct Native {
        z: u8,
        a: u8,
    }
    let bytes = encode(&Native { z: 2, a: 1 }, 64, "native bytes").expect("ordinary transport");
    assert_eq!(
        bytes, br#"{"z":2,"a":1}"#,
        "policy/receipt/raw transport is not rewritten"
    );
}
