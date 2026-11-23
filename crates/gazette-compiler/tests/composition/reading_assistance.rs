//! Reader handoffs remain an optional bounded chrome selection, never provider execution.

use super::*;
use gazette_compiler::{
    toml::PathSegment,
    workspace::{CompositionOriginQuery, ParsedWorkspace, ReadingAssistance},
};
use serde_json::{Value, json};

const CHROME: &str = include_str!("../fixtures/site_chrome.toml");
const ASSISTANCE: &str = include_str!("../fixtures/reading_assistance.toml");

fn source(assistance: &str) -> String {
    [CHROME, assistance, OWNER, ALPHA, BETA].concat()
}

fn value() -> Value {
    json!({
        "label":"Ask about this passage",
        "prompt_template":"{selection}\n{article_url}\n{site_name}\n{site_url}",
        "selection_limit":4096,
        "context_limit":1024,
        "providers":[{"id":"assistant:chosen", "label":"Ask a chosen assistant",
            "endpoint":"https://assistant.example/new?mode=reading", "query_parameter":"prompt"}]
    })
}

fn declared(value: &Value) -> String {
    let quote = |v: &Value| serde_json::to_string(v).unwrap();
    let mut text = String::from("[composition.chrome.reading_assistance]\n");
    for field in [
        "label",
        "prompt_template",
        "selection_limit",
        "context_limit",
    ] {
        text.push_str(&format!("{field}={}\n", quote(&value[field])));
    }
    if value["providers"].as_array().unwrap().is_empty() {
        text.push_str("providers=[]\n");
    }
    for provider in value["providers"].as_array().unwrap() {
        text.push_str("[[composition.chrome.reading_assistance.providers]]\n");
        for field in ["id", "label", "endpoint", "query_parameter"] {
            text.push_str(&format!("{field}={}\n", quote(&provider[field])));
        }
    }
    text
}

#[test]
fn assistance_inherits_with_chrome_and_retains_nested_original_spans() {
    let local_chrome = CHROME.replace("[composition.", "[sites.composition.");
    let source = [CHROME, ASSISTANCE, OWNER, ALPHA, BETA, &local_chrome].concat();
    let parsed = ParsedWorkspace::parse(&source, Default::default()).unwrap();
    let alpha = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:alpha"))
        .unwrap();
    let inherited = alpha.chrome().unwrap();
    assert_eq!(inherited.origin().unwrap().scope, SelectionScope::Workspace);
    assert_eq!(
        inherited
            .value()
            .reading_assistance
            .as_ref()
            .unwrap()
            .providers[0]
            .id,
        key("assistant:chosen")
    );
    let beta = parsed
        .compilation()
        .workspace
        .composition_for(&key("site:beta"))
        .unwrap();
    assert_eq!(
        beta.chrome().unwrap().origin().unwrap().scope,
        SelectionScope::Site(key("site:beta"))
    );
    assert!(beta.chrome().unwrap().value().reading_assistance.is_none());
    let origin = parsed
        .composition_origin(&CompositionOriginQuery {
            site: key("site:alpha"),
            field: SelectionField::Chrome {},
            path: vec![
                PathSegment::Key("reading_assistance".into()),
                PathSegment::Key("providers".into()),
                PathSegment::Index(0),
                PathSegment::Key("endpoint".into()),
            ],
        })
        .unwrap();
    assert_eq!(
        &source[origin.span.start..origin.span.end],
        "'https://assistant.example/new?mode=reading'"
    );
    assert_eq!(origin.selection, *inherited.origin().unwrap());
    let absent = serde_json::to_value(beta.chrome().unwrap().value()).unwrap();
    assert!(absent.get("reading_assistance").is_none());
}

#[test]
fn template_slots_and_unicode_limits_are_validated_without_byte_truncation() {
    let mut boundary = value();
    let required = boundary["prompt_template"].as_str().unwrap();
    boundary["prompt_template"] = json!(format!(
        "{required}{}",
        "🦉".repeat(4096 - required.chars().count())
    ));
    boundary["label"] = json!("🦉".repeat(256));
    boundary["providers"][0]["label"] = json!("🦉".repeat(256));
    boundary["context_limit"] = json!(0);
    compile(&source(&declared(&boundary)));
    let mut over = boundary.clone();
    over["prompt_template"] = json!(format!("{}🦉", over["prompt_template"].as_str().unwrap()));
    assert!(
        invalid(&source(&declared(&over)))
            .iter()
            .any(|e| e.field.ends_with(".prompt_template"))
    );
    for template in [
        "{selection} {article_url} {site_name}",
        "{selection} {article_url} {site_url}",
        "{article_url} {site_name} {site_url}",
        "{selection} {article_url} {site_name} {site_url} {unknown}",
        "{selection} {article_url} {site_name} {site_url} {",
        "{selection} {article_url} {site_name} {site_url} }",
        "{{selection}} {article_url} {site_name} {site_url}",
        "{selection} {article_url} {site_name} {site_url}\u{7}",
    ] {
        let mut candidate = value();
        candidate["prompt_template"] = json!(template);
        assert!(
            invalid(&source(&declared(&candidate)))
                .iter()
                .any(|e| e.code == "workspace_reading_assistance_template"),
            "{template:?}"
        );
    }
    for (field, bad) in [
        ("selection_limit", 0),
        ("selection_limit", 4097),
        ("context_limit", 1025),
    ] {
        let mut candidate = value();
        candidate[field] = json!(bad);
        assert!(
            invalid(&source(&declared(&candidate)))
                .iter()
                .any(|e| e.field.ends_with(field))
        );
    }
}

#[test]
fn provider_destinations_are_explicit_unique_and_safe_query_targets() {
    for endpoint in [
        "http://assistant.example/",
        "javascript:alert(1)",
        "https://user@assistant.example/",
        "https://@assistant.example/",
        "https://assistant.example/#fragment",
        "https://assistant.example/?prompt=competing",
        "https://assistant.example/?%70rompt=competing",
        " https://assistant.example/",
    ] {
        let mut candidate = value();
        candidate["providers"][0]["endpoint"] = json!(endpoint);
        assert!(
            invalid(&source(&declared(&candidate)))
                .iter()
                .any(|e| e.field.ends_with(".endpoint")),
            "{endpoint}"
        );
    }
    for parameter in ["", "a.b", "1prompt", "p&extra", "prömpt"] {
        let mut candidate = value();
        candidate["providers"][0]["query_parameter"] = json!(parameter);
        assert!(
            invalid(&source(&declared(&candidate)))
                .iter()
                .any(|e| e.field.ends_with(".query_parameter"))
        );
    }
    let mut duplicate = value();
    let repeated = duplicate["providers"][0].clone();
    duplicate["providers"]
        .as_array_mut()
        .unwrap()
        .push(repeated);
    assert!(
        invalid(&source(&declared(&duplicate)))
            .iter()
            .any(|e| e.field.ends_with("providers[1].id"))
    );
    for count in [0, 9] {
        let mut candidate = value();
        candidate["providers"] = json!(
            (0..count)
                .map(|n| {
                    let mut p = value()["providers"][0].clone();
                    p["id"] = json!(format!("assistant:p{n}"));
                    p
                })
                .collect::<Vec<_>>()
        );
        assert!(
            invalid(&source(&declared(&candidate)))
                .iter()
                .any(|e| e.field.ends_with(".providers"))
        );
    }
    let mut custom = value();
    custom["providers"][0]["query_parameter"] = json!("Input_2-prompt");
    compile(&source(&declared(&custom)));
}

#[test]
fn schemas_and_parser_share_the_optional_closed_shape() {
    let schema = serde_json::to_value(schemars::schema_for!(ReadingAssistance)).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(schema["properties"]["selection_limit"]["maximum"], 4096);
    assert_eq!(schema["properties"]["context_limit"]["maximum"], 1024);
    assert_eq!(schema["properties"]["providers"]["maxItems"], 8);
    assert_eq!(
        schema["$defs"]["ReadingAssistanceProvider"]["additionalProperties"],
        false
    );
    assert_eq!(
        schema["$defs"]["ReadingAssistanceProvider"]["properties"]["query_parameter"]["pattern"],
        "^[A-Za-z][A-Za-z0-9_-]{0,63}$"
    );
    let mut unknown = value();
    unknown["api_key"] = json!("not-a-supported-field");
    assert!(serde_json::from_value::<ReadingAssistance>(unknown).is_err());
    let mut unknown = value();
    unknown["providers"][0]["script"] = json!("not-a-supported-field");
    assert!(serde_json::from_value::<ReadingAssistance>(unknown).is_err());
    let mut unnamespaced = value();
    unnamespaced["providers"][0]["id"] = json!("chosen");
    assert!(serde_json::from_value::<ReadingAssistance>(unnamespaced).is_err());
}

#[test]
fn inherited_assistance_is_charged_before_expansion_into_sites() {
    let mut candidate = value();
    candidate["prompt_template"] = json!(format!(
        "{}{}",
        candidate["prompt_template"].as_str().unwrap(),
        "x".repeat(3500)
    ));
    let mut source = [CHROME, &declared(&candidate), OWNER].concat();
    for n in 0..20 {
        source.push_str(&format!(
            "[[sites]]\nid='site:s{n}'\npublisher='owner:one'\ndeployable=false\n"
        ));
    }
    let error = workspace::compile(
        &source,
        toml::Limits {
            source_bytes: source.len(),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        workspace::Error::Limit("composition text bytes")
    ));
}
