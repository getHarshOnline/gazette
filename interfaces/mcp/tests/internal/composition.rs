//! Native composition discovery and pre-acquisition transport boundaries.

use super::*;
use serde_json::json;

fn arguments() -> Value {
    json!({"source":"not valid TOML", "site":"site:one"})
}

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named arguments").clone())
}

#[test]
fn discovery_and_decode_select_only_the_named_phase_and_native_defaults() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("native descriptor");
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        for field in [
            None,
            Some("templates"),
            Some("corpus"),
            Some("review"),
            Some("unknown"),
            Some("allow_network"),
            Some("allow_policy_execution"),
        ] {
            let mut value = arguments();
            if let Some(field) = field {
                value[field] = json!({});
            }
            let accepted =
                field.is_none() || (matches!(route, Route::Compile) && field == Some("templates"));
            assert_eq!(
                schema.is_valid(&value),
                accepted,
                "{}: {value}",
                route.name()
            );
            assert_eq!(decode(route, value).is_ok(), accepted, "{}", route.name());
        }
        let input = decode(route, arguments()).expect("default native limits");
        assert_eq!(
            json!(input.request.acquisition),
            json!(default_acquisition())
        );
        assert_eq!(json!(input.request.syntax), json!(toml::Limits::default()));
        if let Step::Compile(limits) = input.request.phase {
            assert_eq!(limits, render::Limits::default());
        }
        assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
    }
}

#[test]
fn denied_reads_precede_invalid_workspace_and_schema_valid_errors_remain_classified() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("native descriptor");
        let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
        for permission in [None, Some(false), Some(true)] {
            let mut value = arguments();
            if let Some(permission) = permission {
                value["allow_read"] = json!(permission);
            }
            let result = decode(route, value)
                .expect("named request")
                .execute(8192)
                .expect("classified failure");
            assert_eq!(result.is_error, Some(true));
            let output = result.structured_content.expect("complete failure record");
            schema
                .validate(&output)
                .expect("native error matches declared schema");
            if permission == Some(true) {
                let native = build::acquire_composition(
                    "not valid TOML",
                    &"site:one".parse().expect("site key"),
                    Default::default(),
                    default_acquisition(),
                )
                .expect_err("actual native configuration failure");
                assert_eq!(output["code"], native.code());
            } else {
                assert_eq!(output["code"], "composition_permission");
            }
        }
    }
}

#[test]
fn explicit_native_workspace_selection_is_schema_valid_and_permission_gated() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("descriptor");
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("schema");
        let value = json!({
            "source": {"kind":"local", "root":"/unselected-fixture-root", "workspace":"gazette.toml"},
            "site":"site:one"
        });
        assert!(schema.is_valid(&value));
        let result = decode(route, value)
            .expect("native selection shape")
            .execute(8192)
            .expect("permission result");
        assert_eq!(
            result.structured_content.expect("failure")["code"],
            "composition_permission"
        );
        for invalid in [
            json!({"kind":"local"}),
            json!({"kind":"local", "root":"/root", "workspace":"../escape"}),
            json!({"kind":"unknown"}),
            json!({"kind":"local", "root":"/root", "workspace":"gazette.toml", "limits":[]}),
            json!({"kind":"local", "root":"/root", "workspace":"gazette.toml", "limits":{"source":[]}}),
            json!({"kind":"local", "root":"/root", "workspace":"gazette.toml", "limits":{"syntax":[]}}),
            json!({"kind":"local", "root":"/root", "workspace":"gazette.toml", "limits":{"syntax":{"document":[]}}}),
            Value::Null,
        ] {
            let value = json!({"source":invalid,"site":"site:one"});
            assert!(!schema.is_valid(&value));
            assert!(decode(route, value).is_err());
        }
    }
}

#[test]
fn every_nested_native_limit_object_rejects_arrays_nulls_and_unknown_fields() {
    for route in Route::ALL {
        let descriptor = serde_json::to_value(route.descriptor()).expect("native descriptor");
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        let mut baseline = arguments();
        baseline["syntax"] = json!(toml::Limits::default());
        baseline["acquisition"] = json!(default_acquisition());
        let mut paths = vec![
            "/syntax",
            "/acquisition",
            "/acquisition/per_source",
            "/acquisition/per_source/content",
        ];
        if matches!(route, Route::Compile) {
            baseline["templates"] = json!(render::Limits::default());
            paths.extend(["/templates", "/templates/templates"]);
        }
        assert!(schema.is_valid(&baseline));
        assert!(decode(route, baseline.clone()).is_ok());
        for path in paths {
            for invalid in [json!([]), Value::Null, json!({"unknown":1})] {
                let mut value = baseline.clone();
                *value.pointer_mut(path).expect("selected native object") = invalid;
                assert!(!schema.is_valid(&value), "{}: {path}", route.name());
                assert!(decode(route, value).is_err(), "{}: {path}", route.name());
            }
        }
    }
}

#[test]
fn integer_bounds_agree_with_native_decoding_without_running_a_provider() {
    let route = Route::Compile;
    let descriptor = serde_json::to_value(route.descriptor()).expect("native descriptor");
    let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
    let mut baseline = arguments();
    baseline["syntax"] = json!(toml::Limits::default());
    baseline["acquisition"] = json!(default_acquisition());
    baseline["templates"] = json!(render::Limits::default());
    for (path, maximum) in [
        ("/syntax/source_bytes", usize::MAX as u64),
        ("/acquisition/sources", usize::MAX as u64),
        ("/acquisition/bytes", u64::MAX),
        ("/acquisition/per_source/metadata_bytes", usize::MAX as u64),
        ("/templates/context_bytes", usize::MAX as u64),
        ("/templates/output_bytes", usize::MAX as u64),
        ("/templates/templates/entries", usize::MAX as u64),
        ("/templates/templates/file_bytes", u64::MAX),
    ] {
        for valid in [json!(0), json!(maximum)] {
            let mut value = baseline.clone();
            *value.pointer_mut(path).expect("native numeric field") = valid;
            assert!(schema.is_valid(&value), "{path}");
            assert!(decode(route, value).is_ok(), "{path}");
        }
        for invalid in [
            json!(-1),
            json!(1.5),
            json!("1"),
            json!(18_446_744_073_709_551_616.0_f64),
        ] {
            let mut value = baseline.clone();
            *value.pointer_mut(path).expect("native numeric field") = invalid;
            assert!(!schema.is_valid(&value), "{path}: {value}");
            assert!(decode(route, value).is_err(), "{path}");
        }
    }
}
