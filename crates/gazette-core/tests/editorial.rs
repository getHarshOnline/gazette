//! Explicit editorial facts through native date parsing and the whole content graph.

use std::cmp::Ordering;

use gazette_core::publication::content::{
    ContentNode, Declaration, EditorialDate, Error, Graph, Limits,
};
use serde_json::{Value, json};

fn node(fields: Value) -> ContentNode {
    let mut value = json!({
        "id":"content:one", "site":"site:one", "title":"Title",
        "lifecycle":"draft", "structure":{"kind":"page","family":{"kind":"article"}}
    });
    value
        .as_object_mut()
        .expect("object")
        .extend(fields.as_object().expect("fields").clone());
    serde_json::from_value(value).expect("typed content node")
}

fn declaration(node: ContentNode) -> Declaration {
    Declaration {
        browsing: vec![],
        nodes: vec![node],
        memberships: vec![],
        related: vec![],
        projects: vec![],
    }
}

#[test]
fn explicit_dates_round_trip_with_calendar_precision_offsets_and_lossless_fractions() {
    for (input, canonical, date_only) in [
        ("2000-02-29", "2000-02-29", true),
        ("0001-01-01", "0001-01-01", true),
        ("9999-12-31", "9999-12-31", true),
        (
            "2024-02-29T00:00:00+14:00",
            "2024-02-29T00:00:00+14:00",
            false,
        ),
        (
            "2024-02-29T00:00:00-14:00",
            "2024-02-29T00:00:00-14:00",
            false,
        ),
        ("2024-02-29", "2024-02-29", true),
        ("2024-02-29T23:59:59+00:00", "2024-02-29T23:59:59Z", false),
        ("2024-02-29T23:59:59.1Z", "2024-02-29T23:59:59.100Z", false),
        (
            "2024-02-29T23:59:59.1234Z",
            "2024-02-29T23:59:59.123400Z",
            false,
        ),
        (
            "2024-02-29T23:59:59.1234567+05:30",
            "2024-02-29T23:59:59.123456700+05:30",
            false,
        ),
        (
            "2024-02-29T23:59:59.123456789-03:30",
            "2024-02-29T23:59:59.123456789-03:30",
            false,
        ),
        ("2024-02-29T23:59:59.000Z", "2024-02-29T23:59:59Z", false),
    ] {
        let date: EditorialDate = input.parse().expect("explicit valid date");
        assert_eq!(date.is_date_only(), date_only, "{input}");
        assert_eq!(
            date.as_date().map(ToString::to_string),
            date_only.then(|| canonical.to_owned()),
            "{input}"
        );
        assert_eq!(date.to_string(), canonical, "{input}");
        assert_eq!(serde_json::to_value(&date).expect("serialize"), canonical);
        assert_eq!(
            serde_json::from_value::<EditorialDate>(json!(input)).expect("deserialize"),
            date
        );
        assert_eq!(
            canonical
                .parse::<EditorialDate>()
                .expect("canonical round trip"),
            date
        );
    }
}

#[test]
fn invalid_calendar_and_unsupported_lexical_forms_fail_without_provider_normalization() {
    for input in [
        "",
        "2023-02-29",
        "1900-02-29",
        "0000-01-01",
        "0000-01-01T00:00:00Z",
        "2024-01-01T00:00:00+14:01",
        "2024-01-01T00:00:00-14:01",
        "2024-01-01T00:00:00+23:59",
        "2024-04-31",
        "2024-00-01",
        "2024-13-01",
        "2024-01-00",
        "2024-2-09",
        "2024-02-9",
        "+2024-02-29",
        "02024-02-29",
        " 2024-02-29",
        "2024-02-29\n",
        "2024-02",
        "２０２４-02-29",
        "2024-02-29T23:59:59",
        "2024-02-29 23:59:59Z",
        "2024-02-29t23:59:59Z",
        "2024-02-29T23:59:59z",
        "2024-02-29T23:59:59-00:00",
        "2024-02-29T23:59:60Z",
        "2024-02-29T23:59:61Z",
        "2024-02-29T24:00:00Z",
        "2024-02-29T00:60:00Z",
        "2024-02-29T00:00:00+24:00",
        "2024-02-29T00:00:00+01:60",
        "2024-02-29T00:00:00−01:00",
        "2024-02-29T00:00:00+0100",
        "2024-02-29T00:00:00.Z",
        "2024-02-29T00:00:00.1234567890Z",
        "2024-02-29T00:00:00,123Z",
        "2024-02-29T00:00:00Zjunk",
        "2024-02-29T00:00:00Z ",
        "2024-02-29T00:00:00Z\0",
    ] {
        assert!(input.parse::<EditorialDate>().is_err(), "parsed {input:?}");
        assert!(
            serde_json::from_value::<EditorialDate>(json!(input)).is_err(),
            "decoded {input:?}"
        );
    }
    for input in [
        json!(null),
        json!(20240229),
        json!(true),
        json!(["2024-02-29"]),
        json!({"date":"2024-02-29"}),
    ] {
        assert!(
            serde_json::from_value::<EditorialDate>(input.clone()).is_err(),
            "non-string {input}"
        );
    }
}

#[test]
fn chronological_comparison_does_not_erase_precision_or_explicit_offset_identity() {
    let date = "2024-03-01".parse::<EditorialDate>().expect("date");
    let yesterday = "2024-02-29".parse::<EditorialDate>().expect("leap day");
    let utc = "2024-03-01T00:00:00Z"
        .parse::<EditorialDate>()
        .expect("UTC");
    let east = "2024-03-01T05:30:00+05:30"
        .parse::<EditorialDate>()
        .expect("same instant");
    let west = "2024-02-29T20:30:00-03:30"
        .parse::<EditorialDate>()
        .expect("same instant");
    let later = "2024-03-01T00:00:00.000000001Z"
        .parse::<EditorialDate>()
        .expect("next nanosecond");
    assert_eq!(date.chronological_cmp(&yesterday), Some(Ordering::Greater));
    assert_eq!(utc.chronological_cmp(&east), Some(Ordering::Equal));
    assert_eq!(utc.chronological_cmp(&west), Some(Ordering::Equal));
    assert_ne!(utc, east);
    assert_ne!(utc, west);
    assert_eq!(utc.chronological_cmp(&later), Some(Ordering::Less));
    assert_eq!(date.chronological_cmp(&utc), None);
    assert_eq!(utc.chronological_cmp(&date), None);
    assert_eq!(
        utc,
        "2024-03-01T00:00:00+00:00"
            .parse::<EditorialDate>()
            .expect("known zero offset")
    );
}

#[test]
fn graph_preserves_independent_editorial_facts_without_lifecycle_or_missing_value_inference() {
    for lifecycle in ["draft", "published", "archived"] {
        let plain = node(json!({"lifecycle":lifecycle}));
        let graph = Graph::resolve(declaration(plain.clone()), Limits::default())
            .expect("omitted chronology");
        let serialized =
            serde_json::to_value(graph.node(&plain.id).expect("retained node")).expect("serialize");
        for field in ["description", "published", "modified"] {
            assert!(
                serialized.get(field).is_none(),
                "inferred {field} for {lifecycle}"
            );
        }
    }
    let facts = node(
        json!({"description":"  Prose\nwith\r\ntabs\tand **literal** markup.  ",
        "published":"2025-01-01", "modified":"2024-01-01T00:00:00Z"}),
    );
    let graph = Graph::resolve(declaration(facts.clone()), Limits::default())
        .expect("independent dates, not an ordering constraint");
    assert_eq!(graph.node(&facts.id), Some(&facts));
}

#[test]
fn description_validation_rejects_empty_or_control_text_with_exact_graph_diagnostic() {
    for description in [
        "",
        " \r\n\t",
        "nul\0text",
        "bell\u{7}",
        "delete\u{7f}",
        "next\u{85}line",
    ] {
        let input = node(json!({"description":description}));
        let Error::Invalid(errors) = Graph::resolve(declaration(input.clone()), Limits::default())
            .expect_err("invalid description")
        else {
            panic!("expected semantic description diagnostic");
        };
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, "content_description");
        assert_eq!(errors[0].source, input.id);
        assert_eq!(errors[0].target, None);
    }
}

#[test]
fn graph_text_budget_counts_utf8_description_and_canonical_date_bytes_exactly() {
    let input = node(json!({"description":"नमस्ते\nText", "published":"2024-02-29",
        "modified":"2024-02-29T00:00:00.1+05:30"}));
    let bytes = "content:one".len()
        + "site:one".len()
        + "Title".len()
        + "नमस्ते\nText".len()
        + "2024-02-29".len()
        + "2024-02-29T00:00:00.100+05:30".len();
    Graph::resolve(
        declaration(input.clone()),
        Limits {
            text_bytes: bytes,
            ..Default::default()
        },
    )
    .expect("exact canonical byte budget");
    assert!(matches!(
        Graph::resolve(
            declaration(input),
            Limits {
                text_bytes: bytes - 1,
                ..Default::default()
            }
        ),
        Err(Error::Limit("text bytes"))
    ));
}
