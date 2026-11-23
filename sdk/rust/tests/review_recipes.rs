//! Shared recipe transport retains independent review bytes without granting acceptance.

use std::collections::BTreeMap;

use gazette_sdk::{authored, build, source::Sha256Digest, store};
use serde_json::{Value, json};

const RESPONSE: &str = "{ \"original\": \"é\", \"original\": 2 }\n";

fn submission() -> build::ReviewSubmission {
    build::ReviewSubmission {
        binding: build::ReviewResponseBinding {
            policy: build::ReviewPolicy {
                id: "policy:one".parse().expect("policy"),
                revision: "revision:one".parse().expect("revision"),
                source_sha256: Sha256Digest::of(b"independent policy"),
            },
            response_sha256: Sha256Digest::of(RESPONSE.as_bytes()),
        },
        response_json: RESPONSE.into(),
    }
}

fn site(name: &str) -> build::SiteRecipe {
    build::SiteRecipe {
        site: format!("site:{name}").parse().expect("site"),
        model_output: Some("model.json".parse().expect("output")),
        graph: None,
    }
}

fn responses(names: &[&str]) -> BTreeMap<gazette_sdk::workspace::Key, build::ReviewSubmission> {
    names
        .iter()
        .map(|name| (site(name).site, submission()))
        .collect()
}

fn selection() -> authored::Selection {
    serde_json::from_value(json!({"content":{},"assets":{"one/image.png":"include"}}))
        .expect("asset-only candidate is a valid transport selection")
}

fn preparation() -> Value {
    let signature =
        json!({"name":"Fixture","email":"fixture@example.invalid","seconds":0,"offset_minutes":0});
    json!({
        "workspace":{"kind":"supplied","source":"invalid TOML deliberately never acquired by transport validation"},
        "journal":"/not-opened/journal", "expected_header":store::JournalHeader {
            schema_version:1,journal_id:[1;32],journal_epoch:1,writer_fence:0
        },
        "coordinator":null,"selection":selection(),"disclosure":{"rules":{}},
        "candidate_check":build::ReviewLimits::default(),
        "sites":[{
            "render":site("one"),"stage_parent":"/not-opened/stages","stage_name":"one",
            "preparation_id":Sha256Digest::of(b"one"),
            "metadata":{"parent":null,"author":signature,"committer":signature,"message":"Fixture"},
            "parent_source":null
        }]
    })
}

fn parse(value: &Value) -> Result<build::LocalPreparationRecipe, &'static str> {
    build::LocalPreparationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
}

#[test]
fn preparation_math_selection_preserves_omission_and_strict_shared_effect_options() {
    let base = preparation();
    let schema = build::LocalPreparationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).expect("preparation schema");
    parse(&base).expect("ordinary preparation omits acquisition");
    let defaults = json!(build::MathGeneration::default());
    for permission in [false, true] {
        let mut value = base.clone();
        value["math_generation"] = defaults.clone();
        value["math_generation"]["allow_execute"] = json!(permission);
        assert!(validator.is_valid(&value));
        parse(&value).expect("named explicit effect options, no source or process activity");
    }
    for options in [
        Value::Null,
        json!([]),
        json!([false, 16_777_216, 30_000, 4096, 67_108_864]),
        json!({}),
        {
            let mut value = defaults.clone();
            value["formulas"] = json!(4097);
            value
        },
        {
            let mut value = defaults.clone();
            value["unknown"] = json!(true);
            value
        },
    ] {
        let mut value = base.clone();
        value["math_generation"] = options;
        assert!(!validator.is_valid(&value), "{value}");
        assert!(parse(&value).is_err(), "{value}");
    }
    let mut value = base;
    value["math_generation"] = defaults;
    let duplicate = value.to_string().replace(
        "\"allow_execute\":false",
        "\"allow_execute\":true,\"allow_execute\":false",
    );
    assert!(build::LocalPreparationRecipe::parse_json(duplicate.as_bytes(), 65536, 2).is_err());
}

#[test]
fn shared_owned_recipe_borrows_exact_bytes_bindings_and_complete_site_set() {
    let recipe = build::PublicationRecipe::new(
        vec![site("two"), site("one")],
        Some(selection()),
        Some(responses(&["one", "two"])),
        2,
    )
    .expect("complete explicit response set");
    let first = recipe.options(Default::default());
    let second = recipe.options(Default::default());
    let first = first
        .review_responses
        .expect("responses projected, not dropped");
    assert!(std::ptr::eq(
        first,
        second.review_responses.expect("same owner")
    ));
    assert_eq!(
        first[&site("one").site].response_json.as_bytes(),
        RESPONSE.as_bytes()
    );
    assert_eq!(first[&site("one").site].binding, submission().binding);
    for selected in [
        vec![],
        vec!["one"],
        vec!["one", "extra"],
        vec!["one", "two", "extra"],
    ] {
        assert!(
            build::PublicationRecipe::new(
                vec![site("one"), site("two")],
                Some(selection()),
                Some(responses(&selected)),
                2,
            )
            .is_err(),
            "partial or foreign response keys must not be ignored"
        );
    }
    let derived =
        build::PublicationRecipe::new(vec![site("one")], None, Some(responses(&["one"])), 1)
            .expect("checked rendering derives the selection before matching the response");
    let options = derived.options(Default::default());
    assert!(
        options.selection.is_none(),
        "transport must not invent a selection"
    );
    assert_eq!(
        options.review_responses.unwrap()[&site("one").site].response_json,
        RESPONSE
    );
}

#[test]
fn preparation_omission_is_preserved_but_present_response_needs_checker() {
    let schema = build::LocalPreparationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).expect("native schema");
    let mut value = preparation();
    assert!(validator.is_valid(&value));
    parse(&value).expect("omitted optional response survives complete-shape equality");
    value["review_responses"] = json!(responses(&["one"]));
    assert!(validator.is_valid(&value));
    parse(&value).expect("native exact response transport, not response acceptance");
    value.as_object_mut().unwrap().remove("selection");
    assert!(validator.is_valid(&value));
    parse(&value).expect("supplied response can bind the derived complete candidate");
    value["candidate_check"] = Value::Null;
    assert_eq!(
        parse(&value).expect_err("no silent ignore"),
        "review responses require an explicitly selected candidate checker"
    );
    value
        .as_object_mut()
        .expect("request")
        .remove("review_responses");
    parse(&value).expect("existing nullable checker remains valid without responses");
    value
        .as_object_mut()
        .expect("request")
        .remove("candidate_check");
    assert!(
        parse(&value).is_err(),
        "existing nullable field remains mandatory"
    );
}

#[test]
fn all_response_named_boundaries_and_duplicate_keys_fail_before_acquisition() {
    let mut valid = preparation();
    valid["review_responses"] = json!(responses(&["one"]));
    let schema = build::LocalPreparationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).expect("native schema");
    for (pointer, replacement) in [
        ("/review_responses", Value::Null),
        ("/review_responses", json!([])),
        (
            "/review_responses/site:one",
            json!([submission().binding, RESPONSE]),
        ),
        (
            "/review_responses/site:one/binding",
            json!([
                submission().binding.policy,
                submission().binding.response_sha256
            ]),
        ),
        (
            "/review_responses/site:one/binding/policy",
            json!(["policy:one", "revision:one", Sha256Digest::of(b"policy")]),
        ),
        (
            "/review_responses/site:one/response_json",
            json!({"must":"remain string"}),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).expect("known selected field") = replacement;
        assert!(parse(&invalid).is_err(), "native accepted {pointer}");
        assert!(!validator.is_valid(&invalid), "schema accepted {pointer}");
    }
    for pointer in [
        "/review_responses/site:one",
        "/review_responses/site:one/binding",
        "/review_responses/site:one/binding/policy",
    ] {
        let mut invalid = valid.clone();
        invalid.pointer_mut(pointer).expect("object")["unknown"] = json!(true);
        assert!(parse(&invalid).is_err());
        assert!(!validator.is_valid(&invalid));
    }
    let mut invalid = valid.clone();
    invalid["review_responses"] = json!({"bad key":submission()});
    assert!(parse(&invalid).is_err());
    assert!(!validator.is_valid(&invalid));
    for field in ["response_json", "binding", "policy", "response_sha256"] {
        let duplicate = valid.to_string().replacen(
            &format!("\"{field}\":"),
            &format!("\"{field}\":null,\"{field}\":"),
            1,
        );
        assert!(build::LocalPreparationRecipe::parse_json(duplicate.as_bytes(), 65536, 2).is_err());
    }
}

#[test]
fn aggregate_and_per_response_bounds_are_independent_and_precede_native_work() {
    let mut value = preparation();
    let mut second = value["sites"][0].clone();
    second["render"]["site"] = json!("site:two");
    second["preparation_id"] = json!(Sha256Digest::of(b"two"));
    value["sites"].as_array_mut().expect("sites").push(second);
    value["review_responses"] = json!(responses(&["one", "two"]));
    let recipe = parse(&value).expect("two response transport");
    let options = |per_response, aggregate| build::LocalPreparationRunOptions {
        allow_read: true,
        review_response: build::ReviewResponseLimits {
            bytes: per_response,
            decisions: 1,
        },
        review_response_bytes: aggregate,
        ..Default::default()
    };
    for (per_response, aggregate, reason) in [
        (RESPONSE.len() - 1, RESPONSE.len() * 2, "per-response"),
        (RESPONSE.len(), RESPONSE.len() * 2 - 1, "aggregate"),
        (0, RESPONSE.len() * 2, "nonzero"),
        (RESPONSE.len(), 0, "nonzero"),
    ] {
        let error = recipe
            .execute(options(per_response, aggregate))
            .expect_err("bound refuses before native source or preparation");
        let build::LocalPreparationRunError::Before { code, message } = error else {
            panic!("no native effects allowed")
        };
        assert_eq!(code, "local_preparation_review_responses");
        assert!(message.contains(reason), "{message}");
    }
    let error = recipe
        .execute(options(RESPONSE.len(), RESPONSE.len() * 2))
        .expect_err("later native permission preflight still refuses");
    let build::LocalPreparationRunError::Before { code, .. } = error else {
        panic!("no native effects allowed")
    };
    assert_eq!(
        code, "local_preparation_preflight",
        "exact independent bounds pass to existing native guard"
    );
}

#[test]
fn unguarded_local_rendering_explicitly_refuses_response_transport() {
    let mut value = json!({"format":"gazette-local-publication-set/1","workspace":{"kind":"supplied","source":"not opened"},"selection":selection(),"sites":[site("one")]});
    build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
        .expect("existing unguarded local recipe");
    value["review_responses"] = json!(responses(&["one"]));
    let schema = build::LocalPublicationRecipe::input_schema();
    jsonschema::validator_for(schema.as_value())
        .expect("discovery")
        .validate(&value)
        .expect("recognized response shape");
    assert_eq!(
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2)
            .expect_err("checker cannot be implied"),
        "local rendering has no candidate checker; review responses require checked preparation"
    );
    value["review_responses"] = Value::Null;
    assert!(
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 65536, 2).is_err()
    );
}
