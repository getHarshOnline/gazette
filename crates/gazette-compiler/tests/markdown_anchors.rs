//! One retained anchor index drives inspection, fragments and real HTML.

#[path = "markdown_anchors/references.rs"]
mod references;

#[path = "markdown_anchors/manual.rs"]
mod manual;

use gazette_compiler::markdown::{Error, Limits, ParsedMarkdown};
use gazette_core::source::Precision;

fn parse(source: &str) -> ParsedMarkdown<'_> {
    ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect("valid bounded syntax")
}

#[test]
fn cached_headings_are_the_same_index_used_by_inspection_lookup_and_both_render_paths() {
    let source = "# Hello *world*\n\n## Guide {#guide .wide .accent}\n\n[read](guide.md)\n";
    let parsed = parse(source);
    let headings = parsed.headings().expect("shared heading index");
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].id, "hello-world");
    assert_eq!(headings[0].fragment, "#hello-world");
    assert_eq!(headings[1].id, "guide");
    assert_eq!(headings[1].parent, Some(0));
    assert_eq!(
        &source[headings[0].span.start..headings[0].span.end],
        "# Hello *world*"
    );
    assert_eq!(headings[0].precision, Precision::Construct);
    let inspected = parsed.inspect().expect("inspect retained syntax");
    assert_eq!(inspected.headings.as_slice(), headings);
    let html = parsed.render_html().expect("render indexed headings");
    assert!(html.contains("<h1 id=\"hello-world\">Hello <em>world</em></h1>"));
    assert!(html.contains("<h2 id=\"guide\" class=\"wide accent\">Guide</h2>"));
    let resolved = parsed
        .render_html_resolving_links(|destination, _| {
            assert_eq!(destination, "guide.md");
            Ok(Some("/docs/guide/#guide".to_owned()))
        })
        .expect("same heading renderer with resolved links");
    assert_eq!(
        resolved,
        html.replace("href=\"guide.md\"", "href=\"/docs/guide/#guide\"")
    );
    for heading in headings {
        assert!(
            parsed
                .contains_anchor(&heading.id)
                .expect("exact anchor lookup")
        );
    }
    assert!(
        !parsed
            .contains_anchor("Guide")
            .expect("case-sensitive lookup")
    );
    assert!(std::ptr::eq(
        headings,
        parsed.headings().expect("cached slice")
    ));
    assert_eq!(parsed.render_html().expect("repeat render"), html);
    assert_eq!(parsed.inspect().expect("repeat inspection"), inspected);
    assert_eq!(parsed.source(), source);
}

#[test]
fn explicit_later_ids_are_reserved_before_source_order_disambiguation() {
    let parsed = parse("# Hello\n# Hello\n# Manual {#hello}\n# Manual {#hello-1}\n# Hello-2\n");
    let headings = parsed.headings().expect("global explicit reservation");
    let ids: Vec<_> = headings.iter().map(|heading| heading.id.as_str()).collect();
    assert_eq!(ids, ["hello-2", "hello-3", "hello", "hello-1", "hello-2-1"]);
    let html = parsed.render_html().expect("unique rendered IDs");
    for id in ids {
        assert_eq!(html.matches(&format!("id=\"{id}\"")).count(), 1);
    }
}

#[test]
fn prototype_slug_transliteration_and_nonempty_fallbacks_are_deterministic() {
    let parsed = parse("# Héllo\n# 日本\n# !!!\n# \n# section\n");
    let ids: Vec<_> = parsed
        .headings()
        .expect("transliteration and fallback")
        .iter()
        .map(|heading| heading.id.as_str())
        .collect();
    assert_eq!(
        ids,
        ["hello", "ri-ben", "section", "section-1", "section-2"]
    );
    let html = parsed
        .render_html()
        .expect("render original Unicode labels");
    assert!(html.contains("<h1 id=\"ri-ben\">日本</h1>"));
    assert!(html.contains("<h1 id=\"section-1\"></h1>"));
}

#[test]
fn footnote_ids_reserve_dom_identity_and_are_queryable_without_becoming_headings() {
    // The automatic heading would otherwise collide with the generated note ID.
    let parsed = parse("# Fn note 6e6f7465\n\nBody[^note].\n\n[^note]: Note body.\n");
    let headings = parsed.headings().expect("footnote-aware index");
    assert_eq!(headings.len(), 1);
    assert_eq!(headings[0].id, "fn-note-6e6f7465-1");
    assert!(
        parsed
            .contains_anchor("fn-note-6e6f7465")
            .expect("footnote destination")
    );
    assert!(
        parsed
            .contains_anchor("fn-note-6e6f7465-1")
            .expect("heading destination")
    );
    assert!(
        !parsed
            .contains_anchor("missing")
            .expect("absent destination")
    );
    let html = parsed.render_html().expect("actual DOM identifiers");
    assert_eq!(html.matches("id=\"fn-note-6e6f7465\"").count(), 1);
    assert_eq!(html.matches("id=\"fn-note-6e6f7465-1\"").count(), 1);
    assert!(html.contains("href=\"#fn-note-6e6f7465\""));
}

#[test]
fn duplicate_explicit_ids_and_footnote_collisions_fail_all_anchor_consumers() {
    for source in [
        "# One {#same}\n# Two {#same}\n",
        "# One {#fn-note-6e6f7465}\n\nBody[^note].\n\n[^note]: Footnote.\n",
        "# One {#fnref-note-6e6f7465}\n\nBody[^note].\n\n[^note]: Footnote.\n",
    ] {
        let parsed = parse(source);
        let error = parsed.headings().expect_err("ambiguous ID rejected");
        assert_eq!(error.code(), "markdown_anchor");
        let Error::Anchor { span, .. } = &error else {
            panic!("expected source-bound anchor diagnostic");
        };
        assert!(span.start < span.end && span.end <= source.len());
        assert_eq!(parsed.headings().expect_err("cached failure"), error);
        assert_eq!(
            parsed
                .contains_anchor("same")
                .expect_err("no partial index"),
            error
        );
        assert_eq!(parsed.render_html().expect_err("no ambiguous HTML"), error);
        assert_eq!(parsed.inspect().expect_err("no partial inspection"), error);
    }
}

#[test]
fn heading_attributes_cannot_smuggle_raw_html_capabilities() {
    for source in [
        "# Bad {#valid onclick=\"alert(1)\"}\n",
        "# Bad {#valid style=\"color:red\"}\n",
        "# Bad {#valid data-extra=\"value\"}\n",
        "# Bad {#\"bad\u{7}id\"}\n",
        "# Bad {.\"bad\u{7}class\"}\n",
    ] {
        let parsed = parse(source);
        assert!(
            matches!(parsed.headings(), Err(Error::Anchor { .. })),
            "{source:?}"
        );
        assert!(
            matches!(parsed.render_html(), Err(Error::Anchor { .. })),
            "{source:?}"
        );
    }
    let parsed = parse("# Safe {#\"A&B\" .wide}\n");
    assert!(parsed.contains_anchor("A&B").expect("literal ID identity"));
    assert!(
        parsed
            .render_html()
            .expect("attribute escaping")
            .contains("id=\"A&amp;B\"")
    );
}

#[test]
fn malformed_attribute_spelling_remains_inert_visible_text() {
    let source = "# Bad {#valid onclick=alert(1)}\n";
    let parsed = parse(source);
    let html = parsed
        .render_html()
        .expect("unrecognized attributes are literal text");
    let opening = html.split_once('>').expect("heading element").0;
    assert!(
        !opening.contains("onclick="),
        "no event-handler attribute: {html}"
    );
    assert!(
        !parsed
            .contains_anchor("valid")
            .expect("literal spelling is not an ID")
    );
    assert!(html.contains("Bad {#valid onclick=alert(1)}"));
}

#[test]
fn attribute_grammar_remains_the_native_parsers_not_a_second_syntax_interpretation() {
    let parsed = parse("# Empty {#}\n# !!! {#}\n# Choice {#last}\n");
    let headings = parsed.headings().expect("native attribute grammar");
    let ids: Vec<_> = headings.iter().map(|heading| heading.id.as_str()).collect();
    assert_eq!(ids, ["empty", "section", "last"]);
    let html = parsed.render_html().expect("render retained IDs");
    assert!(html.contains("<h1 id=\"last\">Choice</h1>"));
}

#[test]
fn multiple_native_heading_id_tokens_fail_before_anchor_allocation() {
    let source = "# Choice {#first .wide #last}\n";
    let error = ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
        .expect_err("ambiguous provider syntax must not reach anchor allocation");
    let Error::Adapter(gazette_core::markdown::Error::Unsupported { location, reason }) = error
    else {
        panic!("expected exact provider diagnostic");
    };
    assert_eq!(reason, "multiple explicit heading IDs are ambiguous");
    assert_eq!(location.precision, Precision::Exact);
    assert_eq!(location.span.slice(source), Some("#last"));
}

#[test]
fn template_fragments_encode_exact_ids_once_without_changing_dom_identity() {
    let parsed = parse(
        "# Unicode {#\"café\"}\n# Reserved {#\"A&B\"}\n# Percent {#a%b}\n# Unreserved {#\"AZaz09-._~\"}\n",
    );
    let headings = parsed.headings().expect("template-ready fragments");
    let pairs: Vec<_> = headings
        .iter()
        .map(|heading| (heading.id.as_str(), heading.fragment.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            ("café", "#caf%C3%A9"),
            ("A&B", "#A%26B"),
            ("a%b", "#a%25b"),
            ("AZaz09-._~", "#AZaz09-._~"),
        ]
    );
    let inspected = parsed.inspect().expect("same machine projection");
    assert_eq!(inspected.headings.as_slice(), headings);
    let html = parsed.render_html().expect("literal DOM IDs");
    assert!(html.contains("id=\"café\""));
    assert!(html.contains("id=\"A&amp;B\""));
    assert!(html.contains("id=\"a%b\""));
    assert!(!html.contains("id=\"a%25b\""));
    assert!(
        parsed
            .contains_anchor("a%b")
            .expect("lookup is already-decoded identity")
    );
    assert!(
        !parsed
            .contains_anchor("a%25b")
            .expect("lookup never decodes twice")
    );
}

#[test]
fn percent_footnote_labels_use_encoded_aliases_without_a_wrong_fragment() {
    let parsed = parse("# Heading\n\nRead[^a%20b].\n\n[^a%20b]: Footnote text.\n");
    assert_eq!(parsed.headings().expect("safe note alias").len(), 1);
    assert!(!parsed.contains_anchor("a%20b").expect("not a raw DOM ID"));
    assert!(
        parsed
            .contains_anchor("fn-note-6125323062")
            .expect("encoded note ID")
    );
    let html = parsed.render_html().expect("safe matched DOM and link");
    assert!(html.contains("id=\"fn-note-6125323062\""));
    assert!(html.contains("href=\"#fn-note-6125323062\""));
    assert!(!html.contains("href=\"#a%20b\""));
}

#[test]
fn heading_text_excludes_footnotes_and_markup_but_preserves_alt_and_break_spaces() {
    let source = "# A *bold* [link](guide.md) `code` ![alt](img.png) &amp; note[^fn]\n\nSoft\nline\n----\n\nHard  \nbreak\n----\n\n[^fn]: Footnote body.\n";
    let parsed = parse(source);
    let headings = parsed.headings().expect("syntax-derived labels");
    let text: Vec<_> = headings
        .iter()
        .map(|heading| heading.text.as_str())
        .collect();
    assert_eq!(
        text,
        ["A bold link code alt & note", "Soft line", "Hard break"]
    );
    assert_eq!(headings[0].id, "a-bold-link-code-alt-note");
    for heading in headings {
        assert!(source.is_char_boundary(heading.span.start));
        assert!(source.is_char_boundary(heading.span.end));
        assert!(heading.span.start < heading.span.end);
    }
    assert_eq!(
        parsed
            .inspect()
            .expect("same label projection")
            .headings
            .as_slice(),
        headings
    );
}

#[test]
fn hierarchy_uses_nearest_preceding_lower_level_without_inventing_missing_levels() {
    let parsed =
        parse("## Start\n#### Deep\n### Less deep\n# Root\n### Skip\n## Sibling\n## Next\n");
    let headings = parsed.headings().expect("source-order hierarchy");
    let parents: Vec<_> = headings.iter().map(|heading| heading.parent).collect();
    assert_eq!(
        parents,
        [None, Some(0), Some(0), None, Some(3), Some(3), Some(3)]
    );
    let levels: Vec<_> = headings.iter().map(|heading| heading.level).collect();
    assert_eq!(levels, [2, 4, 3, 1, 3, 2, 2]);
}

#[test]
fn anchor_allocation_is_lazy_bounded_and_returns_no_partial_result() {
    for limit in [0, 1, 20] {
        let parsed = ParsedMarkdown::parse(
            "# abcdefghijklmnopqrst\n# another\n",
            Limits {
                output_bytes: limit,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak,
        )
        .expect("syntax parsing is independent of projection allowance");
        assert!(parsed.node_count() > 0);
        assert!(matches!(parsed.headings(), Err(Error::Limit(_))));
        assert!(matches!(
            parsed.contains_anchor("abcdefghijklmnopqrst"),
            Err(Error::Limit(_))
        ));
        assert!(matches!(parsed.inspect(), Err(Error::Limit(_))));
        assert!(matches!(parsed.render_html(), Err(Error::Limit(_))));
        assert!(matches!(parsed.headings(), Err(Error::Limit(_))));
    }
    let source = "# Heading\n\n```md\n# Example {#heading}\n```\n";
    let parsed = parse(source);
    assert_eq!(parsed.headings().expect("code is not a heading").len(), 1);
    assert_eq!(parsed.headings().expect("same index")[0].id, "heading");
}
