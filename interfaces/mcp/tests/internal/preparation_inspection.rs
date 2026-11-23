//! Native decoder, permission and exact preparation-verification projection boundaries.

use super::*;
use gazette_cli::sdk::store;
use serde_json::json;
use std::{fs, os::unix::fs::PermissionsExt};

const AUTHORIZATION: &str = include_str!(
    "../../../../crates/gazette-compiler/tests/release_authorization/authorization.json"
);
const BINDING: &str =
    include_str!("../../../../crates/gazette-compiler/tests/release_authorization/binding.json");

fn selection() -> Value {
    json!({"preparation_id":"11".repeat(32),"site":"site:sample","domain":"0.example",
        "commit":"aa".repeat(20),"expected_remote_oid_or_unborn":{"state":"unborn"}})
}

fn arguments(route: Route) -> Value {
    let binding: Value = serde_json::from_str(BINDING).expect("independent native binding fixture");
    let mut value = json!({"database":"/gazette-absent-private/preparations.sqlite",
        "expected_header":binding["journal"],"selections_json":json!([selection()]).to_string(),
        "protected_roots":[]});
    if matches!(route, Route::Verify) {
        value["authorization_json"] = AUTHORIZATION.into();
        value["binding_json"] = BINDING.into();
    }
    value
}

fn decoded(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named request").clone())
}

fn execute(route: Route, value: Value, maximum: usize) -> CallToolResult {
    decoded(route, value)
        .expect("valid carrier shape")
        .execute(maximum)
        .expect("bounded complete response")
}

fn code(result: &CallToolResult) -> &str {
    assert_eq!(result.is_error, Some(true), "{result:?}");
    result
        .structured_content
        .as_ref()
        .expect("structured failure")["code"]
        .as_str()
        .expect("native redacted code")
}

#[test]
fn discovery_and_decoding_preserve_native_named_options_and_distinct_effect_permissions() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        let original = arguments(route);
        assert!(schema.is_valid(&original));
        let input = decoded(route, original.clone()).expect("omitted native defaults");
        assert!(!input.request.allow_read);
        assert!(
            !input
                .request
                .options
                .inspection
                .read
                .allow_coordination_files
        );
        assert_eq!(
            input.request.options.targets,
            release::PreparationBatchOptions::default().targets
        );
        assert_eq!(descriptor["annotations"]["readOnlyHint"], false);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["idempotentHint"], true);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
        assert_eq!(
            descriptor["inputSchema"]["properties"]["selections_json"]["contentMediaType"],
            "application/json"
        );
        for (field, value) in [
            ("database", json!("relative.sqlite")),
            ("expected_header", Value::Null),
            ("expected_header", json!([])),
            ("protected_roots", json!(["relative-source"])),
            ("selections_json", json!([selection()])),
            ("options", Value::Null),
            ("options", json!([])),
            ("options", json!({"inspection":[]})),
            ("options", json!({"inspection":{"read":[]}})),
            ("options", json!({"inspection":{"worktree":{"history":[]}}})),
            (
                "options",
                json!({"inspection":{"worktree":{"output":{"content":[]}}}}),
            ),
            ("options", json!({"targets":-1})),
            (
                "options",
                json!({"inspection":{"read":{"allow_create":true}}}),
            ),
            ("options", json!({"unknown":1})),
            ("allow_write", json!(true)),
            ("allow_network", json!(true)),
        ] {
            let mut changed = original.clone();
            changed[field] = value;
            assert!(!schema.is_valid(&changed), "{}: {changed}", route.name());
            assert!(decoded(route, changed).is_err());
        }
        let output = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
        for maximum in [1 << 20, 1] {
            let response = execute(route, original.clone(), maximum);
            assert!(
                output.is_valid(
                    response
                        .structured_content
                        .as_ref()
                        .expect("complete failure")
                )
            );
        }
    }
    let mut wrong = arguments(Route::Inspect);
    wrong["authorization_json"] = AUTHORIZATION.into();
    assert!(decoded(Route::Inspect, wrong).is_err());
    for (field, value) in [
        ("authorization_json", json!({})),
        ("binding_json", json!({})),
        ("limits", json!([])),
        ("limits", json!({"unknown":1})),
    ] {
        let mut wrong = arguments(Route::Verify);
        wrong[field] = value;
        assert!(decoded(Route::Verify, wrong).is_err());
    }
}

#[test]
fn shared_selection_decoder_rejects_duplicates_positional_forms_and_hidden_variant_fields() {
    let native = selection().to_string();
    let accepted = release::parse_preparation_selection(format!(" \n{native}\t").as_bytes())
        .expect("ordinary JSON whitespace preserves native meaning");
    assert_eq!(json!(accepted), selection());
    let mut unknown = selection();
    unknown["PRIVATE_SENTINEL"] = json!(true);
    let mut variant = selection();
    variant["expected_remote_oid_or_unborn"]["PRIVATE_SENTINEL"] = json!(true);
    let mut positional = selection();
    positional["expected_remote_oid_or_unborn"] = json!(["unborn"]);
    for malformed in [
        "[]".to_owned(),
        "null".to_owned(),
        unknown.to_string(),
        variant.to_string(),
        positional.to_string(),
        native.replacen('{', "{\"domain\":\"PRIVATE_SENTINEL\",", 1),
        native.replacen('{', "{\"d\\u006fmain\":\"PRIVATE_SENTINEL\",", 1),
        native.replace(
            "\"state\":\"unborn\"",
            "\"state\":\"unborn\",\"state\":\"unborn\"",
        ),
    ] {
        assert!(release::parse_preparation_selection(malformed.as_bytes()).is_err());
        for route in Route::ALL {
            let mut input = arguments(route);
            input["allow_read"] = true.into();
            input["selections_json"] = format!("[{malformed}]").into();
            let response = execute(route, input, 1 << 20);
            assert_eq!(code(&response), "release_preparation_selection");
            assert!(!format!("{response:?}").contains("PRIVATE_SENTINEL"));
        }
    }
}

#[test]
fn explicit_permission_and_full_selection_bounds_fail_without_partial_native_evidence() {
    for route in Route::ALL {
        let mut denied = arguments(route);
        denied["selections_json"] = "PRIVATE_SENTINEL malformed JSON".into();
        let response = execute(route, denied, 1 << 20);
        assert_eq!(code(&response), "native_read_permission");
        assert!(!format!("{response:?}").contains("PRIVATE_SENTINEL"));
        for (selections, options) in [
            ("[]".to_owned(), json!({})),
            (json!([selection()]).to_string(), json!({"targets":0})),
            (
                json!([selection(), selection()]).to_string(),
                json!({"targets":1}),
            ),
        ] {
            let mut input = arguments(route);
            input["allow_read"] = true.into();
            input["selections_json"] = selections.into();
            input["options"] = options;
            assert_eq!(
                code(&execute(route, input, 1 << 20)),
                "release_preparation_selection"
            );
        }
    }
}

#[test]
fn raw_authorization_and_binding_fail_in_the_real_verifier_before_missing_native_inputs() {
    for (field, raw, expected) in [
        (
            "authorization_json",
            AUTHORIZATION.replacen('{', "{\"nonce\":\"PRIVATE_SENTINEL\",", 1),
            "release_json",
        ),
        (
            "binding_json",
            BINDING.replacen('{', "{\"observed_at\":120,", 1),
            "release_json",
        ),
        (
            "binding_json",
            BINDING.replace("\"observed_at\": 120", "\"observed_at\": -0"),
            "release_number",
        ),
    ] {
        let mut input = arguments(Route::Verify);
        input["allow_read"] = true.into();
        input[field] = raw.into();
        let response = execute(Route::Verify, input, 1 << 20);
        assert_eq!(code(&response), expected);
        assert!(!format!("{response:?}").contains("PRIVATE_SENTINEL"));
    }
    for field in ["bytes", "nodes", "depth", "targets"] {
        let mut input = arguments(Route::Verify);
        input["allow_read"] = true.into();
        input["limits"] = json!({field:0});
        assert_eq!(
            code(&execute(Route::Verify, input, 1 << 20)),
            "release_limit"
        );
    }
}

#[test]
fn actual_journal_without_preparation_returns_the_same_complete_native_refusal_as_sdk() {
    let root = tempfile::tempdir().expect("private disposable native journal");
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).expect("private root");
    let read = store::ReadOptions {
        allow_coordination_files: true,
        ..Default::default()
    };
    let initialized = store::initialize(
        root.path(),
        store::InitializeOptions {
            allow_create: true,
            session: store::SessionOptions {
                allow_local_reservation: true,
                read,
            },
            ..Default::default()
        },
    )
    .expect("real fresh native schema");
    let before = fs::read(&initialized.database).expect("native original database");
    for allowed in [false, true] {
        let mut input = arguments(Route::Inspect);
        input["database"] = json!(initialized.database);
        input["expected_header"] = json!(initialized.header);
        input["allow_read"] = true.into();
        input["options"] = json!({"inspection":{"read":{"allow_coordination_files":allowed}}});
        let parsed = decoded(Route::Inspect, input.clone()).expect("native option projection");
        let selections = vec![
            release::parse_preparation_selection(selection().to_string().as_bytes())
                .expect("native selection"),
        ];
        let expected = release::inspect_preparations(
            release::PreparationBatchRequest {
                database: &initialized.database,
                expected: initialized.header,
                selections: &selections,
                protected_roots: &[],
            },
            parsed.request.options,
        )
        .expect_err("missing native preparation is not success");
        let response = execute(Route::Inspect, input, 1 << 20);
        assert_eq!(code(&response), expected.code());
        assert_eq!(
            fs::read(&initialized.database).expect("inspection never rewrites SQL"),
            before
        );
    }
}
