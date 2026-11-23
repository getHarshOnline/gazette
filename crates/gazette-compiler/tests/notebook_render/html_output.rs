//! Stored inert HTML exercises the public notebook path, not a tokenizer-only fixture.

use super::*;

fn stored(source: Value) -> Value {
    json!({"output_type":"display_data","metadata":{},"data":{"text/html":source,"text/plain":"Independent alternative"}})
}

fn render(
    source: Value,
    limits: markdown::Limits,
) -> Result<notebook::RenderedNotebook, RenderError> {
    let bytes = encoded(&document(vec![code_cell(
        json!("NEVER_EXECUTE()"),
        vec![stored(source)],
    )]));
    ParsedNotebook::parse(&bytes, Default::default())
        .unwrap()
        .render_static(
            "html",
            limits,
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
}

#[test]
fn inert_html_preserves_tables_lists_entities_and_retained_alternatives() {
    let parts = [
        "<div class=\"report\"><p>हर्ष &amp; Δ <strong>result</strong></p>",
        "<ul><li>First <code>x &lt; 2</code></li><li>Second<br>line</li></ul>",
        "<table class=\"dataframe\"><caption>Measurements</caption><thead><tr><th scope=\"col\">Name</th><th scope=\"col\">Value</th></tr></thead>",
        "<tbody><tr><th scope=\"row\">α</th><td colspan=\"1\" rowspan=\"1\">42</td></tr></tbody><tfoot><tr><td colspan=\"2\">Complete</td></tr></tfoot></table></div>",
    ];
    let source = parts.concat();
    let mut outputs = Vec::new();
    for representation in [json!(source), json!(parts)] {
        let value = document(vec![code_cell(
            json!("NEVER_EXECUTE()"),
            vec![stored(representation)],
        )]);
        let bytes = encoded(&value);
        let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
        let output = parsed
            .render_static(
                "html",
                Default::default(),
                &raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert!(output.html.contains(&source));
        for text in [
            "हर्ष & Δ",
            "result",
            "First",
            "x < 2",
            "Second",
            "line",
            "Measurements",
            "Name",
            "Value",
            "α",
            "42",
            "Complete",
            "Independent alternative",
        ] {
            assert!(
                output.text.contains(text),
                "missing searchable meaning: {text}"
            );
        }
        assert!(!output.text.contains("<table"));
        assert!(!output.text.contains("&amp;"));
        assert_eq!(parsed.source(), bytes.as_slice());
        assert_eq!(parsed.value(), &value);
        assert_eq!(output.source_sha256, Sha256Digest::of(&bytes));
        assert_eq!(
            output,
            parsed
                .render_static(
                    "html",
                    Default::default(),
                    &raster::unavailable,
                    &gazette_adapter_markdown::Comrak
                )
                .unwrap()
        );
        outputs.push(output);
    }
    assert_eq!(outputs[0].html, outputs[1].html);
    assert_eq!(outputs[0].text, outputs[1].text);
    assert_ne!(outputs[0].source_sha256, outputs[1].source_sha256);
}

#[test]
fn active_lossy_malformed_and_browser_repaired_html_never_falls_back() {
    for source in [
        "<script>alert(1)</script>",
        "<iframe></iframe>",
        "<svg></svg>",
        "<math></math>",
        "<img src=\"x\">",
        "<a href=\"https://example.invalid\">link</a>",
        "<div style=\"display:none\">hidden</div>",
        "<p onclick=\"run()\">text</p>",
        "<p id=\"foreign\">text</p>",
        "<p data-value=\"private\">text</p>",
        "<div xmlns=\"other\">foreign</div>",
        "<p class=\"a\" class=\"b\">duplicate</p>",
        "<p>unclosed",
        "<p><div>repaired</div></p>",
        "<b><i>misnested</b></i>",
        "<p/>",
        "</br>",
        "<!-- omitted --><p>text</p>",
        "<!doctype html><p>text</p>",
        "<p>&broken;</p>",
        "<p>nul\0text</p>",
        "<table>fostered text</table>",
        "<table><tr><td>implicit body</td></tr></table>",
        "<td>orphan</td>",
        "<ul><div>not an item</div></ul>",
        "<ul>uncontained text</ul>",
        "<table><tbody><tr><td><table></table></td></tr></tbody></table>",
        "<table><tbody></tbody><thead></thead></table>",
        "<table><caption>one</caption><caption>two</caption></table>",
        "<table><tfoot></tfoot><tbody></tbody></table>",
        "<table><tbody><tr><td colspan=\"0\">bad span</td></tr></tbody></table>",
        "<table><tbody><tr><td rowspan=\"1001\">large span</td></tr></tbody></table>",
        "<table><tbody><tr><th scope=\"invalid\">bad scope</th></tr></tbody></table>",
        "<table><tbody><tr><td scope=\"row\">wrong cell</td></tr></tbody></table>",
    ] {
        assert!(
            matches!(
                render(json!(source), Default::default()),
                Err(RenderError::Unsupported {
                    cell: Some(0),
                    feature: "HTML representation"
                })
            ),
            "must refuse full result: {source:?}"
        );
    }
}

#[test]
fn rich_unsupported_alternative_is_not_hidden_by_supported_html() {
    let mut output = stored(json!("<p>Valid HTML first</p>"));
    output["data"]["image/svg+xml"] = json!("<svg/>");
    let bytes = encoded(&document(vec![code_cell(json!(""), vec![output])]));
    let parsed = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
    assert!(matches!(
        parsed.render_static(
            "html",
            Default::default(),
            &raster::unavailable,
            &gazette_adapter_markdown::Comrak
        ),
        Err(RenderError::Unsupported {
            feature: "MIME representation",
            ..
        })
    ));
}

#[test]
fn stored_html_token_depth_decoded_and_output_limits_refuse_complete_result() {
    let source = "<div><p>Decoded &amp; retained</p></div>";
    for limits in [
        markdown::Limits {
            nodes: 2,
            ..Default::default()
        },
        markdown::Limits {
            depth: 1,
            ..Default::default()
        },
        markdown::Limits {
            fact_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(matches!(
            render(json!(source), limits),
            Err(RenderError::Limit(_))
        ));
    }
    let successful = render(json!(source), Default::default()).unwrap();
    let limits = markdown::Limits {
        output_bytes: successful.html.len().max(successful.text.len()) - 1,
        ..Default::default()
    };
    assert!(matches!(
        render(json!(source), limits),
        Err(RenderError::Limit(_))
    ));
}

#[test]
fn utf8_entities_and_rejected_tail_across_tokenizer_feed_boundaries() {
    let source = format!("<p>{}हर्ष &amp; preserved</p>", "x".repeat(249));
    let output = render(json!(source), Default::default()).unwrap();
    assert!(output.text.contains("हर्ष & preserved"));
    assert!(output.html.contains("हर्ष &amp; preserved"));
    let poisoned = format!("{source}<script>late effect</script>");
    assert!(matches!(
        render(json!(poisoned), Default::default()),
        Err(RenderError::Unsupported {
            feature: "HTML representation",
            ..
        })
    ));
    let entity_boundary = format!("<p>{}ह&amp; preserved</p>", "x".repeat(247));
    let output = render(json!(entity_boundary), Default::default()).unwrap();
    assert!(output.text.contains("ह& preserved"));
    assert!(output.html.contains("ह&amp; preserved"));
}

#[test]
fn unfinished_html_token_has_its_own_native_buffering_ceiling() {
    let source = format!("<div class=\"{}", "x".repeat(17 << 10));
    assert!(matches!(
        render(json!(source), Default::default()),
        Err(RenderError::Limit("stored HTML unemitted input bytes"))
    ));
    // The same aggregate text size is allowed when the tokenizer emits bounded
    // text chunks; this is not a second whole-document size limit.
    let source = format!("<p>{}</p>", "x".repeat(17 << 10));
    assert!(render(json!(source), Default::default()).is_ok());
}
