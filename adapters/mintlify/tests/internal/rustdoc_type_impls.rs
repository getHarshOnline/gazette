//! Literal data selection never executes the native registration wrapper.
use super::select;
use serde_json::{Value, json};

fn envelope(rows: Value) -> String {
    let entry = serde_json::to_string(&json!(["sample", rows])).unwrap();
    let prefix = "(function() {\n    var type_impls = Object.fromEntries(";
    format!(
        "{prefix}[{entry}]);\n    if (window.register_type_impls) {{\n        window.register_type_impls(type_impls);\n    }} else {{\n        window.pending_type_impls = type_impls;\n    }}\n}})()\n//{{\"start\":{},\"fragment_lengths\":[{}]}}",
        prefix.len() + 1,
        entry.len()
    )
}

#[test]
fn literal_rows_select_the_exact_crate_and_alias() {
    let data = envelope(json!([
        ["<p>First method</p>", 0, "sample::First", "sample::Second"],
        ["<p>First trait</p>", "Borrow", "sample::First"],
        ["<p>Second method</p>", 0, "sample::Second"]
    ]));
    let rows = select(data.as_bytes(), "sample", "sample::First", "fixture").unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].html, "<p>First method</p>");
    assert!(!rows[0].is_trait);
    assert!(rows[1].is_trait);
    for (name, path) in [("other", "sample::First"), ("sample", "sample::Missing")] {
        assert!(select(data.as_bytes(), name, path, "fixture").is_err());
    }
}

#[test]
fn invalid_rows_or_changed_envelopes_are_refused() {
    for row in [
        json!(["", 0, "sample::First"]),
        json!(["<p>Text</p>", 1, "sample::First"]),
        json!(["<p>Text</p>", "", "sample::First"]),
        json!(["<p>Text</p>", 0]),
        json!(["<p>Text</p>", 0, "sample::First", "sample::First"]),
        json!(["<p>Text</p>", 0, "sample::First", null]),
        json!(["<p>Text</p>", 0, "sample:: First"]),
    ] {
        assert!(
            select(
                envelope(json!([row])).as_bytes(),
                "sample",
                "sample::First",
                "fixture"
            )
            .is_err()
        );
    }
    let data = envelope(json!([["<p>Text</p>", 0, "sample::First"]]));
    for altered in [
        format!("{data}\nwindow.extra=true;"),
        data.replace("var type_impls", "const type_impls"),
        data.replace("\"start\":", "\"start\":9"),
    ] {
        if altered != data {
            assert!(select(altered.as_bytes(), "sample", "sample::First", "fixture").is_err());
        }
    }
}
