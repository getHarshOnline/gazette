//! Pure release verification exposes one closed status with its existing wire spelling.

use gazette_compiler::release::VerificationStatus;
use serde_json::{Value, json};

#[test]
fn verification_status_schema_and_serialization_are_closed() {
    let schema = schemars::schema_for!(VerificationStatus);
    let one_of = schema
        .get("oneOf")
        .and_then(Value::as_array)
        .expect("verification status has one closed schema variant");
    assert_eq!(
        one_of[0].get("const"),
        Some(&json!("signature-and-binding-verified"))
    );
    assert_eq!(
        serde_json::to_value(VerificationStatus::SignatureAndBindingVerified)
            .expect("verification status serializes"),
        Value::String("signature-and-binding-verified".to_owned())
    );
}
