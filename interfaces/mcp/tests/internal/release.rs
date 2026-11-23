//! Frozen real-signature fixtures reach the shared verifier, with no test issuer.

use super::*;
use gazette_cli::sdk::source::Sha256Digest;
use serde_json::json;

const AUTHORIZATION: &str = include_str!(
    "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
);
const BINDING: &str =
    include_str!("../../../../crates/gazette-compiler/tests/release_authorization/binding.json");

fn arguments(authorization: &str, binding: &str) -> Value {
    json!({"authorization_json":authorization,"binding_json":binding})
}

fn execute(input: Value, maximum: usize) -> CallToolResult {
    decode(input.as_object().expect("named request").clone())
        .expect("valid transport, not yet verified authorization")
        .execute(maximum)
        .expect("bounded tool response")
}

fn report(value: &CallToolResult) -> &Value {
    assert_eq!(value.is_error, Some(false), "{value:?}");
    value.structured_content.as_ref().expect("digest report")
}

fn failure(value: &CallToolResult) -> &str {
    assert_eq!(value.is_error, Some(true), "{value:?}");
    let structured = value.structured_content.as_ref().expect("redacted failure");
    assert!(structured.get("authorization_sha256").is_none());
    assert!(structured.get("status").is_none());
    structured["code"].as_str().expect("stable failure code")
}

fn native(authorization: &str, binding: &str, limits: release::Limits) -> Value {
    let binding =
        release::parse_binding(binding.as_bytes(), limits).expect("independent fixture binding");
    let verified =
        release::verify(authorization.as_bytes(), &binding, limits).expect("real signature");
    serde_json::to_value(verified.report()).expect("same report as CLI")
}

#[test]
fn real_signature_report_matches_sdk_and_retains_exact_authorization_transport() {
    let ordinary = execute(arguments(AUTHORIZATION, BINDING), 1 << 20);
    assert_eq!(
        report(&ordinary),
        &native(AUTHORIZATION, BINDING, release::Limits::default())
    );
    let spaced = format!(" \n{AUTHORIZATION}\t");
    let equivalent_binding = format!("\n{BINDING} ");
    let varied = execute(arguments(&spaced, &equivalent_binding), 1 << 20);
    assert_eq!(
        report(&varied),
        &native(&spaced, &equivalent_binding, release::Limits::default())
    );
    assert_eq!(
        report(&varied)["authorization_sha256"],
        json!(Sha256Digest::of(spaced.as_bytes()))
    );
    assert_ne!(
        report(&ordinary)["authorization_sha256"],
        report(&varied)["authorization_sha256"]
    );
    for field in [
        "canonical_body_sha256",
        "binding_sha256",
        "observed_at",
        "target_count",
        "publisher_count",
    ] {
        assert_eq!(report(&ordinary)[field], report(&varied)[field]);
    }
    let serialized = serde_json::to_value(&ordinary).expect("protocol result");
    let readable: Value = serde_json::from_str(
        serialized["content"][0]["text"]
            .as_str()
            .expect("JSON text"),
    )
    .expect("same native JSON projection");
    assert_eq!(&readable, report(&ordinary));
    for private in [
        "publisher:one",
        "example.invalid",
        "nonce:",
        "signature_envelope",
        "public_key",
    ] {
        assert!(
            !serialized.to_string().contains(private),
            "digest-only response must omit {private}"
        );
    }
}

#[test]
fn signature_binding_time_and_body_changes_preserve_native_redacted_errors() {
    let binding: Value = serde_json::from_str(BINDING).expect("fixture binding");
    let authorization: Value = serde_json::from_str(AUTHORIZATION).expect("fixture authorization");
    let mut signature = authorization.clone();
    signature["publisher_authorizations"][0]["signature_envelope"]["signature"] =
        json!("A".repeat(86) + "==");
    let mut mismatched = binding.clone();
    mismatched["coordinator_id"] = json!("PRIVATE-COORDINATOR");
    let mut expired = binding.clone();
    expired["observed_at"] = json!(200);
    let mut changed = authorization.clone();
    changed["nonce"] = json!("PRIVATE-NONCE");
    for (authorization, binding, expected) in [
        (
            signature.to_string(),
            BINDING.to_owned(),
            "release_signature",
        ),
        (
            AUTHORIZATION.to_owned(),
            mismatched.to_string(),
            "release_binding",
        ),
        (
            AUTHORIZATION.to_owned(),
            expired.to_string(),
            "release_time",
        ),
        (changed.to_string(), BINDING.to_owned(), "release_digest"),
    ] {
        let limits = release::Limits::default();
        let native = release::parse_binding(binding.as_bytes(), limits)
            .and_then(|binding| release::verify(authorization.as_bytes(), &binding, limits));
        assert_eq!(native.err().expect("native refusal").code(), expected);
        let result = execute(arguments(&authorization, &binding), 1 << 20);
        assert_eq!(failure(&result), expected);
        assert!(!format!("{result:?}").contains("PRIVATE"));
    }
    assert_eq!(
        report(&execute(arguments(AUTHORIZATION, BINDING), 1 << 20)),
        &native(AUTHORIZATION, BINDING, release::Limits::default())
    );
}

#[test]
fn duplicate_keys_and_numeric_spelling_reach_native_parser_without_value_normalization() {
    let duplicate_authorization =
        AUTHORIZATION.replacen("{", "{\"nonce\":\"PRIVATE-DUPLICATE\",", 1);
    let escaped_duplicate = AUTHORIZATION.replacen("{", "{\"n\\u006fnce\":\"PRIVATE-ESCAPED\",", 1);
    let duplicate_binding = BINDING.replacen("{", "{\"observed_at\":120,", 1);
    let negative_zero = BINDING.replace("\"observed_at\": 120", "\"observed_at\": -0");
    for (authorization, binding, expected) in [
        (duplicate_authorization.as_str(), BINDING, "release_json"),
        (escaped_duplicate.as_str(), BINDING, "release_json"),
        (AUTHORIZATION, duplicate_binding.as_str(), "release_json"),
        (AUTHORIZATION, negative_zero.as_str(), "release_number"),
    ] {
        let result = execute(arguments(authorization, binding), 1 << 20);
        assert_eq!(failure(&result), expected);
        assert!(!format!("{result:?}").contains("PRIVATE"));
    }
}

#[test]
fn every_native_limit_and_complete_output_ceiling_are_enforced() {
    for field in ["bytes", "nodes", "depth", "targets"] {
        let mut input = arguments(AUTHORIZATION, BINDING);
        input["limits"] = json!({field:0});
        assert_eq!(
            failure(&execute(input, 1 << 20)),
            "release_limit",
            "{field}"
        );
    }
    let mut input = arguments(AUTHORIZATION, BINDING);
    input["limits"] = json!({"bytes":AUTHORIZATION.len().max(BINDING.len()),"targets":1});
    let successful = execute(input.clone(), 1 << 20);
    let length = serde_json::to_vec(report(&successful))
        .expect("exact complete output")
        .len();
    assert_eq!(report(&execute(input.clone(), length)), report(&successful));
    assert_eq!(failure(&execute(input, length - 1)), "mcp_output_limit");
}

#[test]
fn discovery_and_decoding_agree_on_strings_objects_and_native_unsigned_limits() {
    let descriptor = serde_json::to_value(descriptor()).expect("tool descriptor");
    assert_eq!(descriptor["name"], "verify_release_authorization");
    for hint in ["readOnlyHint", "idempotentHint"] {
        assert_eq!(descriptor["annotations"][hint], true);
    }
    for hint in ["destructiveHint", "openWorldHint"] {
        assert_eq!(descriptor["annotations"][hint], false);
    }
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("typed input schema");
    let valid = arguments(AUTHORIZATION, BINDING);
    assert!(schema.is_valid(&valid));
    let beyond: Value =
        serde_json::from_str(&((usize::MAX as u128) + 1).to_string()).expect("exact integer");
    for field in ["bytes", "nodes", "depth", "targets"] {
        for (value, accepted) in [
            (json!(usize::MAX), true),
            (beyond.clone(), false),
            (json!(-1), false),
        ] {
            let mut input = valid.clone();
            input["limits"] = json!({field:value});
            assert_eq!(schema.is_valid(&input), accepted);
            assert_eq!(
                decode(input.as_object().expect("object").clone()).is_ok(),
                accepted
            );
        }
    }
    for input in [
        json!({"authorization_json":{},"binding_json":BINDING}),
        json!({"authorization_json":AUTHORIZATION,"binding_json":{}}),
        json!({"authorization_json":AUTHORIZATION,"binding_json":BINDING,"limits":[]}),
        json!({"authorization_json":AUTHORIZATION,"binding_json":BINDING,"limits":null}),
        json!({"authorization_json":AUTHORIZATION,"binding_json":BINDING,"limits":{"unknown":1}}),
        json!({"authorization_json":AUTHORIZATION,"binding_json":BINDING,"private_key":"PRIVATE"}),
    ] {
        assert!(!schema.is_valid(&input));
        let error = decode(input.as_object().expect("object").clone())
            .err()
            .expect("shape refused");
        assert!(!format!("{error:?}").contains("PRIVATE"));
    }
}

#[test]
fn output_schema_accepts_native_success_refusal_and_complete_result_limit() {
    let descriptor = serde_json::to_value(descriptor()).expect("descriptor");
    let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
    for response in [
        execute(arguments(AUTHORIZATION, BINDING), 1 << 20),
        execute(arguments("{", BINDING), 1 << 20),
        execute(arguments(AUTHORIZATION, BINDING), 1),
    ] {
        assert!(
            schema.is_valid(
                response
                    .structured_content
                    .as_ref()
                    .expect("structured response")
            )
        );
    }
    let mut wrong_digest = native(AUTHORIZATION, BINDING, release::Limits::default());
    wrong_digest["authorization_sha256"] = json!("not-a-digest");
    assert!(!schema.is_valid(&wrong_digest));
}
