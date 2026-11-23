//! Actual read-only executable transport; supplied bindings are not native authority acquisition.

use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

use gazette_sdk::{release, source::Sha256Digest};
use serde_json::{Value, json};
use tempfile::tempdir;

// One shared independent signed fixture; the interface has no signing dependency
// or test-only issuance algorithm of its own.
const AUTHORIZATION: &str =
    include_str!("../../../crates/gazette-compiler/tests/release_authorization/authorization.json");
const BINDING: &str =
    include_str!("../../../crates/gazette-compiler/tests/release_authorization/binding.json");

fn envelope(authorization: &str, binding: &str) -> Vec<u8> {
    format!("{{\"authorization\":{authorization},\"binding\":{binding}}}").into_bytes()
}

fn report(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("complete verification report")
}

fn run(bytes: &[u8], arguments: &[&str]) -> Output {
    let cwd = tempdir().expect("independent empty working directory");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gazette"))
        .arg("verify-release-authorization")
        .args(arguments)
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Gazette verifier");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(bytes)
        .expect("private request");
    let output = child.wait_with_output().expect("verifier exit");
    assert!(
        std::fs::read_dir(cwd.path())
            .expect("working directory")
            .next()
            .is_none()
    );
    output
}

fn rejected(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        output.stdout.is_empty(),
        "no partial verification: {output:?}"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("gazette: {code}: no verification report produced; no release effects\n")
    );
}

#[test]
fn private_envelope_rejects_wrong_shapes_duplicates_unknown_fields_and_trailing_data() {
    for input in [
        "",
        "[]",
        "[{},{}]",
        "null",
        "\"PRIVATE_SENTINEL\"",
        "{}",
        "{\"authorization\":{}}",
        "{\"binding\":{}}",
        "{\"authorization\":{},\"binding\":{},\"PRIVATE_SENTINEL\":true}",
        "{\"authorization\":{},\"authorization\":{},\"binding\":{}}",
        "{\"authorization\":{},\"binding\":{},\"bind\\u0069ng\":{}}",
        "{\"authorization\":{},\"binding\":{}} PRIVATE_SENTINEL",
        "{\"authorization\":PRIVATE_SENTINEL,\"binding\":{}}",
    ] {
        rejected(&run(input.as_bytes(), &[]), "release_request");
    }
    rejected(&run(b"\xffPRIVATE_SENTINEL", &[]), "release_request");
}

#[test]
fn whole_envelope_byte_ceiling_precedes_private_json_decoding() {
    let input = b"PRIVATE_SENTINEL";
    rejected(&run(input, &["--max-bytes", "0"]), "release_input_limit");
    rejected(&run(input, &["--max-bytes", "15"]), "release_input_limit");
    rejected(&run(input, &["--max-bytes", "16"]), "release_request");
}

#[test]
fn help_names_the_verification_boundary_and_has_no_effect_options() {
    let output = run(b"", &["--help"]);
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout).expect("help UTF-8");
    for expected in [
        "stdin",
        "no clock, native reads, writes or release effects",
        "--max-bytes",
        "--max-nodes",
        "--max-depth",
        "--max-targets",
    ] {
        assert!(help.contains(expected), "missing {expected}: {help}");
    }
    for unsupported in [
        "--publish",
        "--advance-writer-generation",
        "--allow-coordination-files",
    ] {
        assert_eq!(run(b"", &[unsupported]).status.code(), Some(2));
    }
}

#[test]
fn signed_fixture_reaches_the_same_pure_sdk_and_cli_without_native_effects() {
    let authorization = AUTHORIZATION.trim();
    let binding = release::parse_binding(BINDING.as_bytes(), release::Limits::default())
        .expect("independently retained test binding");
    let verified = release::verify(
        authorization.as_bytes(),
        &binding,
        release::Limits::default(),
    )
    .expect("independently signed static test packet");
    let input = envelope(authorization, BINDING.trim());
    let output = run(&input, &[]);
    let actual = report(&output);
    assert_eq!(
        actual,
        serde_json::to_value(verified.report()).expect("SDK report")
    );
    assert_eq!(actual["status"], "signature-and-binding-verified");
    assert_eq!(
        actual["authorization_sha256"],
        Sha256Digest::of(authorization.as_bytes()).to_string()
    );
    assert_eq!(actual["observed_at"], binding.observed_at);
    assert_eq!(actual["target_count"], binding.ordered_targets.len());
    assert_eq!(actual["publisher_count"], binding.publishers.len());
    assert_eq!(actual.as_object().expect("report object").len(), 7);
    let public = String::from_utf8(output.stdout.clone()).expect("report UTF-8");
    for publisher in &binding.publishers {
        assert!(
            !public.contains(&publisher.public_key),
            "report must not expose public-key transport"
        );
    }
    for target in &binding.ordered_targets {
        assert!(
            !public.contains(&target.remote),
            "report must not expose target coordinates"
        );
    }
    assert_eq!(
        run(&input, &[]).stdout,
        output.stdout,
        "no ambient time or cwd identity"
    );

    let pretty = serde_json::to_string_pretty(
        &serde_json::from_str::<Value>(authorization).expect("fixture JSON"),
    )
    .expect("alternate valid transport");
    let alternate = report(&run(&envelope(&pretty, BINDING), &[]));
    assert_eq!(
        alternate["canonical_body_sha256"],
        actual["canonical_body_sha256"]
    );
    assert_eq!(alternate["binding_sha256"], actual["binding_sha256"]);
    assert_eq!(
        alternate["authorization_sha256"],
        Sha256Digest::of(pretty.as_bytes()).to_string()
    );
}

#[test]
fn complete_envelope_and_shared_verifier_ceilings_fail_without_partial_reports() {
    let input = envelope(AUTHORIZATION.trim(), BINDING.trim());
    let exact = input.len().to_string();
    assert!(run(&input, &["--max-bytes", &exact]).status.success());
    let smaller = (input.len() - 1).to_string();
    rejected(
        &run(&input, &["--max-bytes", &smaller]),
        "release_input_limit",
    );
    for option in ["--max-nodes", "--max-depth", "--max-targets"] {
        rejected(&run(&input, &[option, "0"]), "release_limit");
    }
    assert!(
        run(&input, &[]).status.success(),
        "limit refusal does not consume or poison the packet"
    );
}

#[test]
fn supplied_time_stop_and_binding_mismatches_are_redacted_native_verifier_failures() {
    let original: Value = serde_json::from_str(BINDING).expect("binding fixture");
    let authorization: Value = serde_json::from_str(AUTHORIZATION).expect("authorization fixture");
    for (field, replacement, code) in [
        ("emergency_stop", json!(true), "release_inactive"),
        (
            "observed_at",
            authorization["expires_at"].clone(),
            "release_time",
        ),
        (
            "coordinator_id",
            json!("PRIVATE_SENTINEL"),
            "release_binding",
        ),
    ] {
        let mut binding = original.clone();
        binding[field] = replacement;
        let binding = serde_json::to_string(&binding).expect("mutated test binding");
        rejected(&run(&envelope(AUTHORIZATION, &binding), &[]), code);
    }
    let mut corrupted = authorization;
    corrupted["publisher_authorizations"][0]["signature_envelope"]["signature"] =
        json!("PRIVATE_SENTINEL");
    rejected(
        &run(
            &envelope(
                &serde_json::to_string(&corrupted).expect("corrupt signature fixture"),
                BINDING,
            ),
            &[],
        ),
        "release_signature",
    );
}

#[test]
fn nested_objects_keep_duplicate_and_unknown_field_evidence_for_the_shared_decoder() {
    let authorization = AUTHORIZATION.trim();
    let binding = BINDING.trim();
    let duplicate_authorization = format!(
        "{{\"non\\u0063e\":\"PRIVATE_SENTINEL\",{}",
        &authorization[1..]
    );
    rejected(
        &run(&envelope(&duplicate_authorization, binding), &[]),
        "release_json",
    );
    let duplicate_binding = format!("{{\"observed_at\":0,{}", &binding[1..]);
    rejected(
        &run(&envelope(authorization, &duplicate_binding), &[]),
        "release_json",
    );
    let unknown = format!("{{\"PRIVATE_SENTINEL\":true,{}", &binding[1..]);
    rejected(
        &run(&envelope(authorization, &unknown), &[]),
        "release_shape",
    );
}
