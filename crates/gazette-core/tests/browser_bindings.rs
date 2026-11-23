//! Public client bindings preserve literal identity without asserting provider readiness.

use gazette_core::publication::{
    CapabilityId, CoreCapabilityCatalog,
    browser::{
        GoogleMeasurementId, InvalidGoogleMeasurementId, InvalidWebMcpTools, WebMcpTool,
        WebMcpTools,
    },
    capability_runtime::{CapabilityValue, SearchIndexFormat},
    routing::RoutePath,
};
use serde_json::{Value, json};

fn definition<'a>(root: &'a Value, value: &'a Value) -> &'a Value {
    match value.get("$ref").and_then(Value::as_str) {
        Some(reference) => root
            .pointer(
                reference
                    .strip_prefix('#')
                    .expect("native local schema reference"),
            )
            .expect("native schema definition"),
        None => value,
    }
}

#[test]
fn ga4_literal_preserves_scalar_serialization_and_exact_native_bounds() {
    for literal in [
        "G-A".to_owned(),
        "G-012ABC".to_owned(),
        format!("G-{}", "A".repeat(62)),
    ] {
        let id: GoogleMeasurementId = literal.parse().expect("explicit GA4 literal");
        assert_eq!(id.as_str(), literal);
        assert_eq!(
            GoogleMeasurementId::try_from(literal.clone()),
            Ok(id.clone())
        );
        assert_eq!(
            serde_json::to_value(&id).expect("scalar binding"),
            json!(literal)
        );
        assert_eq!(
            serde_json::from_value::<GoogleMeasurementId>(json!(literal))
                .expect("checked scalar decoding"),
            id
        );
    }
    assert_eq!(
        format!("G-{}", "A".repeat(63)).parse::<GoogleMeasurementId>(),
        Err(InvalidGoogleMeasurementId)
    );
}

#[test]
fn malformed_lowercase_controls_and_non_scalar_ids_cannot_enter() {
    for literal in [
        "",
        "G-",
        "g-A",
        "G-a",
        "GT-ABC",
        "UA-123",
        "G-ABC-1",
        "G-ABC_1",
        " G-ABC",
        "G-ABC ",
        "G-A\n",
        "G-A\r\n",
        "G-A\0",
        "G-\tA",
        "G-É",
        "G-🦀",
        "G-A\u{2028}",
        "G-A\u{2029}",
    ] {
        assert_eq!(
            literal.parse::<GoogleMeasurementId>(),
            Err(InvalidGoogleMeasurementId)
        );
        assert_eq!(
            GoogleMeasurementId::try_from(literal.to_owned()),
            Err(InvalidGoogleMeasurementId)
        );
        assert!(
            serde_json::from_value::<GoogleMeasurementId>(json!(literal)).is_err(),
            "{literal:?}"
        );
    }
    for value in [
        json!(null),
        json!(1),
        json!(true),
        json!(["G-ABC"]),
        json!({"id":"G-ABC"}),
    ] {
        assert!(serde_json::from_value::<GoogleMeasurementId>(value).is_err());
    }
}

#[test]
fn ga4_schema_has_the_same_literal_ascii_and_length_boundary() {
    let schema =
        serde_json::to_value(schemars::schema_for!(GoogleMeasurementId)).expect("native schema");
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["pattern"], "^G-[A-Z0-9]+$");
    assert_eq!(schema["maxLength"], 64);
    // A portable dollar anchor may match before a final newline. The whole-string
    // exclusion must therefore agree with native rejection of every control.
    assert_eq!(schema["not"], json!({"pattern":"[^A-Z0-9-]"}));
    for byte in 0_u8..=127 {
        let literal = format!("G-{}", char::from(byte));
        let accepted = byte.is_ascii_uppercase() || byte.is_ascii_digit();
        assert_eq!(
            literal.parse::<GoogleMeasurementId>().is_ok(),
            accepted,
            "{literal:?}"
        );
        assert_eq!(
            serde_json::from_value::<GoogleMeasurementId>(json!(literal)).is_ok(),
            accepted
        );
    }
}

#[test]
fn selected_read_only_tools_serialize_as_a_list_in_caller_order() {
    for (selected, serialized) in [
        (vec![WebMcpTool::CurrentPage], json!(["current_page"])),
        (vec![WebMcpTool::Search], json!(["search"])),
        (
            vec![WebMcpTool::CurrentPage, WebMcpTool::Search],
            json!(["current_page", "search"]),
        ),
        (
            vec![WebMcpTool::Search, WebMcpTool::CurrentPage],
            json!(["search", "current_page"]),
        ),
    ] {
        let tools = WebMcpTools::try_from(selected.clone()).expect("explicit unique subset");
        assert_eq!(tools.as_slice(), selected);
        assert_eq!(
            serde_json::to_value(&tools).expect("list binding"),
            serialized
        );
        assert_eq!(
            serde_json::from_value::<WebMcpTools>(serialized).expect("checked list"),
            tools
        );
    }
}

#[test]
fn empty_duplicate_overlong_and_effectful_tool_selections_are_refused() {
    for selected in [
        vec![],
        vec![WebMcpTool::CurrentPage, WebMcpTool::CurrentPage],
        vec![WebMcpTool::Search, WebMcpTool::Search],
        vec![
            WebMcpTool::CurrentPage,
            WebMcpTool::Search,
            WebMcpTool::CurrentPage,
        ],
    ] {
        assert_eq!(
            WebMcpTools::try_from(selected.clone()),
            Err(InvalidWebMcpTools)
        );
        assert!(
            serde_json::from_value::<WebMcpTools>(serde_json::to_value(selected).expect("list"))
                .is_err()
        );
    }
    for value in [
        json!(["subscribe"]),
        json!(["navigate"]),
        json!(["stage_rendered_site"]),
        json!(["CurrentPage"]),
        json!(["current-page"]),
        json!(["search", "unknown"]),
        json!(null),
        json!("search"),
        json!({"tools":["search"]}),
        json!([true]),
    ] {
        assert!(serde_json::from_value::<WebMcpTools>(value).is_err());
    }
}

#[test]
fn tool_schema_is_a_nonempty_unique_bounded_closed_subset() {
    let schema = serde_json::to_value(schemars::schema_for!(WebMcpTools)).expect("native schema");
    assert_eq!(schema["type"], "array");
    assert_eq!(schema["minItems"], 1);
    assert_eq!(schema["maxItems"], 2);
    assert_eq!(schema["uniqueItems"], true);
    let item = definition(&schema, &schema["items"]);
    // Native variant documentation is retained as separate literal alternatives.
    let alternatives = item["oneOf"]
        .as_array()
        .expect("documented tool alternatives");
    assert_eq!(alternatives.len(), 2);
    for (alternative, name) in alternatives.iter().zip(["current_page", "search"]) {
        assert_eq!(alternative["type"], "string");
        assert_eq!(alternative["const"], name);
    }
}

#[test]
fn capability_values_remain_distinct_tagged_catalog_owned_bindings() {
    let analytics = CapabilityValue::PublicationAnalyticsV1 {
        measurement_id: "G-ABC123".parse().expect("explicit tag"),
    };
    let webmcp = CapabilityValue::PublicationWebMcpV1 {
        tools: vec![WebMcpTool::Search, WebMcpTool::CurrentPage]
            .try_into()
            .expect("explicit selection"),
    };
    let search = CapabilityValue::PublicationSearchV1 {
        index: "/search.json".parse::<RoutePath>().expect("public route"),
        format: SearchIndexFormat::GazetteDocumentsV1,
    };
    assert_eq!(
        serde_json::to_value(&analytics).expect("analytics"),
        json!({
            "contract":"publication-analytics-v1", "measurement_id":"G-ABC123",
        })
    );
    assert_eq!(
        serde_json::to_value(&webmcp).expect("WebMCP"),
        json!({
            "contract":"publication-webmcp-v1", "tools":["search","current_page"],
        })
    );
    for (value, capability) in [
        (&analytics, CoreCapabilityCatalog::publication_analytics()),
        (&webmcp, CoreCapabilityCatalog::publication_webmcp()),
        (&search, CoreCapabilityCatalog::publication_search()),
    ] {
        assert_eq!(value.capability(), capability);
        assert!(CoreCapabilityCatalog::contains(&capability));
    }
    assert_ne!(analytics.capability(), webmcp.capability());
    assert!(
        CoreCapabilityCatalog::IDS
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    for name in [
        "publication.analytics/v2",
        "publication.webmcp/v2",
        "plugin.browser/v1",
    ] {
        let capability: CapabilityId = name.parse().expect("syntactically valid contract");
        assert!(!CoreCapabilityCatalog::contains(&capability));
    }

    let schema =
        serde_json::to_value(schemars::schema_for!(CapabilityValue)).expect("native schema");
    let alternatives = schema["oneOf"]
        .as_array()
        .expect("distinct tagged alternatives");
    let variant = |tag| {
        alternatives
            .iter()
            .find(|alternative| alternative["properties"]["contract"]["const"] == tag)
            .expect("native contract variant")
    };
    let analytics = variant(json!("publication-analytics-v1"));
    let webmcp = variant(json!("publication-webmcp-v1"));
    assert_eq!(analytics["additionalProperties"], false);
    assert_eq!(webmcp["additionalProperties"], false);
    assert_eq!(analytics["required"], json!(["contract", "measurement_id"]));
    assert_eq!(webmcp["required"], json!(["contract", "tools"]));
    assert!(analytics["properties"].get("tools").is_none());
    assert!(webmcp["properties"].get("measurement_id").is_none());
    assert_eq!(
        definition(&schema, &analytics["properties"]["measurement_id"])["type"],
        "string"
    );
    assert_eq!(
        definition(&schema, &webmcp["properties"]["tools"])["type"],
        "array"
    );
}
