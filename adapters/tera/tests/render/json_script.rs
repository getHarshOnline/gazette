//! Real captured Tera rendering, including intermediate expansion failures.

use super::{captured, compile_port, has_native_tera_cause, output};
use gazette_adapter_tera::{Error, Limits, TemplateSet};
use serde_json::{Value, json};

#[test]
fn every_json_form_round_trips_through_the_explicit_filter_and_dynamic_port() {
    let source = captured(&[("value.html", b"{{ value | json_script }}")]);
    let direct = TemplateSet::compile(&source, Limits::default()).expect("direct templates");
    let dynamic = compile_port(&source, Limits::default()).expect("dynamic templates");
    let entry = "value.html".parse().expect("entry");
    drop(source);
    for value in [
        Value::Null,
        json!(true),
        json!(false),
        json!(i64::MIN),
        json!(u64::MAX),
        json!(-12.5),
        json!("null"),
        json!("雪🌟"),
        json!([]),
        json!({}),
        json!([null, true, -7, "text", {"key": [1, 2, 3]}]),
    ] {
        let context = json!({"value":value});
        let bytes = direct.render(&entry, &context).expect("supplied JSON");
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).expect("valid compact JSON"),
            value
        );
        assert_eq!(
            bytes,
            serde_json::to_vec(&value).expect("no additional escapes required")
        );
        assert_eq!(
            dynamic
                .render(&entry, &context)
                .expect("same port implementation"),
            bytes
        );
        assert_eq!(
            direct
                .render(&entry, &context)
                .expect("repeat without state"),
            bytes
        );
    }
}

#[test]
fn script_sensitive_characters_are_json_escaped_in_keys_and_values_without_html_entities() {
    let dangerous = "</script><script>alert('x' & y)</script>\u{2028}\u{2029}雪";
    let mut object = serde_json::Map::new();
    object.insert(
        dangerous.into(),
        json!([dangerous, "\"\\\n\r\t\0", "already \\u003c escaped"]),
    );
    let value = Value::Object(object);
    let source = captured(&[(
        "value.html",
        b"<script type=\"application/ld+json\">{{ value | json_script }}</script>",
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("script transport template");
    let rendered = output(&set, "value.html", json!({"value":value})).expect("script text");
    let payload = rendered
        .strip_prefix("<script type=\"application/ld+json\">")
        .and_then(|value| value.strip_suffix("</script>"))
        .expect("unchanged literal wrapper");
    assert_eq!(
        serde_json::from_str::<Value>(payload).expect("transport preserves supplied meaning"),
        value
    );
    for character in ['<', '>', '&', '\'', '\u{2028}', '\u{2029}'] {
        assert!(
            !payload.contains(character),
            "unescaped script-sensitive character: {character:?}"
        );
    }
    for escape in [
        "\\u003c", "\\u003e", "\\u0026", "\\u0027", "\\u2028", "\\u2029",
    ] {
        assert!(
            payload.matches(escape).count() >= 2,
            "keys and values both use {escape}"
        );
    }
    assert!(payload.contains("雪"), "unrelated Unicode is preserved");
    assert!(
        payload.contains("\\\"\\\\\\n\\r\\t\\u0000"),
        "serde retains its normal JSON escapes"
    );
    assert!(!payload.contains("&quot;"));
    assert_eq!(
        rendered.matches("</script>").count(),
        1,
        "only the template can close its script"
    );
}

#[test]
fn escaped_json_obeys_exact_output_budget_and_intermediate_expansion_fails_typed() {
    let source = captured(&[
        ("value.html", b"{{ value | json_script }}"),
        ("hidden.html", b"{% set encoded = value | json_script %}ok"),
        (
            "twice.html",
            b"{{ value | json_script }}{{ value | json_script }}",
        ),
    ]);
    let expected = b"\"\\u003c\"";
    let exact = Limits {
        output_bytes: expected.len(),
        ..Limits::default()
    };
    let direct = TemplateSet::compile(&source, exact).expect("exact-bound templates");
    let dynamic = compile_port(&source, exact).expect("exact-bound dynamic templates");
    let entry = "value.html".parse().expect("entry");
    let context = json!({"value":"<"});
    assert_eq!(
        direct
            .render(&entry, &context)
            .expect("exact expanded bytes"),
        expected
    );
    assert_eq!(
        dynamic
            .render(&entry, &context)
            .expect("exact expanded dynamic bytes"),
        expected
    );
    assert_eq!(
        output(&direct, "hidden.html", context.clone()).expect("bounded unused value"),
        "ok"
    );
    assert!(
        matches!(
            output(&direct, "twice.html", context.clone()),
            Err(Error::Limit("output bytes"))
        ),
        "outer output still counts repeated successful filters"
    );

    let smaller = Limits {
        output_bytes: expected.len() - 1,
        ..exact
    };
    let direct = TemplateSet::compile(&source, smaller).expect("one-below templates");
    let dynamic = compile_port(&source, smaller).expect("one-below dynamic templates");
    for name in ["value.html", "hidden.html"] {
        let entry = name.parse().expect("entry");
        assert!(
            matches!(
                direct.render(&entry, &context),
                Err(Error::Limit("output bytes"))
            ),
            "{name}: expanded intermediate must not evade the typed ceiling"
        );
        assert!(
            matches!(
                dynamic.render(&entry, &context),
                Err(Error::Limit("output bytes"))
            ),
            "{name}: port preserves typed filter overflow"
        );
        let recovery = json!({"value":true});
        let expected = if name == "hidden.html" {
            b"ok".as_slice()
        } else {
            b"true".as_slice()
        };
        assert_eq!(
            direct.render(&entry, &recovery).expect("no poisoned state"),
            expected
        );
        assert_eq!(
            dynamic.render(&entry, &recovery).expect("dynamic recovery"),
            expected
        );
    }
}

#[test]
fn escaped_keys_and_unicode_separators_are_charged_before_intermediate_success() {
    for (value, expected) in [
        (json!("\u{2028}\u{2029}"), "\"\\u2028\\u2029\""),
        (json!({"<":null}), "{\"\\u003c\":null}"),
        (json!("'&>"), "\"\\u0027\\u0026\\u003e\""),
    ] {
        let source = captured(&[("value.html", b"{{ value | json_script }}")]);
        let limits = Limits {
            output_bytes: expected.len(),
            ..Limits::default()
        };
        let exact = TemplateSet::compile(&source, limits).expect("exact output bound");
        assert_eq!(
            output(&exact, "value.html", json!({"value":value})).expect("exact bytes"),
            expected
        );
        let smaller = TemplateSet::compile(
            &source,
            Limits {
                output_bytes: expected.len() - 1,
                ..limits
            },
        )
        .expect("smaller bound");
        assert!(matches!(
            output(&smaller, "value.html", json!({"value":value})),
            Err(Error::Limit("output bytes"))
        ));
    }
}

#[test]
fn arguments_and_missing_values_fail_without_poisoning_valid_renders() {
    let source = captured(&[
        ("argument.html", b"{{ value | json_script(pretty=true) }}"),
        (
            "context.html",
            b"{{ value | json_script(context='attribute') }}",
        ),
        ("missing.html", b"{{ absent | json_script }}"),
        ("value.html", b"{{ value | json_script }}"),
    ]);
    let set = TemplateSet::compile(&source, Limits::default()).expect("valid filter syntax");
    for name in ["argument.html", "context.html", "missing.html"] {
        let error = output(&set, name, json!({"value":"private-input-canary"}))
            .expect_err("no arguments or inferred missing values");
        let Error::Template(renderer) = error else {
            panic!("ordinary renderer error was misclassified as a resource limit")
        };
        assert!(
            has_native_tera_cause(renderer.as_ref()),
            "retain native diagnostic identity"
        );
        assert!(!renderer.to_string().contains("private-input-canary"));
        assert_eq!(
            output(&set, "value.html", json!({"value":null})).expect("subsequent exact value"),
            "null"
        );
    }
}
