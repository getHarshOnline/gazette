//! Real signatures at the pure release verifier, without issuer or effect authority.

#[path = "release_authorization/fixture.rs"]
mod fixture;

use base64::{Engine, engine::general_purpose::STANDARD};
use ed25519_dalek::Signer;
use gazette_compiler::release::{self, Binding, Error, KeyState, Limits};
use gazette_core::source::Sha256Digest;
use serde_json::{Value, json};

fn verify(bytes: &[u8], binding: &Binding) -> Result<release::VerifiedAuthorization, Error> {
    release::verify(bytes, binding, Limits::default())
}

fn failure(bytes: &[u8], binding: &Binding) -> Error {
    match verify(bytes, binding) {
        Ok(_) => panic!("unexpected verification evidence for refused input"),
        Err(error) => error,
    }
}

#[test]
fn independently_signed_frozen_fixture_matches_exact_canonical_bytes() {
    // The golden was signed using Node v26.3.0's native Ed25519 implementation, not
    // this verifier or the mutable Dalek fixture issuer. The body member order
    // was specified explicitly for this ASCII-key, integral-number fixture.
    let authorization = include_bytes!("release_authorization/authorization.json");
    let binding = release::parse_binding(
        include_bytes!("release_authorization/binding.json"),
        Limits::default(),
    )
    .expect("complete independently pinned fixture binding");
    let verified = verify(authorization, &binding).expect("independent Ed25519 signature verifies");
    let expected = include_str!("release_authorization/canonical_body.json").trim_end_matches('\n');
    assert_eq!(verified.canonical_body(), expected.as_bytes());
    assert_eq!(
        verified.report().canonical_body_sha256.to_string(),
        "2fcc02ab3b16e87f4338fd77c70c8ff35aa245563515f631518c08337c52fe81"
    );
    assert_eq!(
        verified.report().authorization_sha256,
        Sha256Digest::of(authorization)
    );
    let body: Value = serde_json::from_slice(verified.canonical_body())
        .expect("exact canonical body remains JSON");
    assert!(body.get("canonical_body_sha256").is_none());
    assert!(
        body["publisher_authorizations"][0]
            .get("signature_envelope")
            .is_none()
    );
    assert_eq!(
        body["publisher_authorizations"][0]["authorized_target_ordinals"],
        json!([0])
    );
    assert_eq!(body["nonce"], "nonce:é😀");
}

#[test]
fn real_disjoint_signatures_bind_all_targets_and_report_exact_transport_and_authority_inputs() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let verified = verify(&bytes, &binding).expect("two real signatures over one complete batch");
    assert_eq!(verified.authorization().ordered_targets.len(), 3);
    assert_eq!(
        verified.authorization().publisher_authorizations[0].authorized_target_ordinals,
        [0, 2]
    );
    assert_eq!(
        verified.authorization().publisher_authorizations[1].authorized_target_ordinals,
        [1]
    );
    assert_eq!(
        verified.canonical_body(),
        fixture::body(
            &serde_json::from_slice(&bytes)
                .expect("valid fixture structure and exact serialization")
        )
    );
    let report = serde_json::to_value(verified.report())
        .expect("valid fixture structure and exact serialization");
    assert_eq!(
        report,
        json!({
            "status":"signature-and-binding-verified", "authorization_sha256":Sha256Digest::of(&bytes),
            "canonical_body_sha256":binding.canonical_body_sha256,
            "binding_sha256":Sha256Digest::of(&serde_jcs::to_vec(&binding).expect("valid fixture structure and exact serialization")),
            "observed_at":120, "target_count":3, "publisher_count":2
        })
    );
    let parsed = release::parse_binding(
        &serde_json::to_vec(&binding).expect("valid fixture structure and exact serialization"),
        Limits::default(),
    )
    .expect("valid fixture structure and exact serialization");
    assert_eq!(parsed, binding);
    // A pure repeat remains valid; it neither consumes a nonce nor claims replay prevention.
    assert_eq!(
        verify(&bytes, &binding)
            .expect("valid fixture structure and exact serialization")
            .report()
            .canonical_body_sha256,
        verified.report().canonical_body_sha256
    );
    let mut changed_frontier = binding.clone();
    changed_frontier.revocation_frontier_sha256 =
        Sha256Digest::of(b"fresh independent observation");
    let later =
        verify(&bytes, &changed_frontier).expect("valid fixture structure and exact serialization");
    assert_eq!(later.canonical_body(), verified.canonical_body());
    assert_ne!(
        later.report().binding_sha256,
        verified.report().binding_sha256
    );
}

#[test]
fn transport_formatting_and_equivalent_escapes_preserve_signed_meaning_not_received_byte_identity()
{
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let value: Value =
        serde_json::from_slice(&bytes).expect("valid fixture structure and exact serialization");
    let alternate = serde_json::to_string_pretty(&value)
        .expect("valid fixture structure and exact serialization")
        .replace("é😀", "\\u00e9\\ud83d\\ude00");
    let first = verify(&bytes, &binding).expect("valid fixture structure and exact serialization");
    let second = verify(alternate.as_bytes(), &binding)
        .expect("valid fixture structure and exact serialization");
    assert_eq!(first.canonical_body(), second.canonical_body());
    assert_eq!(
        first.report().binding_sha256,
        second.report().binding_sha256
    );
    assert_ne!(
        first.report().authorization_sha256,
        second.report().authorization_sha256
    );
    let mut decomposed = value;
    decomposed["nonce"] = json!("nonce:e\u{301}😀");
    assert_eq!(
        failure(
            &serde_json::to_vec(&decomposed)
                .expect("valid fixture structure and exact serialization"),
            &binding
        ),
        Error::Digest
    );
}

#[test]
fn freshly_signed_changed_actions_cannot_replace_independently_retained_expectations() {
    let original = fixture::declaration();
    let (_, retained) = fixture::sign(original.clone());
    let cases = [
        ("/coordinator_id", json!("coordinator:other")),
        ("/journal_id/0", json!(8)),
        ("/journal_epoch", json!(2)),
        ("/writer_fence", json!(10)),
        ("/ordered_targets/0/domain", json!("changed.example")),
        (
            "/ordered_targets/0/remote",
            json!("https://example.invalid/other.git"),
        ),
        ("/ordered_targets/0/branch", json!("other")),
        (
            "/ordered_targets/0/refspec",
            json!("aa".repeat(20) + ":refs/heads/other"),
        ),
        (
            "/ordered_targets/0/local_commit_oid",
            json!("cc".repeat(20)),
        ),
        (
            "/ordered_targets/0/expected_remote_oid_or_unborn",
            json!({"state":"commit","oid":"dd".repeat(20)}),
        ),
        (
            "/ordered_targets/0/prepared_release_receipt_ref",
            json!("55".repeat(32)),
        ),
        ("/ordered_targets/0/output_digest", json!("66".repeat(32))),
    ];
    for (path, value) in cases {
        let mut changed = original.clone();
        *changed
            .pointer_mut(path)
            .expect("valid fixture structure and exact serialization") = value;
        let (bytes, claimed) = fixture::sign(changed);
        let mut expected = retained.clone();
        // Even independently accepting the new body digest cannot change retained destinations.
        expected.canonical_body_sha256 = claimed.canonical_body_sha256;
        assert_eq!(failure(&bytes, &expected), Error::Binding, "{path}");
    }
    for field in [
        "validation_evidence_digest",
        "disclosure_evidence_digest",
        "policy_evidence_digest",
        "generic_rule_set_digest",
        "website_policy_projection_digest",
        "scanner_revision",
        "record_id",
        "nonce",
    ] {
        let mut changed = original.clone();
        changed[field] = if field.ends_with("digest") {
            json!("77".repeat(32))
        } else {
            json!("changed")
        };
        let (bytes, _) = fixture::sign(changed);
        assert_eq!(
            failure(&bytes, &retained),
            Error::Digest,
            "independent reviewed body binds {field}"
        );
    }
    let mut reordered = original;
    reordered["ordered_targets"]
        .as_array_mut()
        .expect("valid fixture structure and exact serialization")
        .swap(0, 1);
    let (bytes, _) = fixture::sign(reordered);
    assert_eq!(failure(&bytes, &retained), Error::Digest);
}

#[test]
fn complete_publisher_coverage_is_disjoint_ordered_and_bound_to_exact_admitted_keys() {
    let original = fixture::declaration();
    for (first, second) in [
        (json!([0]), json!([1])),
        (json!([0, 2]), json!([0, 1])),
        (json!([2, 0]), json!([1])),
        (json!([0, 0, 2]), json!([1])),
        (json!([]), json!([1])),
        (json!([0, 3]), json!([1])),
    ] {
        let mut changed = original.clone();
        changed["publisher_authorizations"][0]["authorized_target_ordinals"] = first;
        changed["publisher_authorizations"][1]["authorized_target_ordinals"] = second;
        let (bytes, binding) = fixture::sign(changed);
        assert_eq!(failure(&bytes, &binding), Error::Scope);
    }
    let (bytes, binding) = fixture::sign(original.clone());
    for path in [
        "publisher_id",
        "publisher_key_id",
        "publisher_key_epoch",
        "authority_reference",
        "authorized_target_ordinals",
    ] {
        let mut value = serde_json::to_value(&binding)
            .expect("valid fixture structure and exact serialization");
        value["publishers"][0][path] = match path {
            "publisher_key_epoch" => json!(2),
            "authority_reference" => json!("33".repeat(32)),
            "authorized_target_ordinals" => json!([1]),
            _ => json!("not-admitted"),
        };
        let changed: Binding =
            serde_json::from_value(value).expect("valid fixture structure and exact serialization");
        assert_eq!(failure(&bytes, &changed), Error::Scope, "{path}");
    }
    let mut swapped = original;
    swapped["publisher_authorizations"][0]["authorized_target_ordinals"] = json!([1]);
    swapped["publisher_authorizations"][1]["authorized_target_ordinals"] = json!([0, 2]);
    let (bytes, new_claim) = fixture::sign(swapped);
    let mut retained = binding;
    retained.canonical_body_sha256 = new_claim.canonical_body_sha256;
    assert_eq!(
        failure(&bytes, &retained),
        Error::Scope,
        "valid signatures cannot exchange native publisher scopes"
    );
}

#[test]
fn observation_time_key_lifecycle_and_emergency_stop_are_rechecked_on_every_call() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    for (observed, valid) in [
        (99, false),
        (109, false),
        (110, true),
        (199, true),
        (200, false),
        (201, false),
    ] {
        let mut current = binding.clone();
        current.observed_at = observed;
        if valid {
            verify(&bytes, &current).expect("inside exact half-open window");
        } else {
            assert_eq!(failure(&bytes, &current), Error::Time);
        }
    }
    for state in [KeyState::Revoked, KeyState::Superseded] {
        let mut current = binding.clone();
        current.publishers[0].state = state;
        assert_eq!(failure(&bytes, &current), Error::Inactive);
    }
    let mut stopped = binding.clone();
    stopped.emergency_stop = true;
    assert_eq!(failure(&bytes, &stopped), Error::Inactive);
    for (field, value) in [
        ("not_before", 200),
        ("approved_at", 121),
        ("expires_at", 110),
    ] {
        let mut changed = fixture::declaration();
        changed[field] = json!(value);
        let (bytes, binding) = fixture::sign(changed);
        assert_eq!(failure(&bytes, &binding), Error::Time, "{field}");
    }
}

#[test]
fn strict_ed25519_rejects_wrong_domains_weak_points_and_noncanonical_transport() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let original: Value =
        serde_json::from_slice(&bytes).expect("valid fixture structure and exact serialization");
    for message in [
        binding.canonical_body_sha256.as_bytes().to_vec(),
        [
            fixture::DOMAIN,
            binding.canonical_body_sha256.to_string().as_bytes(),
        ]
        .concat(),
        [
            b"OTHER_RECORD\0".as_slice(),
            binding.canonical_body_sha256.as_bytes(),
        ]
        .concat(),
    ] {
        let mut changed = original.clone();
        changed["publisher_authorizations"][0]["signature_envelope"]["signature"] =
            json!(STANDARD.encode(fixture::key("publisher:one").sign(&message).to_bytes()));
        assert_eq!(
            failure(
                &serde_json::to_vec(&changed)
                    .expect("valid fixture structure and exact serialization"),
                &binding
            ),
            Error::Signature
        );
    }
    for signature in [
        STANDARD.encode([0; 64]),
        STANDARD.encode([255; 64]),
        STANDARD.encode([0; 63]),
        "not base64".into(),
        original["publisher_authorizations"][0]["signature_envelope"]["signature"]
            .as_str()
            .expect("valid fixture structure and exact serialization")
            .trim_end_matches('=')
            .into(),
    ] {
        let mut changed = original.clone();
        changed["publisher_authorizations"][0]["signature_envelope"]["signature"] =
            json!(signature);
        assert_eq!(
            failure(
                &serde_json::to_vec(&changed)
                    .expect("valid fixture structure and exact serialization"),
                &binding
            ),
            Error::Signature
        );
    }
    let mut identity = [0; 32];
    identity[0] = 1;
    for key in [
        STANDARD.encode(identity),
        STANDARD.encode([0; 32]),
        STANDARD.encode([0; 31]),
        "invalid".into(),
    ] {
        let mut changed = binding.clone();
        changed.publishers[0].public_key = key;
        assert_eq!(failure(&bytes, &changed), Error::Key);
    }
    let mut wrong = binding;
    wrong.publishers[0].public_key =
        STANDARD.encode(fixture::key("publisher:two").verifying_key().to_bytes());
    assert_eq!(failure(&bytes, &wrong), Error::Signature);
}

#[test]
fn exact_action_validation_rejects_force_deletion_wildcards_zero_oids_and_duplicate_destinations() {
    for (path, value) in [
        ("/ordered_targets/0/refspec", json!("+HEAD:refs/heads/main")),
        ("/ordered_targets/0/refspec", json!(":refs/heads/main")),
        (
            "/ordered_targets/0/refspec",
            json!("refs/heads/*:refs/heads/*"),
        ),
        ("/ordered_targets/0/branch", json!("../main")),
        ("/ordered_targets/0/local_commit_oid", json!("0".repeat(40))),
        (
            "/ordered_targets/0/expected_remote_oid_or_unborn",
            json!({"state":"commit","oid":"0".repeat(40)}),
        ),
        ("/ordered_targets/1/domain", json!("0.example")),
        (
            "/ordered_targets/1/remote",
            json!("https://example.invalid/site-0.git"),
        ),
        ("/ordered_targets/1/ordinal", json!(2)),
    ] {
        let mut declaration = fixture::declaration();
        *declaration
            .pointer_mut(path)
            .expect("valid fixture structure and exact serialization") = value;
        if path == "/ordered_targets/0/local_commit_oid" {
            declaration["ordered_targets"][0]["refspec"] =
                json!("0".repeat(40) + ":refs/heads/main");
        }
        let (bytes, binding) = fixture::sign(declaration);
        assert_eq!(failure(&bytes, &binding), Error::Target, "{path}");
    }
}

#[test]
fn signed_target_destination_uses_the_shared_literal_grammar_without_normalization() {
    for (remote, branch, valid) in [
        (
            "git@EXAMPLE.invalid:Exact Repository.git",
            "publication/é",
            true,
        ),
        ("/not-opened/repository", "release/v1", true),
        ("origin", "HEAD", true),
        (" origin", "main", false),
        ("origin\nPRIVATE", "main", false),
        ("origin", "-main", false),
        ("origin", "a//b", false),
        ("origin", "a.lock", false),
        ("origin", "a@{b", false),
        ("origin", "@", false),
        ("origin", "a:b", false),
        ("origin", "a b", false),
        ("origin", "a\\b", false),
    ] {
        assert_eq!(
            gazette_core::release::validate_git_destination(remote, branch).is_ok(),
            valid
        );
        let mut declaration = fixture::declaration();
        let target = &mut declaration["ordered_targets"][0];
        target["remote"] = json!(remote);
        target["branch"] = json!(branch);
        target["refspec"] = json!(format!(
            "{}:refs/heads/{branch}",
            target["local_commit_oid"].as_str().expect("fixture commit")
        ));
        let (bytes, binding) = fixture::sign(declaration);
        if valid {
            let verified =
                verify(&bytes, &binding).expect("real signature and existing literal coordinates");
            let target = &verified.authorization().ordered_targets[0];
            assert_eq!(target.remote, remote);
            assert_eq!(target.branch, branch);
        } else {
            assert_eq!(failure(&bytes, &binding), Error::Target);
        }
    }
}

#[test]
fn zero_journal_coordinates_fail_even_when_independent_assertions_match() {
    let mut declaration = fixture::declaration();
    declaration["journal_epoch"] = json!(0);
    let (bytes, binding) = fixture::sign(declaration);
    assert_eq!(failure(&bytes, &binding), Error::Binding);
    let (bytes, mut binding) = fixture::sign(fixture::declaration());
    binding.journal.schema_version = 0;
    assert_eq!(failure(&bytes, &binding), Error::Binding);
}

#[test]
fn strict_wire_shapes_duplicates_and_lossy_numbers_fail_before_verification() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let text =
        String::from_utf8(bytes.clone()).expect("valid fixture structure and exact serialization");
    for token in [
        "1.0",
        "1e0",
        "-0",
        "-1",
        "9007199254740992",
        "18446744073709551617",
        "1e400",
    ] {
        let changed = text.replacen(
            "\"journal_epoch\":1",
            &format!("\"journal_epoch\":{token}"),
            1,
        );
        assert_ne!(changed, text);
        assert_eq!(
            failure(changed.as_bytes(), &binding),
            Error::Number,
            "{token}"
        );
    }
    for changed in [
        format!("{text} {{}}"),
        text.replacen("\"nonce\":", "\"nonce\":\"first\",\"n\\u006fnce\":", 1),
        text.replacen("\"ordinal\":0", "\"ordinal\":0,\"ordin\\u0061l\":0", 1),
    ] {
        assert_eq!(failure(changed.as_bytes(), &binding), Error::Json);
    }
    let original: Value =
        serde_json::from_slice(&bytes).expect("valid fixture structure and exact serialization");
    for path in [
        "",
        "/ordered_targets/0",
        "/publisher_authorizations/0",
        "/publisher_authorizations/0/signature_envelope",
        "/ordered_targets/0/expected_remote_oid_or_unborn",
    ] {
        for replacement in [Value::Null, json!([])] {
            let mut changed = original.clone();
            if path.is_empty() {
                changed = replacement;
            } else {
                *changed
                    .pointer_mut(path)
                    .expect("valid fixture structure and exact serialization") = replacement;
            }
            assert_eq!(
                failure(
                    &serde_json::to_vec(&changed)
                        .expect("valid fixture structure and exact serialization"),
                    &binding
                ),
                Error::Shape,
                "{path}"
            );
        }
        let mut changed = original.clone();
        let object = if path.is_empty() {
            changed
                .as_object_mut()
                .expect("valid fixture structure and exact serialization")
        } else {
            changed
                .pointer_mut(path)
                .expect("valid fixture structure and exact serialization")
                .as_object_mut()
                .expect("valid fixture structure and exact serialization")
        };
        object.insert("unknown_critical".into(), json!(true));
        assert_eq!(
            failure(
                &serde_json::to_vec(&changed)
                    .expect("valid fixture structure and exact serialization"),
                &binding
            ),
            Error::Shape,
            "{path}"
        );
    }
    for field in ["nonce", "canonical_body_sha256", "publisher_authorizations"] {
        let mut changed = original.clone();
        changed
            .as_object_mut()
            .expect("valid fixture structure and exact serialization")
            .remove(field);
        assert_eq!(
            failure(
                &serde_json::to_vec(&changed)
                    .expect("valid fixture structure and exact serialization"),
                &binding
            ),
            Error::Shape
        );
    }
}

#[test]
fn correct_length_positional_arrays_cannot_replace_required_named_objects() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let authorization: Value = serde_json::from_slice(&bytes).expect("signed fixture transport");
    let positional = |object: &Value, fields: &str| -> Value {
        Value::Array(
            fields
                .split_whitespace()
                .map(|field| {
                    object
                        .get(field)
                        .expect("each positional fixture member exists")
                        .clone()
                })
                .collect(),
        )
    };
    for (path, fields) in [
        (
            "",
            "schema_id record_id nonce coordinator_id journal_id journal_epoch writer_fence ordered_targets validation_evidence_digest disclosure_evidence_digest policy_evidence_digest scanner_revision generic_rule_set_digest website_policy_projection_digest approved_at not_before expires_at canonical_body_sha256 publisher_authorizations",
        ),
        (
            "/ordered_targets/0",
            "ordinal domain remote branch refspec expected_remote_oid_or_unborn prepared_release_receipt_ref local_commit_oid output_digest",
        ),
        (
            "/publisher_authorizations/0",
            "publisher_id publisher_key_id publisher_key_epoch authority_reference authorized_target_ordinals signature_envelope",
        ),
        (
            "/publisher_authorizations/0/signature_envelope",
            "schema_id signature",
        ),
    ] {
        let mut changed = authorization.clone();
        let object = if path.is_empty() {
            &authorization
        } else {
            authorization.pointer(path).expect("nested fixture object")
        };
        let array = positional(object, fields);
        if path.is_empty() {
            changed = array;
        } else {
            *changed.pointer_mut(path).expect("exact nested coordinate") = array;
        }
        assert_eq!(
            failure(
                &serde_json::to_vec(&changed).expect("valid positional JSON"),
                &binding
            ),
            Error::Shape,
            "{path}"
        );
    }
    let original = serde_json::to_value(&binding).expect("complete binding");
    for (path, fields) in [
        (
            "",
            "canonical_body_sha256 coordinator_id journal ordered_targets publishers authority_frontier_sha256 revocation_frontier_sha256 supersession_frontier_sha256 observed_at emergency_stop",
        ),
        (
            "/journal",
            "schema_version journal_id journal_epoch writer_fence",
        ),
        (
            "/publishers/0",
            "publisher_id publisher_key_id publisher_key_epoch authority_reference role authorized_target_ordinals public_key state",
        ),
    ] {
        let mut changed = original.clone();
        let object = if path.is_empty() {
            &original
        } else {
            original.pointer(path).expect("binding object")
        };
        let array = positional(object, fields);
        if path.is_empty() {
            changed = array;
        } else {
            *changed.pointer_mut(path).expect("binding coordinate") = array;
        }
        assert_eq!(
            release::parse_binding(
                &serde_json::to_vec(&changed).expect("positional JSON"),
                Limits::default()
            )
            .expect_err("positional binding denied"),
            Error::Shape,
            "{path}"
        );
    }
}

#[test]
fn independent_binding_decoder_has_the_same_strict_shape_numeric_and_resource_boundary() {
    let (bytes, binding) = fixture::sign(fixture::declaration());
    let original =
        serde_json::to_value(&binding).expect("valid fixture structure and exact serialization");
    for path in ["", "/journal", "/ordered_targets/0", "/publishers/0"] {
        let mut value = original.clone();
        if path.is_empty() {
            value = json!([])
        } else {
            *value
                .pointer_mut(path)
                .expect("valid fixture structure and exact serialization") = json!([])
        }
        assert_eq!(
            release::parse_binding(
                &serde_json::to_vec(&value)
                    .expect("valid fixture structure and exact serialization"),
                Limits::default()
            )
            .expect_err("malformed independent binding must be refused"),
            Error::Shape
        );
    }
    let text =
        serde_json::to_string(&binding).expect("valid fixture structure and exact serialization");
    for token in ["120.0", "120e0", "9007199254740992"] {
        let changed = text.replace("\"observed_at\":120", &format!("\"observed_at\":{token}"));
        assert_eq!(
            release::parse_binding(changed.as_bytes(), Limits::default())
                .expect_err("malformed independent binding must be refused"),
            Error::Number
        );
    }
    let duplicate = text.replace(
        "\"emergency_stop\":false",
        "\"emergency_stop\":false,\"emergency_st\\u006fp\":false",
    );
    assert_eq!(
        release::parse_binding(duplicate.as_bytes(), Limits::default())
            .expect_err("malformed independent binding must be refused"),
        Error::Json
    );
    let mut safe = binding.clone();
    safe.observed_at = 9_007_199_254_740_991;
    assert_eq!(
        release::parse_binding(
            &serde_json::to_vec(&safe).expect("valid fixture structure and exact serialization"),
            Limits::default()
        )
        .expect("valid fixture structure and exact serialization"),
        safe
    );
    for limits in [
        Limits {
            bytes: bytes.len() - 1,
            ..Limits::default()
        },
        Limits {
            nodes: 1,
            ..Limits::default()
        },
        Limits {
            depth: 1,
            ..Limits::default()
        },
        Limits {
            targets: 2,
            ..Limits::default()
        },
    ] {
        match release::verify(&bytes, &binding, limits) {
            Err(error) => assert_eq!(error, Error::Limit),
            Ok(_) => panic!("limit returned verification evidence"),
        }
    }
    let bound = bytes.len().max(
        serde_json::to_vec(&binding)
            .expect("valid fixture structure and exact serialization")
            .len(),
    );
    release::verify(
        &bytes,
        &binding,
        Limits {
            bytes: bound,
            ..Limits::default()
        },
    )
    .expect("exact larger complete input fits");
}
