//! Coordinates are obtained from native TOML syntax, including decoded keys and arrays.

use gazette_compiler::toml::{Limits, ParsedToml, PathSegment};

fn path(value: serde_json::Value) -> Vec<PathSegment> {
    serde_json::from_value(value).expect("typed syntax path")
}

#[test]
fn quoted_keys_dotted_keys_unicode_arrays_and_tables_have_exact_original_ranges() {
    let source = "# 🦀 'different'\r\n'quoted.key' = 'Rêve'\r\na.b = [ { '0' = 'first' }, { '0' = 'different' } ]\r\n[[rows]]\r\nid = 'second'\r\n[[rows]]\r\nid = 'first'\r\n";
    let parsed = ParsedToml::parse(source, Limits::default()).expect("retained syntax");
    for (query, expected) in [
        (serde_json::json!(["quoted.key"]), "'Rêve'"),
        (serde_json::json!(["a", "b", 0, "0"]), "'first'"),
        (serde_json::json!(["a", "b", 1, "0"]), "'different'"),
        (serde_json::json!(["rows", 0, "id"]), "'second'"),
        (serde_json::json!(["rows", 1, "id"]), "'first'"),
    ] {
        let query = path(query);
        let span = parsed.span_at(&query).expect("native value span");
        assert_eq!(&source[span.start..span.end], expected);
        assert_eq!(parsed.span_at(&query), Some(span));
    }
    for query in [
        serde_json::json!(["quoted", "key"]),
        serde_json::json!(["a", "b", "0"]),
        serde_json::json!(["a", "b", 0, 0]),
        serde_json::json!(["rows", 2, "id"]),
        serde_json::json!(["rows", 0, "absent"]),
        serde_json::json!(["rows", 0, "id", "child"]),
    ] {
        assert_eq!(parsed.span_at(&path(query)), None);
    }
    assert_eq!(parsed.source(), source);
}

#[test]
fn inline_and_regular_arrays_share_the_same_location_operation() {
    let source = "rows=[{id='a', values=[[1,2],[3,4]]}, {id='b'}]";
    let parsed = ParsedToml::parse(source, Limits::default()).expect("inline syntax");
    let span = parsed
        .span_at(&path(serde_json::json!(["rows", 0, "values", 1, 0])))
        .expect("nested numeric value");
    assert_eq!(&source[span.start..span.end], "3");
    for invalid in [
        serde_json::json!([-1]),
        serde_json::json!([0.5]),
        serde_json::json!([true]),
    ] {
        assert!(serde_json::from_value::<Vec<PathSegment>>(invalid).is_err());
    }
}
