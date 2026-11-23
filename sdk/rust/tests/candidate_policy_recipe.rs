//! Live candidate requirements are independent inputs, never reconstructed from stdout.

use gazette_sdk::{authored, build, process, source::Sha256Digest, store};
use serde_json::{Value, json};

fn requirement() -> build::CandidatePolicyRequirement {
    build::CandidatePolicyRequirement {
        policy: build::ReviewPolicy {
            id: "policy:one".parse().unwrap(),
            revision: "revision:one".parse().unwrap(),
            source_sha256: Sha256Digest::of(b"independently selected policy bytes"),
        },
        command_sha256: Sha256Digest::of(b"independently selected command"),
    }
}

fn recipe() -> Value {
    let signature =
        json!({"name":"Fixture","email":"fixture@example.invalid","seconds":0,"offset_minutes":0});
    json!({
        "workspace":{"kind":"supplied","source":"invalid TOML: transport validation must not acquire anything"},
        "journal":"/not-opened/journal",
        "expected_header":store::JournalHeader { schema_version:1,journal_id:[1;32],journal_epoch:1,writer_fence:0 },
        "coordinator":null,
        "selection":serde_json::from_value::<authored::Selection>(json!({"content":{},"assets":{"image.png":"include"}})).unwrap(),
        "disclosure":{"rules":{}},"candidate_check":build::ReviewLimits::default(),
        "candidate_policies":{"site:one":requirement()},
        "sites":[{
            "render":build::SiteRecipe { site:"site:one".parse().unwrap(),model_output:Some("model.json".parse().unwrap()),graph:None },
            "stage_parent":"/not-opened/stages","stage_name":"one","preparation_id":Sha256Digest::of(b"one"),
            "metadata":{"parent":null,"author":signature,"committer":signature,"message":"Fixture"},"parent_source":null
        }]
    })
}

fn parse(value: &Value) -> Result<build::LocalPreparationRecipe, &'static str> {
    build::LocalPreparationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
}

#[test]
fn exact_live_policy_shape_and_omission_share_native_discovery() {
    let schema = build::LocalPreparationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    let mut input = recipe();
    assert!(validator.is_valid(&input));
    parse(&input).expect("policy selection is not runtime approval");
    input.as_object_mut().unwrap().remove("candidate_policies");
    assert!(validator.is_valid(&input));
    parse(&input).expect("existing exit-only/imported-response direction remains available");
}

#[test]
fn live_requirements_need_explicit_checker_and_cannot_mix_imported_responses() {
    let mut input = recipe();
    input["candidate_check"] = Value::Null;
    assert_eq!(
        parse(&input).unwrap_err(),
        "candidate policies require an explicitly selected candidate checker"
    );
    let mut input = recipe();
    input["review_responses"] = json!({"site:one":build::ReviewSubmission {
        binding:build::ReviewResponseBinding { policy:requirement().policy,response_sha256:Sha256Digest::of(b"{}") },
        response_json:"{}".into(),
    }});
    assert_eq!(
        parse(&input).unwrap_err(),
        "live candidate policies cannot mix with imported review responses"
    );
}

#[test]
fn live_requirements_cover_exact_complete_site_set() {
    for keys in [vec![], vec!["site:two"], vec!["site:one", "site:two"]] {
        let mut input = recipe();
        input["candidate_policies"] = Value::Object(
            keys.into_iter()
                .map(|key| (key.into(), json!(requirement())))
                .collect(),
        );
        assert_eq!(
            parse(&input).unwrap_err(),
            "candidate policies require the exact complete requested site set"
        );
    }
    let mut input = recipe();
    let mut second = input["sites"][0].clone();
    second["render"]["site"] = json!("site:two");
    second["preparation_id"] = json!(Sha256Digest::of(b"two"));
    input["sites"].as_array_mut().unwrap().push(second);
    assert!(
        parse(&input).is_err(),
        "partial selection cannot silently shrink the set"
    );
    input["candidate_policies"]["site:two"] = json!(requirement());
    parse(&input).expect("complete multi-site set");
}

#[test]
fn malformed_nested_requirements_cannot_hide_in_serde_positional_forms() {
    let required = json!(requirement());
    let invalid = [
        Value::Null,
        json!([]),
        json!({"site:one":[required["policy"],required["command_sha256"]]}),
        json!({"site:one":{"policy":["policy:one","revision:one",required["policy"]["source_sha256"]],"command_sha256":required["command_sha256"]}}),
        json!({"site:one":{"policy":required["policy"]}}),
        json!({"site:one":{"policy":required["policy"],"command_sha256":required["command_sha256"],"response_sha256":required["command_sha256"]}}),
    ];
    let schema = build::LocalPreparationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for value in invalid {
        let mut input = recipe();
        input["candidate_policies"] = value;
        assert!(
            !validator.is_valid(&input),
            "discovery rejects non-native shapes"
        );
        assert!(parse(&input).is_err(), "decoder rejects non-native shapes");
    }
}

#[test]
fn duplicate_decoded_policy_keys_are_not_last_writer_wins() {
    let input = recipe().to_string();
    let duplicate = input.replace(
        "\"candidate_policies\":{",
        &format!(
            "\"candidate_policies\":{{\"site:one\":{},",
            json!(requirement())
        ),
    );
    assert!(build::LocalPreparationRecipe::parse_json(duplicate.as_bytes(), 65536, 2).is_err());
}

fn preflight(command: &str) -> build::LocalPreparationRunError {
    let mut input = recipe();
    input["workspace"] = json!({
        "kind":"supplied",
        "source":format!(
            "{command}\n[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n"
        )
    });
    parse(&input)
        .unwrap()
        .execute(build::LocalPreparationRunOptions {
            allow_read: true,
            ..Default::default()
        })
        .unwrap_err()
}

#[test]
fn absent_native_checker_fails_before_acquisition_or_preparation_permission_checks() {
    let error = preflight("");
    assert!(
        matches!(
            &error,
            build::LocalPreparationRunError::Before {
                code: "policy_unconfigured",
                ..
            }
        ),
        "{error}"
    );
}

#[test]
fn unpinned_or_wrong_command_fails_before_opening_selected_native_paths() {
    for pin in [
        String::new(),
        format!(
            "executable={{bytes=1,sha256='{}'}}\n",
            Sha256Digest::of(b"x")
        ),
    ] {
        let error = preflight(&format!(
            "[policy_check]\nprogram='/not-opened/checker'\ndirectory='/not-opened'\n{pin}"
        ));
        assert!(
            matches!(
                &error,
                build::LocalPreparationRunError::Before {
                    code: "publication_review_response_binding",
                    ..
                }
            ),
            "{error}"
        );
    }
}

#[test]
fn failed_captured_candidate_retains_existing_native_custody_and_byte_evidence() {
    let stdout = b"invalid response\xff".to_vec();
    let invocation = build::PolicyInvocation {
        request_sha256: Sha256Digest::of(b"request"),
        command_sha256: requirement().command_sha256,
    };
    let error =
        build::LocalPreparationRunError::Candidate(Box::new(build::CandidateCheckFailure {
            completed: Default::default(),
            completed_responses: Default::default(),
            site: "site:one".parse().unwrap(),
            cause: build::PolicyCheckError::CandidateResponse {
                invocation,
                captured: process::Captured {
                    exit: process::Exit { code: Some(0) },
                    stdout: stdout.clone(),
                    stderr: None,
                },
                source: build::ReviewResponseError::Disposition,
            },
        }));
    let report = serde_json::to_value(error).unwrap();
    assert_eq!(report["status"], "candidate-failed");
    assert_eq!(report["completed_responses"], json!({}));
    assert_eq!(report["evidence"]["invocation"], json!(invocation));
    assert_eq!(report["evidence"]["child_custody"], "reaped");
    assert_eq!(report["evidence"]["exit"], json!({"code":0}));
    assert_eq!(
        report["evidence"]["response_sha256"],
        json!(Sha256Digest::of(&stdout))
    );
    assert_eq!(report["evidence"]["response_bytes"], stdout.len());
    assert!(report.get("technical_certificate").is_none());
}
