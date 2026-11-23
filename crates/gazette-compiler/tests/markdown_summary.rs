//! Retained summary cuts share reference resolution and global anchors with the full body.

#[path = "support/html_text.rs"]
mod html_text;

use gazette_compiler::markdown::{Error, Limits, LinkKind, ParsedMarkdown};
use gazette_core::source::Span;

#[test]
fn explicit_marker_returns_exact_prefix_text_source_evidence_and_complete_body() {
    let source = "Before **bold** & café.\r\n\r\n<!-- \tmore \t-->\r\n\r\nAfter *tail*.\r\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("retained CRLF syntax");
    let rendered = parsed
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("summary projection");
    let summary = rendered.summary.expect("explicit cut");
    assert_eq!(
        summary.html,
        "<p>Before <strong>bold</strong> &amp; café.</p>\n"
    );
    assert_eq!(summary.text, "Before bold & café.\n");
    assert_eq!(summary.continuation, "#continue-reading");
    assert_eq!(
        source[summary.marker.start..summary.marker.end].trim(),
        "<!-- \tmore \t-->"
    );
    assert!(
        rendered
            .body_html
            .contains("<span id=\"continue-reading\"></span>")
    );
    assert!(rendered.body_html.contains("<p>After <em>tail</em>.</p>"));
    let boundary = "<!-- gazette-continuation -->";
    assert_eq!(rendered.body_html.matches(boundary).count(), 1);
    assert!(!rendered.body_html.replace(boundary, "").contains("<!--"));
    assert!(
        !rendered
            .body_html
            .contains(source[summary.marker.start..summary.marker.end].trim())
    );
    assert!(!parsed.text().expect("retained body text").contains("more"));
    let inspection = parsed.inspect().expect("retained marker inspection");
    assert_eq!(
        inspection.summary_marker.map(|location| location.span),
        Some(summary.marker)
    );
    assert!(
        inspection.raw_html.is_empty(),
        "marker is not untrusted raw HTML"
    );
    assert_eq!(inspection.text, parsed.text().expect("same event text"));
    assert_eq!(parsed.source(), source);
    let lf = source.replace("\r\n", "\n");
    let lf = ParsedMarkdown::parse(&lf, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("LF equivalent syntax");
    let lf_rendered = lf
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("LF equivalent output");
    assert_eq!(lf_rendered.body_html, rendered.body_html);
    assert_eq!(
        lf_rendered.summary.expect("same semantic cut").html,
        summary.html
    );
    assert_eq!(
        lf.text().expect("LF text"),
        parsed.text().expect("CRLF text")
    );
    assert_eq!(
        parsed.render_html().expect("HTML convenience"),
        rendered.body_html
    );
}

#[test]
fn absent_and_empty_prefix_are_distinct_and_code_markers_are_only_literal_content() {
    for source in [
        "No marker.\n",
        "`<!-- more -->`\n",
        "```html\n<!-- more -->\n```\n",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("literal syntax");
        let rendered = parsed
            .render_resolving_references(|_, _, _| Ok(None))
            .expect("ordinary body");
        assert!(rendered.summary.is_none(), "invented cut for {source:?}");
        assert!(
            !parsed
                .contains_anchor("continue-reading")
                .expect("anchor lookup")
        );
        if source.contains("more") {
            assert!(html_text::text(&rendered.body_html).contains("<!-- more -->"));
            assert!(
                parsed
                    .text()
                    .expect("literal code text")
                    .contains("<!-- more -->")
            );
        }
    }
    let parsed = ParsedMarkdown::parse(
        "<!--more-->\n\nTail.\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("leading marker");
    let rendered = parsed
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("empty explicit summary");
    let summary = rendered.summary.expect("not absence");
    assert_eq!(summary.html, "");
    assert_eq!(summary.text, "");
    assert!(rendered.body_html.contains("Tail."));
}

#[test]
fn nested_cuts_close_only_the_retained_prefix_without_reparsing_or_tail_leakage() {
    for (source, expected_prefix, expected_suffix) in [
        (
            "> Before **bold**\n>\n> <!-- more -->\n>\n> Tail.\n",
            "<blockquote>\n<p>Before <strong>bold</strong></p>",
            "</blockquote>\n",
        ),
        (
            "- Before **bold**\n\n  <!-- more -->\n\n  Tail.\n",
            "<ul>\n<li>\n<p>Before <strong>bold</strong></p>",
            "</li>\n</ul>\n",
        ),
        (
            "Before **bold** <!-- more --> tail.\n",
            "<p>Before <strong>bold</strong> ",
            "</p>\n",
        ),
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("native nested events");
        let rendered = parsed
            .render_resolving_references(|_, _, _| Ok(None))
            .expect("balanced summary");
        let summary = rendered.summary.expect("actual marker event");
        assert!(
            summary.html.starts_with(expected_prefix),
            "{source:?}: {}",
            summary.html
        );
        assert!(
            summary.html.ends_with(expected_suffix),
            "{source:?}: {}",
            summary.html
        );
        assert!(!summary.html.to_lowercase().contains("tail"));
        assert!(!summary.text.to_lowercase().contains("tail"));
        assert!(rendered.body_html.to_lowercase().contains("tail"));
        assert!(source[summary.marker.start..summary.marker.end].contains("<!-- more -->"));
    }
}

#[test]
fn one_reference_pass_supplies_both_outputs_and_preserves_all_body_asset_dependencies() {
    let source = "[before](before.md) ![cover](cover.png)\n\n<!-- more -->\n\n[after](after.md) ![tail](tail.png)\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("references around cut");
    let mut calls = Vec::new();
    let rendered = parsed
        .render_resolving_references(|kind, destination, span| {
            calls.push((
                kind,
                destination.to_owned(),
                source[span.start..span.end].to_owned(),
            ));
            Ok(Some(format!("/resolved/{destination}")))
        })
        .expect("one capture of mappings");
    assert_eq!(
        calls,
        [
            (
                LinkKind::Link,
                "before.md".into(),
                "[before](before.md)".into()
            ),
            (
                LinkKind::Image,
                "cover.png".into(),
                "![cover](cover.png)".into()
            ),
            (
                LinkKind::Link,
                "after.md".into(),
                "[after](after.md)".into()
            ),
            (
                LinkKind::Image,
                "tail.png".into(),
                "![tail](tail.png)".into()
            ),
        ]
    );
    let summary = rendered.summary.expect("summary");
    for fragment in [
        "href=\"/resolved/before.md\"",
        "src=\"/resolved/cover.png\"",
    ] {
        assert!(summary.html.contains(fragment));
        assert!(rendered.body_html.contains(fragment));
    }
    for fragment in ["href=\"/resolved/after.md\"", "src=\"/resolved/tail.png\""] {
        assert!(!summary.html.contains(fragment));
        assert!(rendered.body_html.contains(fragment));
    }
    assert_eq!(summary.text, "before cover\n");
}

#[test]
fn continuation_uses_the_full_anchor_namespace_but_summary_has_no_heading_or_footnote_ids() {
    let source = "# First {#continue-reading}\n\nRead[^note].\n\n<!-- more -->\n\n# Later {#continue-reading-1}\n\n[^note]: Footnote body.\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("headings and footnotes");
    let rendered = parsed
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("global namespace");
    let summary = rendered.summary.expect("cut");
    assert_eq!(summary.continuation, "#continue-reading-2");
    for anchor in [
        "continue-reading",
        "continue-reading-1",
        "continue-reading-2",
        "fn-note-6e6f7465",
    ] {
        assert!(parsed.contains_anchor(anchor).expect("global lookup"));
        assert_eq!(
            rendered
                .body_html
                .matches(&format!("id=\"{anchor}\""))
                .count(),
            1
        );
    }
    assert!(rendered.body_html.contains("href=\"#fn-note-6e6f7465\""));
    assert!(summary.html.contains("<h1>First</h1>"));
    assert!(!summary.html.contains(" id="));
    assert!(!summary.html.contains("footnote"));
    assert!(!summary.html.contains("#fn-note-6e6f7465"));
    assert_eq!(summary.text, "First\nRead.\n");
    let headings = parsed.headings().expect("real headings only");
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[1].id, "continue-reading-1");
}

#[test]
fn duplicate_or_forbidden_context_markers_fail_with_source_bound_summary_errors() {
    for source in [
        "<!-- more -->\n\n<!--more-->\n",
        "Body[^note].\n\n[^note]: Before <!-- more --> after.\n",
        "![before <!-- more --> after](image.png)\n",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("native marker syntax");
        let mut calls = 0;
        let error = parsed
            .render_resolving_references(|_, _, _| {
                calls += 1;
                Ok(None)
            })
            .expect_err("unsupported summary cut");
        assert_eq!(calls, 0);
        assert_eq!(error.code(), "markdown_summary");
        assert_eq!(
            parsed.inspect().expect_err("same cached invalid boundary"),
            error
        );
        assert_eq!(
            parsed
                .text()
                .expect_err("no partial text on invalid boundary"),
            error
        );
        assert_eq!(
            parsed
                .render_resolving_references(|_, _, _| panic!(
                    "cached failure must precede callbacks"
                ))
                .expect_err("repeat cached failure"),
            error
        );
        let Error::Summary { span, .. } = error else {
            panic!("exact summary failure")
        };
        assert!(source[span.start..span.end].contains("more"));
    }
}

#[test]
fn omitted_early_footnote_definitions_do_not_stall_later_mapping_or_heading_cursors() {
    let source = "[^note]: # Hidden heading\n\n    [hidden](hidden.md)\n\n# Visible heading\n\n[visible](visible.md)[^note]\n\n<!-- more -->\n\nTail.\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("definition before visible content");
    let mut calls = Vec::new();
    let rendered = parsed
        .render_resolving_references(|kind, destination, span| {
            assert_eq!(kind, LinkKind::Link);
            calls.push((
                destination.to_owned(),
                source[span.start..span.end].to_owned(),
            ));
            Ok(Some(format!("/resolved/{destination}")))
        })
        .expect("filtered definition still advances source-indexed mappings");
    assert_eq!(
        calls,
        [
            ("hidden.md".into(), "[hidden](hidden.md)".into()),
            ("visible.md".into(), "[visible](visible.md)".into()),
        ]
    );
    let summary = rendered.summary.expect("explicit prefix");
    assert!(summary.html.contains("<h1>Visible heading</h1>"));
    assert!(
        summary
            .html
            .contains("<a href=\"/resolved/visible.md\">visible</a>")
    );
    for excluded in ["Hidden heading", "hidden.md", "footnote", " id=", "#note"] {
        assert!(
            !summary.html.contains(excluded),
            "leaked {excluded}: {}",
            summary.html
        );
    }
    assert_eq!(summary.text, "Visible heading\nvisible\n");
    assert!(
        rendered
            .body_html
            .contains("<h1 id=\"hidden-heading\">Hidden heading</h1>")
    );
    assert!(
        rendered
            .body_html
            .contains("<h1 id=\"visible-heading\">Visible heading</h1>")
    );
    assert!(rendered.body_html.contains("href=\"/resolved/hidden.md\""));
    assert!(rendered.body_html.contains("href=\"/resolved/visible.md\""));
    assert!(rendered.body_html.contains("href=\"#fn-note-6e6f7465\""));
}

#[test]
fn continuation_allocation_cannot_displace_an_automatic_heading_after_the_cut() {
    let source =
        "# Intro\n\n<!-- more -->\n\n# Continue reading\n\n# Reserved {#continue-reading-1}\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("late automatic heading");
    let headings = parsed.headings().expect("anchor-first cache order");
    assert_eq!(
        headings
            .iter()
            .map(|heading| heading.id.as_str())
            .collect::<Vec<_>>(),
        ["intro", "continue-reading", "continue-reading-1"]
    );
    let rendered = parsed
        .render_resolving_references(|_, _, _| Ok(None))
        .expect("continuation after all headings");
    let summary = rendered.summary.expect("prefix");
    assert_eq!(summary.continuation, "#continue-reading-2");
    assert_eq!(summary.html, "<h1>Intro</h1>\n");
    for id in [
        "continue-reading",
        "continue-reading-1",
        "continue-reading-2",
    ] {
        assert!(parsed.contains_anchor(id).expect("shared exact lookup"));
        assert_eq!(
            rendered.body_html.matches(&format!("id=\"{id}\"")).count(),
            1
        );
    }
    assert!(
        rendered
            .body_html
            .contains("<h1 id=\"continue-reading\">Continue reading</h1>")
    );
    assert!(
        rendered
            .body_html
            .contains("<span id=\"continue-reading-2\"></span>")
    );
    assert!(std::ptr::eq(
        headings,
        parsed.headings().expect("stable cached headings")
    ));
}

#[test]
fn full_body_failures_after_the_cut_cannot_be_hidden_by_a_successful_prefix() {
    for tail in [
        "<script>bad()</script>",
        "<div><!-- more --></div>",
        "<!-- more -->extra",
        "[bad](javascript:alert)",
        "# First {#same}\n# Second {#same}",
    ] {
        let source = format!("[good](good.md)\n\n<!-- more -->\n\n{tail}\n");
        let parsed = ParsedMarkdown::parse(
            &source,
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("complete syntax");
        let mut calls = 0;
        let error = parsed
            .render_resolving_references(|_, _, _| {
                calls += 1;
                Ok(None)
            })
            .expect_err("complete-body rejection");
        assert_eq!(calls, 0, "{tail}");
        assert!(
            matches!(
                error,
                Error::RawHtml(_) | Error::UnsafeUrl(_) | Error::Anchor { .. }
            ),
            "{tail}: {error}"
        );
    }
    let source = "[before](before.md)\n\n<!-- more -->\n\n[after](after.md)\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("late resolution failure");
    let failure = Error::LinkResolution {
        span: Span { start: 0, end: 1 },
        reason: "late retained target missing",
    };
    let mut calls = 0;
    let error = parsed
        .render_resolving_references(|_, destination, _| {
            calls += 1;
            if destination == "after.md" {
                Err(failure.clone())
            } else {
                Ok(Some("/before/".into()))
            }
        })
        .expect_err("no partial body or summary");
    assert_eq!(error, failure);
    assert_eq!(calls, 2);
    assert!(
        parsed
            .render_resolving_references(|_, _, _| Ok(None))
            .is_ok(),
        "failure does not mutate retained AST"
    );
}

#[test]
fn exact_output_ceiling_and_aggregate_mapping_exhaustion_return_no_partial_result() {
    let source = "[before](before.md)\n\n<!-- more -->\n\nAfter.\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("baseline syntax");
    let expected = parsed
        .render_resolving_references(|_, _, _| Ok(Some("/resolved/".into())))
        .expect("baseline rendering");
    let limit = expected.body_html.len();
    let summary = expected.summary.as_ref().expect("summary");
    assert!(summary.html.len() < limit && summary.text.len() < limit);
    let bounded = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: limit,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded parse");
    let actual = bounded
        .render_resolving_references(|_, _, _| Ok(Some("/resolved/".into())))
        .expect("exact body bound");
    assert_eq!(actual.body_html, expected.body_html);
    assert_eq!(actual.summary.expect("same cut").html, summary.html);
    let smaller = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: limit - 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("one less byte");
    assert!(matches!(
        smaller.render_resolving_references(|_, _, _| Ok(Some("/resolved/".into()))),
        Err(Error::Limit("rendered output bytes"))
    ));
    let mapped = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: 128,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("mapping bound");
    assert_eq!(
        mapped
            .render_resolving_references(|_, _, _| Ok(Some("x".repeat(129))))
            .expect_err("mapping limit"),
        Error::Limit("render plan string bytes")
    );
}
