//! Behavior at the real parser and independent projections, not helper snapshots.

#[path = "support/html_text.rs"]
mod html_text;

use gazette_compiler::markdown::{Error, Limits, LinkKind, ParsedMarkdown};
use gazette_core::source::{Precision, Sha256Digest};

#[test]
fn text_projection_preserves_retained_syntax_code_alt_and_block_boundaries() {
    let source = "# Title &amp; `code`\n\nA *bold* [label](https://example.test/private) ![alt](https://example.test/image.png)\\\nnext\nline.\n\n```txt\n[not](url) <tag>\n```\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("one native parse");
    let expected = "Title & code\nA bold label alt\nnext line.\n[not](url) <tag>\n\n";
    assert_eq!(parsed.text().expect("bounded text"), expected);
    assert_eq!(
        parsed.inspect().expect("same retained syntax").text,
        expected
    );
    assert_eq!(parsed.text().expect("repeat projection"), expected);
    let html = parsed.render_html().expect("independent HTML projection");
    assert!(html.contains("<em>bold</em>"));
    assert!(html.contains("alt=\"alt\""));
    assert!(html.contains("&lt;tag&gt;"));
    assert!(
        !expected.contains("https://"),
        "reference destinations are not visible text"
    );
}

#[test]
fn text_projection_charges_exact_utf8_output_independently_of_link_inspection_overhead() {
    let source = "[雪](https://example.test/a-long-private-destination \"private title\")";
    let expected = "雪\n";
    let parsed = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: expected.len(),
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native syntax parse");
    assert_eq!(parsed.text().expect("exact UTF-8 text budget"), expected);
    assert!(
        matches!(
            parsed.inspect(),
            Err(Error::Limit("inspection string bytes"))
        ),
        "text does not allocate or charge irrelevant link records"
    );
    let smaller = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: expected.len() - 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("native syntax remains valid");
    assert!(matches!(smaller.text(), Err(Error::Limit("text bytes"))));
    assert_eq!(
        ParsedMarkdown::parse(
            "",
            Limits {
                output_bytes: 0,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        )
        .expect("empty AST")
        .text()
        .expect("zero byte text"),
        ""
    );
}

#[test]
fn text_projection_preserves_raw_source_without_claiming_visible_html_or_admission() {
    let source = "before <b>bold</b> after\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("syntax");
    assert_eq!(parsed.text().expect("literal syntax text"), source);
    assert_eq!(parsed.inspect().expect("syntax evidence").text, source);
    assert!(
        matches!(parsed.render_html(), Err(Error::RawHtml(_))),
        "search text does not grant raw HTML rendering permission"
    );
}

#[test]
fn retained_source_drives_text_headings_resolved_references_and_escaped_html() {
    let source = "# Héllo *world* & `code`\n\n[ref][target] and ![alt](asset.png \"image\")\n\n[target]: https://example.invalid/a?q=1&x=2 \"title\"\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("parse");
    let report = parsed.inspect().expect("inspection");
    assert_eq!(report.source_sha256, Sha256Digest::of(source.as_bytes()));
    assert_eq!(report.headings.len(), 1);
    assert_eq!(report.headings[0].level, 1);
    assert_eq!(report.headings[0].text, "Héllo world & code");
    let heading = report.headings[0].span;
    assert_eq!(
        &source[heading.start..heading.end],
        "# Héllo *world* & `code`"
    );
    assert_eq!(report.headings[0].precision, Precision::Construct);
    assert_eq!(report.links.len(), 2);
    assert_eq!(report.links[0].kind, LinkKind::Link);
    assert_eq!(
        report.links[0].destination,
        "https://example.invalid/a?q=1&x=2"
    );
    assert_eq!(report.links[0].title, "title");
    assert_eq!(report.links[1].kind, LinkKind::Image);
    assert_eq!(report.links[1].destination, "asset.png");
    assert_eq!(report.text, "Héllo world & code\nref and alt\n");
    assert!(report.raw_html.is_empty());
    let html = parsed.render_html().expect("HTML");
    assert!(
        html.contains(
            "<h1 id=\"hello-world-code\">Héllo <em>world</em> &amp; <code>code</code></h1>"
        )
    );
    assert!(html.contains("href=\"https://example.invalid/a?q=1&amp;x=2\" title=\"title\""));
    assert!(html.contains("src=\"asset.png\" alt=\"alt\" title=\"image\""));
    assert_eq!(parsed.inspect().expect("repeat inspection"), report);
    assert_eq!(parsed.render_html().expect("repeat render"), html);
}

#[test]
fn code_examples_are_not_links_html_or_headings() {
    let source = "`[not link](javascript:alert)`\n\n```html\n# not a heading\n<script>alert(1)</script>\n[[site:alias]]\n```\n\n[real](../docs/)\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("parse");
    let report = parsed.inspect().expect("inspect");
    assert!(report.headings.is_empty());
    assert!(report.raw_html.is_empty());
    assert_eq!(report.links.len(), 1);
    assert_eq!(report.links[0].destination, "../docs/");
    let html = parsed.render_html().expect("escaped code");
    assert!(html_text::text(&html).contains("<script>alert(1)</script>"));
    assert!(!html.contains("<script>"));
    assert!(
        html_text::text(&html).contains("[[site:alias]]"),
        "no site-link rewriting inside code"
    );
}

#[test]
fn syntax_extensions_share_the_same_text_and_html_inputs() {
    let source = "- [x] done\n- [ ] pending\n\n| first | second |\n| --- | --- |\n| ~~old~~ | new |\n\nNote[^n].\n\n[^n]: Footnote body.\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("parse");
    let report = parsed.inspect().expect("inspect");
    let html = parsed.render_html().expect("render");
    for expected in [
        "[x] done",
        "[ ] pending",
        "first\tsecond",
        "old\tnew",
        "Footnote body.",
    ] {
        assert!(
            report.text.contains(expected),
            "{expected}: {}",
            report.text
        );
    }
    for expected in [
        "type=\"checkbox\"",
        "<table>",
        "<del>old</del>",
        "footnote",
        "Footnote body.",
    ] {
        assert!(html.contains(expected), "{expected}: {html}");
    }
}

#[test]
fn raw_html_is_explicit_syntax_evidence_but_never_implicitly_trusted() {
    for source in [
        "<script>window.secret()</script>\n",
        "hello <b>bold</b>",
        "<!-- hidden -->\n",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("parse");
        let report = parsed.inspect().expect("inspection permitted");
        assert!(!report.raw_html.is_empty());
        for location in report.raw_html {
            let span = location.span;
            assert!(source[span.start..span.end].starts_with('<'));
        }
        assert!(matches!(parsed.render_html(), Err(Error::RawHtml(_))));
    }
}

#[test]
fn source_renderability_check_matches_the_real_renderer_before_theme_selection() {
    let source = "<script>window.secret()</script>\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("source syntax remains inspectable");
    let checked = parsed
        .check_source_rendering()
        .expect_err("unsupported source must fail before Theme selection");
    assert!(
        checked
            .to_string()
            .contains("requires an admitted renderer"),
        "{checked}"
    );
    assert_eq!(
        parsed
            .render_html()
            .expect_err("the actual renderer must reject the same source"),
        checked,
    );

    for source in [
        "note[^outer]\n\n[^outer]: nested[^inner]\n\n[^inner]: target\n",
        "![label[^note]](https://example.invalid/image.png)\n\n[^note]: detail\n",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("footnote source syntax remains inspectable");
        parsed
            .check_source_rendering()
            .expect("effective emitted footnote closure is renderable");
        parsed
            .render_html()
            .expect("the actual renderer accepts the same footnote source");
    }
    let summary = ParsedMarkdown::parse(
        "Lead.\n\n<!-- more -->\n\nRest.\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("summary source");
    summary
        .check_source_rendering()
        .expect("the explicit summary marker is not arbitrary raw HTML");
    summary
        .render_html()
        .expect("the same summary source renders");
}

#[test]
fn html_rejects_dangerous_destinations_after_markdown_entity_decoding() {
    for source in [
        "[x](javascript:alert)",
        "[x](JaVaScRiPt:alert)",
        "[x](javascript&#58;alert)",
        "[x](java&#x09;script:alert)",
        "[x](data:text/html,evil)",
        "[x](file:///private/path)",
        "![x](mailto:person@example.invalid)",
        "![x](data:image/svg+xml,evil)",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("syntax");
        assert_eq!(
            parsed.inspect().expect("syntax evidence").links.len(),
            1,
            "{source}"
        );
        assert!(
            matches!(parsed.render_html(), Err(Error::UnsafeUrl(_))),
            "{source}"
        );
    }
    for source in [
        "[x](HTTPS://example.invalid)",
        "[x](mailto:person@example.invalid)",
        "[x](#part:two)",
        "[x](?key=value:two)",
        "[x](./part:two)",
        "![x](//example.invalid/image.png)",
    ] {
        assert!(
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("syntax")
                .render_html()
                .is_ok(),
            "{source}"
        );
    }
}

#[test]
fn input_node_depth_and_each_projection_are_bounded_without_partial_success() {
    let source = "**nested**";
    let node_count =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("baseline")
            .inspect()
            .expect("inspection")
            .node_count;
    for (limits, expected) in [
        (
            Limits {
                source_bytes: source.len() - 1,
                ..Limits::default()
            },
            "source bytes",
        ),
        (
            Limits {
                nodes: node_count - 1,
                ..Limits::default()
            },
            "syntax nodes",
        ),
        (
            Limits {
                depth: 1,
                ..Limits::default()
            },
            "syntax depth",
        ),
    ] {
        assert!(
            matches!(ParsedMarkdown::parse(source, limits, &gazette_adapter_markdown::Comrak), Err(Error::Limit(resource)) if resource == expected)
        );
    }
    let exact = ParsedMarkdown::parse(
        source,
        Limits {
            source_bytes: source.len(),
            nodes: node_count,
            // Document root 0, paragraph 1, strong 2, text 3.
            depth: 3,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("exact bounds");
    let text = exact.inspect().expect("inspect").text;
    let html = exact.render_html().expect("render");
    let bounded = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: text.len() - 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("parse");
    assert!(matches!(bounded.inspect(), Err(Error::Limit("text bytes"))));
    let bounded = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: html.len() - 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("parse");
    assert!(matches!(
        bounded.render_html(),
        Err(Error::Limit("rendered output bytes"))
    ));
    assert_eq!(
        ParsedMarkdown::parse(
            source,
            Limits {
                output_bytes: html.len(),
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak
        )
        .expect("parse")
        .render_html()
        .expect("exact HTML bound"),
        html
    );
    assert!(matches!(
        ParsedMarkdown::parse(
            "",
            Limits {
                source_bytes: 0,
                nodes: 0,
                depth: 0,
                fact_bytes: 0,
                output_bytes: 0,
            },
            &gazette_adapter_markdown::Comrak,
        ),
        Err(Error::Limit("syntax nodes"))
    ));
    let empty = ParsedMarkdown::parse(
        "",
        Limits {
            source_bytes: 0,
            nodes: 1,
            depth: 0,
            fact_bytes: 0,
            output_bytes: 0,
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("empty source");
    assert_eq!(empty.inspect().expect("empty inspection").text, "");
    assert_eq!(empty.render_html().expect("empty HTML"), "");
}

#[test]
fn repeated_links_stay_occurrences_and_output_identity_binds_exact_source_bytes() {
    let source = "[one](/same) [two](/same)\r\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("parse");
    let report = parsed.inspect().expect("inspect");
    assert_eq!(report.links.len(), 2);
    assert_ne!(report.links[0].span, report.links[1].span);
    assert_eq!(report.links[0].destination, report.links[1].destination);
    let changed = ParsedMarkdown::parse(
        "[one](/same) [two](/same)\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("changed")
    .inspect()
    .expect("changed inspection");
    assert_ne!(report.source_sha256, changed.source_sha256);
    assert_eq!(report.text, changed.text);
    let debug = format!("{parsed:?}");
    assert!(
        !debug.contains("[one]"),
        "debug is structural, not a source dump"
    );
}

#[test]
fn emitted_image_occurrences_preserve_native_labels_and_exact_resolved_destinations() {
    let source = "![A *strong* `code` &amp;\nline](image.svg \"A title\")\n\n![](image.svg)\n\nUnused[^missing].\n\n[^orphan]: ![not emitted](private.svg)\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .unwrap();
    let rendered = parsed
        .render_resolving_references(|kind, _, _| {
            assert_eq!(kind, LinkKind::Image);
            Ok(Some("https://example.test/image.svg?v=1&x=2".into()))
        })
        .unwrap();
    assert_eq!(rendered.images.len(), 2);
    assert_eq!(rendered.images[0].alt, "A strong code & line");
    assert_eq!(rendered.images[0].title.as_deref(), Some("A title"));
    assert_eq!(rendered.images[1].alt, "");
    assert_eq!(rendered.images[1].occurrence, 2);
    assert!(rendered.images[0].address.is_none());
    assert_eq!(
        rendered.images[0].destination,
        rendered.images[1].destination
    );
    assert!(
        rendered
            .body_html
            .contains("alt=\"A strong code &amp; line\"")
    );
    assert!(!rendered.body_html.contains("private.svg"));
}

#[test]
fn image_metadata_expansion_respects_the_existing_complete_render_budget() {
    let source = (0..24)
        .map(|_| "![label](image.svg)\n\n")
        .collect::<String>();
    let parsed = ParsedMarkdown::parse(
        &source,
        Limits {
            output_bytes: 256,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(
        parsed
            .render_resolving_references(|_, _, _| Ok(Some(
                "https://example.test/image.svg".into()
            )))
            .is_err()
    );
}
