use super::*;

#[test]
fn editorial_dates_keep_authored_days_offsets_and_html_escaping() {
    let source = captured(&[(
        "page.html",
        br#"<time datetime="{{ date }}">{{ date | editorial_date(format=format) }}</time>"#,
    )]);
    let set = TemplateSet::compile(&source, Limits::default()).unwrap();
    for (date, format, expected) in [
        ("2024-02-29", "%-d %B %Y", "29 February 2024"),
        (
            "2026-01-01T00:15:00+14:00",
            "%-d %B %Y %H:%M %:z",
            "1 January 2026 00:15 +14:00",
        ),
        ("2025-12-31T23:45:00-12:00", "%-d %B %Y", "31 December 2025"),
        ("2026-11-19", "<b>%Y</b>", "&lt;b&gt;2026&lt;&#x2F;b&gt;"),
    ] {
        assert_eq!(
            output(&set, "page.html", json!({"date": date, "format": format})).unwrap(),
            format!("<time datetime=\"{date}\">{expected}</time>")
        );
    }
}

#[test]
fn editorial_date_refuses_invented_precision_and_invalid_values() {
    let source = captured(&[("page.html", b"{{ date | editorial_date(format=format) }}")]);
    let set = TemplateSet::compile(&source, Limits::default()).unwrap();
    for (date, format) in [
        (json!("2026-11-19"), json!("%H:%M")),
        (json!("2026-11-19"), json!("%:z")),
        (json!("2026-11-19"), json!("%Q")),
        (json!("2026-02-30"), json!("%Y")),
        (json!("2026-11-19T10:00:00"), json!("%Y")),
        (json!(null), json!("%Y")),
        (json!(2026), json!("%Y")),
        (json!("2026-11-19"), json!(false)),
    ] {
        assert!(output(&set, "page.html", json!({"date":date,"format":format})).is_err());
    }
}

#[test]
fn editorial_date_bounds_hidden_expansion_and_can_be_reused_after_failure() {
    let source = captured(&[(
        "page.html",
        b"{% set text = date | editorial_date(format=format) %}{{ text }}",
    )]);
    let set = TemplateSet::compile(
        &source,
        Limits {
            output_bytes: 4,
            ..Limits::default()
        },
    )
    .unwrap();
    assert!(matches!(
        output(
            &set,
            "page.html",
            json!({"date":"2026-11-19", "format":"%Y%Y"})
        ),
        Err(Error::Limit("output bytes"))
    ));
    assert_eq!(
        output(
            &set,
            "page.html",
            json!({"date":"2026-11-19", "format":"%Y"})
        )
        .unwrap(),
        "2026"
    );
}
