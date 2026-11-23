//! Embedded fragments reuse the native renderer without colliding local anchors.

use gazette_compiler::markdown::{Error, Limits, ParsedMarkdown};

#[test]
fn namespaced_heading_cannot_collide_with_provider_footnote_anchors_before_resolution() {
    for id in ["n-fnote-61", "nref-fnote-61"] {
        let source = format!(
            "# Heading {{#{id}}}\n\n[reference](https://example.test/) Note[^a].\n\n[^a]: Detail\n"
        );
        let parsed = ParsedMarkdown::parse(
            &source,
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("source syntax");
        let mut called = false;
        let result = parsed.render_in_namespace("f", |_, _, _| {
            called = true;
            Ok(None)
        });
        assert!(
            matches!(result, Err(Error::Anchor { .. })),
            "colliding {id} must fail"
        );
        assert!(!called, "collision must be found before caller resolution");
    }
}

#[test]
fn embedded_headings_links_footnotes_and_summary_share_one_namespace() {
    let source = "# Shared\n\n[local](#shared) [external](https://example.test/#shared)\n\nNote[^n].\n\n[^n]: Detail\n\n<!-- more -->\n\nTail\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("parse");
    let first = parsed
        .render_in_namespace("cell-one-", |_, _, _| Ok(None))
        .expect("first");
    let second = parsed
        .render_in_namespace("cell-two-", |_, _, _| Ok(None))
        .expect("second");
    assert!(first.body_html.contains("id=\"cell-one-shared\""));
    assert!(first.body_html.contains("href=\"#cell-one-shared\""));
    assert!(
        first
            .body_html
            .contains("href=\"https://example.test/#shared\"")
    );
    assert!(first.body_html.contains("href=\"#fn-cell-one-note-6e\""));
    assert!(first.body_html.contains("id=\"fn-cell-one-note-6e\""));
    assert!(!first.body_html.contains("cell-two-"));
    assert!(second.body_html.contains("id=\"cell-two-shared\""));
    let summary = first.summary.expect("summary");
    assert!(summary.continuation.starts_with("#cell-one-"));
    assert!(
        first
            .body_html
            .contains(&format!("id=\"{}\"", &summary.continuation[1..]))
    );
    assert!(!summary.html.contains("id="));
    assert_eq!(
        parsed
            .render_in_namespace("", |_, _, _| Ok(None))
            .expect("ordinary"),
        parsed
            .render_resolving_references(|_, _, _| Ok(None))
            .expect("same owner")
    );
}

#[test]
fn resolver_output_and_original_safety_are_preserved() {
    let parsed = ParsedMarkdown::parse(
        "[link](#one)",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("parse");
    let top = ParsedMarkdown::parse(
        "[top](#)",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("parse")
    .render_in_namespace("cell-", |_, _, _| Ok(None))
    .expect("top remains document top");
    assert!(top.body_html.contains("href=\"#\""));
    let mapped = parsed
        .render_in_namespace("cell-", |_, _, _| {
            Ok(Some("https://example.test/#one".into()))
        })
        .expect("mapping");
    assert!(
        mapped
            .body_html
            .contains("href=\"https://example.test/#one\"")
    );
    let unsafe_body = ParsedMarkdown::parse(
        "[link](javascript:alert)",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("syntax");
    assert!(matches!(
        unsafe_body.render_in_namespace("cell-", |_, _, _| panic!(
            "unsafe original must reject first"
        )),
        Err(Error::UnsafeUrl(_))
    ));
}

#[test]
fn namespace_validation_and_output_bounds_do_not_return_partial_fragments() {
    let parsed = ParsedMarkdown::parse(
        "# Title",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("parse");
    for invalid in ["quote\"", "a b", "<script>", "é", &"x".repeat(257)] {
        assert!(matches!(
            parsed.render_in_namespace(invalid, |_, _, _| Ok(None)),
            Err(Error::Namespace)
        ));
    }
    let maximum = parsed
        .render_in_namespace("cell-", |_, _, _| Ok(None))
        .expect("render")
        .body_html
        .len();
    let bounded = |limit| {
        ParsedMarkdown::parse(
            "# Title",
            Limits {
                output_bytes: limit,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak,
        )
        .expect("parse")
        .render_in_namespace("cell-", |_, _, _| Ok(None))
    };
    assert!(bounded(maximum).is_ok());
    assert!(matches!(bounded(maximum - 1), Err(Error::Limit(_))));
}
