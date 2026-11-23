//! Synthetic response fixtures exercise matching, never policy authority or approval.

use super::{Fixture, acquire, acquisition_limits, review_cli, review_fixture};
use gazette_sdk::{build, source};
use serde_json::{Value, json};
use std::fs;

fn native_review() -> (Fixture, Fixture, String, build::PublicationReview) {
    let (one, other, configuration) = review_fixture();
    let inputs = acquire(&configuration, acquisition_limits()).expect("real native capture");
    let parsed = inputs
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same semantic owner");
    let review = resolved
        .review(
            Default::default(),
            &gazette_sdk::raster::inspect,
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete review request");
    (one, other, configuration, review)
}

fn policy() -> build::ReviewPolicy {
    build::ReviewPolicy {
        id: "policy:fixture".parse().expect("policy identity"),
        revision: "revision:fixture-v1".parse().expect("policy revision"),
        source_sha256: source::Sha256Digest::of(b"synthetic test policy bytes"),
    }
}

fn binding(bytes: &[u8]) -> build::ReviewResponseBinding {
    build::ReviewResponseBinding {
        policy: policy(),
        response_sha256: source::Sha256Digest::of(bytes),
    }
}

fn response(review: &build::PublicationReview) -> Value {
    let mut decisions = Vec::new();
    for (surface, projection) in review.request().surfaces().iter().enumerate() {
        for usage in projection.uses() {
            let outcome = ["allow", "hold", "exclude"][decisions.len() % 3];
            decisions.push(json!({
                "surface":surface,
                "pointer":usage.pointer,
                "outcome":outcome,
                "evidence":["evidence:fixture", "evidence:second"]
            }));
        }
    }
    // Coverage is canonical; response order is the external responder's exact intent.
    decisions.reverse();
    json!({
        "format":"gazette-review-response/1",
        "request_sha256":review.sha256(),
        "policy":policy(),
        "decisions":decisions
    })
}

fn reject(review: &build::PublicationReview, response: &Value, reason: &str) -> &'static str {
    let bytes = serde_json::to_vec(response).expect("synthetic response encoding");
    review
        .match_response(&bytes, &binding(&bytes), Default::default())
        .expect_err(&format!(
            "must reject {reason} even when its exact bytes are independently bound"
        ))
        .code()
}

#[test]
fn real_cli_review_matches_complete_mixed_response_without_granting_authority() {
    let (one, other, configuration, review) = native_review();
    let cli = review_cli(&configuration, &[]);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&cli.stdout).expect("real CLI review"),
        serde_json::to_value(&review).expect("retained SDK review")
    );
    let response = response(&review);
    let bytes = serde_json::to_vec(&response).expect("synthetic mixed response");
    let expected_binding = binding(&bytes);
    let matched = review
        .match_response(&bytes, &expected_binding, Default::default())
        .expect("exact complete matching is not approval");
    assert_eq!(matched.request_sha256(), review.sha256());
    assert_eq!(matched.response_json().as_bytes(), bytes);
    assert_eq!(
        serde_json::to_value(matched.decisions()).expect("response decisions"),
        response["decisions"]
    );
    let report = serde_json::to_value(&matched).expect("matched report");
    assert_eq!(
        report,
        json!({
            "status":"matched-unverified",
            "request_sha256":review.sha256(),
            "response_sha256":source::Sha256Digest::of(&bytes),
            "policy":policy(),
            "decisions":response["decisions"],
            "response_json":std::str::from_utf8(&bytes).expect("original valid JSON")
        })
    );
    for outcome in ["allow", "hold", "exclude"] {
        assert!(
            report["decisions"]
                .as_array()
                .expect("decisions")
                .iter()
                .any(|decision| decision["outcome"] == outcome)
        );
    }
    let mut all_allow = response.clone();
    for decision in all_allow["decisions"]
        .as_array_mut()
        .expect("complete decisions")
    {
        decision["outcome"] = json!("allow");
    }
    let all_allow = serde_json::to_vec(&all_allow).expect("all-allow response");
    let all_allow = review
        .match_response(&all_allow, &binding(&all_allow), Default::default())
        .expect("all-allow is still only matching");
    assert_eq!(
        serde_json::to_value(all_allow).expect("all-allow match report")["status"],
        "matched-unverified"
    );
    let before = [&one, &other].map(|fixture| {
        source::inspect(
            &fixture.directory.path().join("selected"),
            None,
            source::Limits::default(),
        )
        .expect("native inputs remain unchanged")
        .identity()
    });
    assert_eq!(
        serde_json::to_value(
            review
                .match_response(&bytes, &expected_binding, Default::default())
                .expect("repeat exact match")
        )
        .expect("repeat report"),
        report
    );
    for (fixture, identity) in [&one, &other].into_iter().zip(before) {
        assert_eq!(
            source::inspect(
                &fixture.directory.path().join("selected"),
                None,
                source::Limits::default()
            )
            .expect("matching does not mutate source")
            .identity(),
            identity
        );
    }
    fs::write(
        one.directory.path().join("selected/page.md"),
        b"changed after request capture",
    )
    .expect("isolated source edit");
    assert_eq!(
        serde_json::to_value(
            review
                .match_response(&bytes, &expected_binding, Default::default())
                .expect("retained request is unaffected by later native bytes")
        )
        .expect("retained report"),
        report
    );
    assert!(acquire(&configuration, acquisition_limits()).is_err());
}

#[test]
fn matching_requires_independent_exact_request_policy_and_response_byte_bindings() {
    let (_one, _other, _configuration, review) = native_review();
    let original = response(&review);
    let bytes = serde_json::to_vec(&original).expect("response");
    let mut changed = original.clone();
    changed["request_sha256"] = json!(source::Sha256Digest::of(b"another request"));
    assert_eq!(
        reject(&review, &changed, "another retained request"),
        "publication_review_response_binding"
    );
    for (field, value) in [
        ("id", json!("policy:other")),
        ("revision", json!("revision:other")),
        (
            "source_sha256",
            json!(source::Sha256Digest::of(b"another policy source")),
        ),
    ] {
        let mut changed = original.clone();
        changed["policy"][field] = value;
        assert_eq!(
            reject(&review, &changed, field),
            "publication_review_response_binding"
        );
    }
    let mut wrong_binding = binding(&bytes);
    wrong_binding.policy.id = "policy:another-expected-owner"
        .parse()
        .expect("different policy");
    assert!(
        review
            .match_response(&bytes, &wrong_binding, Default::default())
            .is_err()
    );
    let mut spaced = bytes.clone();
    spaced.push(b'\n');
    assert!(
        review
            .match_response(&spaced, &binding(&bytes), Default::default())
            .is_err(),
        "semantic equality cannot replace independently bound response bytes"
    );
    let rematched = review
        .match_response(&spaced, &binding(&spaced), Default::default())
        .expect("explicitly rebound exact whitespace is valid JSON");
    assert_eq!(rematched.response_json().as_bytes(), spaced);
    assert_eq!(rematched.request_sha256(), review.sha256());
    assert_eq!(
        serde_json::to_value(rematched.decisions()).expect("same ordered decisions"),
        original["decisions"]
    );
    assert_eq!(
        serde_json::to_value(rematched).expect("new exact byte identity")["response_sha256"],
        json!(source::Sha256Digest::of(&spaced))
    );
}

#[test]
fn matching_requires_every_exact_surface_use_without_aliases_or_prefix_coverage() {
    let (_one, _other, _configuration, review) = native_review();
    let original = response(&review);
    let decisions = original["decisions"]
        .as_array()
        .expect("complete decisions");
    let mut duplicate = original.clone();
    duplicate["decisions"]
        .as_array_mut()
        .expect("decisions")
        .push(decisions[0].clone());
    assert_eq!(
        reject(&review, &duplicate, "duplicate exact occurrence"),
        "publication_review_response_duplicate"
    );
    let mut missing = original.clone();
    missing["decisions"]
        .as_array_mut()
        .expect("decisions")
        .pop();
    assert_eq!(
        reject(&review, &missing, "missing occurrence"),
        "publication_review_response_coverage"
    );
    let root = decisions
        .iter()
        .position(|item| item["pointer"] == "")
        .expect("root use");
    let mut missing_root = original.clone();
    missing_root["decisions"]
        .as_array_mut()
        .expect("decisions")
        .remove(root);
    reject(
        &review,
        &missing_root,
        "missing container despite all of its children",
    );
    let mut root_only = original.clone();
    root_only["decisions"] = json!([decisions[root]]);
    reject(
        &review,
        &root_only,
        "root or prefix decision cannot cover descendants",
    );
    let array_use = decisions
        .iter()
        .position(|item| {
            item["pointer"]
                .as_str()
                .is_some_and(|pointer| pointer.contains("/0/"))
        })
        .expect("actual array occurrence");
    let original_pointer = decisions[array_use]["pointer"]
        .as_str()
        .expect("array pointer");
    for pointer in [
        original_pointer.replacen("/0/", "/00/", 1),
        original_pointer.replacen("/0/", "/-/", 1),
        format!("#{original_pointer}"),
        "/*".to_owned(),
        "/not-a-retained-field".to_owned(),
    ] {
        let mut changed = original.clone();
        changed["decisions"][array_use]["pointer"] = json!(pointer);
        reject(
            &review,
            &changed,
            "noncanonical, wildcard or absent use pointer",
        );
    }
    let escaped = decisions
        .iter()
        .position(|item| {
            item["pointer"]
                .as_str()
                .is_some_and(|pointer| pointer.contains("content:foreign~0~1one"))
        })
        .expect("escaped native key");
    let mut malformed = original.clone();
    malformed["decisions"][escaped]["pointer"] = json!(
        decisions[escaped]["pointer"]
            .as_str()
            .expect("escaped pointer")
            .replace("content:foreign~0~1one", "content:foreign~/one")
    );
    reject(&review, &malformed, "malformed RFC 6901 escape");
    let mut extra_surface = original.clone();
    extra_surface["decisions"][0]["surface"] = json!(review.request().surfaces().len());
    reject(
        &review,
        &extra_surface,
        "extra surface outside the exact inventory",
    );
    let mut wrong_surface = original.clone();
    wrong_surface["decisions"][array_use]["surface"] = json!(0);
    reject(
        &review,
        &wrong_surface,
        "pointer attached to another occurrence context",
    );
}

#[test]
fn response_shape_and_evidence_are_strict_even_with_a_matching_byte_digest() {
    let (_one, _other, _configuration, review) = native_review();
    let original = response(&review);
    let policy = &original["policy"];
    let decision = &original["decisions"][0];
    let root_array = json!([
        original["format"],
        original["request_sha256"],
        policy,
        original["decisions"]
    ]);
    assert_eq!(
        reject(&review, &root_array, "positional root"),
        "publication_review_response_json"
    );
    for (path, value) in [
        (
            "/policy",
            json!([policy["id"], policy["revision"], policy["source_sha256"]]),
        ),
        (
            "/decisions/0",
            json!([
                decision["surface"],
                decision["pointer"],
                decision["outcome"],
                decision["evidence"]
            ]),
        ),
        ("/decisions/0/outcome", json!({"allow":null})),
        ("/decisions/0/outcome", json!({"hold":null})),
        ("/decisions/0/outcome", json!({"exclude":null})),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).expect("existing named boundary") = value;
        assert_eq!(
            reject(&review, &changed, path),
            "publication_review_response_json"
        );
    }
    for (path, value) in [
        ("/format", json!("gazette-review-response/2")),
        ("/decisions/0/outcome", json!("approved")),
        ("/decisions/0/evidence", json!([])),
        (
            "/decisions/0/evidence",
            json!(["evidence:duplicate", "evidence:duplicate"]),
        ),
        ("/decisions/0/evidence", json!(["not-namespaced"])),
        ("/decisions/0/evidence", Value::Null),
        ("/decisions/0/surface", json!(-1)),
        ("/decisions/0/surface", json!(0.5)),
        ("/decisions/0/pointer", Value::Null),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(path).expect("known fixture field") = value;
        reject(&review, &changed, path);
    }
    for path in ["", "/policy", "/decisions/0"] {
        let mut changed = original.clone();
        changed
            .pointer_mut(path)
            .expect("known object")
            .as_object_mut()
            .expect("object")
            .insert("unexpected".into(), json!(true));
        reject(&review, &changed, "unknown field");
    }
    let mut missing = original.clone();
    missing["decisions"][0]
        .as_object_mut()
        .expect("decision")
        .remove("evidence");
    reject(&review, &missing, "absent evidence");
    let raw = serde_json::to_string(&original).expect("valid raw response");
    let first_surface = original["decisions"][0]["surface"]
        .as_u64()
        .expect("surface");
    for malformed in [
        raw.replacen(
            "\"format\":",
            "\"format\":\"gazette-review-response/1\",\"format\":",
            1,
        ),
        raw.replacen("\"policy\":{", "\"policy\":{\"id\":\"policy:fixture\",", 1),
        raw.replacen(
            "\"surface\":",
            &format!("\"surface\":{first_surface},\"surface\":"),
            1,
        ),
        format!(
            "{{\"\\u0066ormat\":\"gazette-review-response/1\",{}",
            &raw[1..]
        ),
        format!("{raw}{{}}"),
        raw.replacen(
            "\"outcome\":",
            "\"PRIVATE-RESPONSE-FIELD\":true,\"outcome\":",
            1,
        ),
        raw.replacen(
            "\"outcome\":",
            "\"outcome\":\"PRIVATE-RESPONSE-VALUE\",\"outcome\":",
            1,
        ),
    ] {
        let error = review
            .match_response(
                malformed.as_bytes(),
                &binding(malformed.as_bytes()),
                Default::default(),
            )
            .expect_err("strict JSON rejects duplicate/unknown members or trailing values");
        assert_eq!(error.code(), "publication_review_response_json");
        assert!(!error.to_string().contains("PRIVATE-RESPONSE"));
    }
    assert!(
        review
            .match_response(b"\xff", &binding(b"\xff"), Default::default())
            .is_err()
    );
}

#[test]
fn matched_response_owns_exact_original_text_after_request_and_submission_are_dropped() {
    let (matched, request_digest, original_text) = {
        let (_one, _other, _configuration, review) = native_review();
        let text = format!(
            " \r\n{}\t\n",
            serde_json::to_string_pretty(&response(&review)).expect("original formatted response")
        );
        let submission = build::ReviewSubmission {
            binding: binding(text.as_bytes()),
            response_json: text.clone(),
        };
        let transport = serde_json::to_value(&submission).expect("typed private transport");
        let restored: build::ReviewSubmission =
            serde_json::from_value(transport).expect("restore exact response string");
        assert_eq!(restored.binding, submission.binding);
        assert_eq!(restored.response_json, text);
        let matched = review
            .match_response(
                restored.response_json.as_bytes(),
                &restored.binding,
                Default::default(),
            )
            .expect("match through the existing boundary");
        (matched, review.sha256(), text)
    };
    assert_eq!(matched.request_sha256(), request_digest);
    assert_eq!(matched.response_json(), original_text);
    let retained = serde_json::to_value(&matched).expect("owned private evidence");
    assert_eq!(retained["response_json"], original_text);
    assert_eq!(
        retained["response_sha256"],
        json!(source::Sha256Digest::of(original_text.as_bytes()))
    );
    assert_eq!(retained["status"], "matched-unverified");
    assert!(!matched.decisions().is_empty());
}

#[test]
fn response_byte_and_decision_limits_accept_exact_bounds_without_truncation() {
    let (_one, _other, _configuration, review) = native_review();
    let response = response(&review);
    let bytes = serde_json::to_vec(&response).expect("response bytes");
    let decisions = response["decisions"].as_array().expect("decisions").len();
    let expected_binding = binding(&bytes);
    let matched = review
        .match_response(
            &bytes,
            &expected_binding,
            build::ReviewResponseLimits {
                bytes: bytes.len(),
                decisions,
            },
        )
        .expect("exact complete limits");
    assert_eq!(matched.decisions().len(), decisions);
    for limits in [
        build::ReviewResponseLimits {
            bytes: bytes.len() - 1,
            decisions,
        },
        build::ReviewResponseLimits {
            bytes: bytes.len(),
            decisions: decisions - 1,
        },
    ] {
        assert_eq!(
            review
                .match_response(&bytes, &expected_binding, limits)
                .expect_err("no partial matching or subset success at a logical bound")
                .code(),
            "publication_review_response_limit"
        );
    }
}
