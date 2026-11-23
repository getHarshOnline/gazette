//! Discoverable native site-documentation arguments agree with execution boundaries.

use super::*;
use serde_json::json;

fn arguments() -> Value {
    json!({"source":"not valid TOML", "site":"site:one"})
}

fn decode(route: Route, value: Value) -> Result<Input, ErrorData> {
    route.decode(value.as_object().expect("named arguments").clone())
}

#[test]
fn named_phase_and_defaults_match_discovery_without_unrelated_options() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).expect("input schema");
        for field in [
            None,
            Some("corpus"),
            Some("templates"),
            Some("graph"),
            Some("review"),
            Some("allow_network"),
            Some("allow_policy_execution"),
        ] {
            let mut value = arguments();
            if let Some(field) = field {
                value[field] = json!({});
            }
            let accepted =
                field.is_none() || (matches!(route, Route::Documents) && field == Some("corpus"));
            assert_eq!(
                schema.is_valid(&value),
                accepted,
                "{}: {value}",
                route.name()
            );
            assert_eq!(decode(route, value).is_ok(), accepted);
        }
        let input = decode(route, arguments()).expect("native defaults");
        assert!(!input.request.allow_read);
        assert_eq!(json!(input.request.syntax), json!(toml::Limits::default()));
        assert_eq!(
            json!(input.request.acquisition),
            json!(default_acquisition())
        );
        match route {
            Route::Inspect => assert!(input.request.phase.is_none()),
            Route::Documents => assert_eq!(
                json!(input.request.phase.expect("corpus")),
                json!(corpus::Limits::default())
            ),
        }
        assert_eq!(descriptor["annotations"]["readOnlyHint"], true);
        assert_eq!(descriptor["annotations"]["destructiveHint"], false);
        assert_eq!(descriptor["annotations"]["idempotentHint"], true);
        assert_eq!(descriptor["annotations"]["openWorldHint"], true);
    }
}

#[test]
fn permission_precedes_native_workspace_parser_and_failures_match_output_schema() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["outputSchema"]).expect("output schema");
        for permission in [None, Some(false), Some(true)] {
            let mut value = arguments();
            if let Some(permission) = permission {
                value["allow_read"] = json!(permission);
            }
            let result = decode(route, value)
                .expect("named request")
                .execute(8192)
                .expect("tool result");
            assert_eq!(result.is_error, Some(true));
            let output = result.structured_content.expect("classified failure");
            schema.validate(&output).expect("actual failure schema");
            if permission == Some(true) {
                let native = build::acquire_site(
                    "not valid TOML",
                    &"site:one".parse().expect("site"),
                    Default::default(),
                    default_acquisition(),
                )
                .expect_err("native TOML failure");
                assert_eq!(output["code"], native.code());
            } else {
                assert_eq!(output["code"], "site_permission");
            }
        }
    }
}

#[test]
fn native_workspace_selection_is_discoverable_and_requires_read_permission() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
        let schema = jsonschema::validator_for(&descriptor["inputSchema"]).unwrap();
        let mut value = arguments();
        value["source"] = json!({
            "kind": "local", "root": "/unavailable/workspace", "workspace": "gazette.toml"
        });
        assert!(schema.is_valid(&value), "{value}");
        let result = decode(route, value).unwrap().execute(8192).unwrap();
        assert_eq!(
            result.structured_content.unwrap()["code"],
            "site_permission"
        );
    }
}

#[test]
fn every_nested_limit_uses_named_native_shapes_and_integer_bounds() {
    for route in Route::ALL {
        let descriptor = json!(route.descriptor());
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
        if matches!(route, Route::Documents) {
            baseline["corpus"] = json!(corpus::Limits::default());
            paths.extend([
                "/corpus",
                "/corpus/document",
                "/corpus/document/metadata",
                "/corpus/document/markdown",
                "/corpus/notebook",
            ]);
        }
        assert!(schema.is_valid(&baseline));
        assert!(decode(route, baseline.clone()).is_ok());
        for path in paths {
            for invalid in [json!([]), Value::Null, json!({"unknown":1})] {
                let mut value = baseline.clone();
                *value.pointer_mut(path).expect("native limit object") = invalid;
                assert!(!schema.is_valid(&value), "{path}: {value}");
                assert!(decode(route, value).is_err(), "{path}");
            }
            for (field, default) in baseline
                .pointer(path)
                .expect("native limit object")
                .as_object()
                .expect("object")
            {
                if !default.is_number() {
                    continue;
                }
                let pointer = format!("{path}/{field}");
                for (replacement, accepted) in [
                    (json!(0), true),
                    (json!(-1), false),
                    (json!(1.5), false),
                    (json!("1"), false),
                    (json!(18_446_744_073_709_551_616.0_f64), false),
                ] {
                    let mut value = baseline.clone();
                    *value.pointer_mut(&pointer).expect("native integer bound") = replacement;
                    assert_eq!(schema.is_valid(&value), accepted, "{pointer}: {value}");
                    assert_eq!(decode(route, value).is_ok(), accepted, "{pointer}");
                }
            }
        }
    }
}
