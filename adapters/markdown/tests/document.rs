//! Real-adapter source, binding, projection and refusal boundaries.

#[path = "document/highlighting.rs"]
mod highlighting;

#[path = "document/inline_svg.rs"]
mod inline_svg;

#[path = "document/dark_sources.rs"]
mod dark_sources;

#[path = "document/responsive.rs"]
mod responsive;

use gazette_adapter_markdown::Comrak;
use gazette_core::markdown::{
    Error, Excerpt, FootnoteBinding, HeadingBinding, Limits, LinkBinding, Plan, Profile, Syntax,
};
use gazette_core::source::{Precision, Sha256Digest};
use gazette_ports::markdown::{Document, Markdown};

fn parse(source: &str) -> Box<dyn Document> {
    Comrak
        .parse(source, Profile::Publication, Limits::default())
        .expect("parse retained source")
}

fn plan(syntax: &Syntax) -> Plan {
    Plan {
        identity: syntax.identity,
        headings: syntax
            .headings
            .iter()
            .enumerate()
            .map(|(index, h)| HeadingBinding {
                occurrence: h.clone(),
                id: format!("heading-{index}"),
                level: h.level,
            })
            .collect(),
        links: syntax
            .links
            .iter()
            .filter(|l| l.emitted)
            .map(|l| LinkBinding {
                image_sources: None,
                svg_layout: None,
                image_width: None,
                image_dimensions: None,
                image_loading: None,
                image_fetch_priority: None,
                inline_svg: None,
                image_address: None,
                occurrence: l.clone(),
                destination: l.destination.clone(),
                annotation: None,
            })
            .collect(),
        footnotes: syntax
            .footnotes
            .iter()
            .enumerate()
            .map(|(index, f)| FootnoteBinding {
                occurrence: f.clone(),
                alias: format!("note-{index}"),
            })
            .collect(),
        excerpt: None,
        math: vec![],
        components: vec![],
    }
}

#[test]
fn github_alerts_render_all_five_meanings_with_checked_content() {
    for (marker, label, class) in [
        ("NOTE", "Note", "note"),
        ("TIP", "Tip", "tip"),
        ("IMPORTANT", "Important", "important"),
        ("WARNING", "Warning", "warning"),
        ("CAUTION", "Caution", "caution"),
    ] {
        let source = format!(
            "> [!{marker}]\r\n> Café 🧭 **meaning**, `code` and [details](./old).\r\n>\r\n> ## A heading\r\n>\r\n> - First\r\n> - Second\r\n"
        );
        let document = parse(&source);
        let before = document.syntax().clone();
        let link = &before.links[0];
        assert_eq!(
            &source[link.location.span.start..link.location.span.end],
            "[details](./old)"
        );
        let mut selected = plan(&before);
        selected.links[0].destination = "/resolved/".into();
        let rendered = document.render(&selected, None).unwrap();
        let html = &rendered.body.html;
        assert!(
            html.contains(&format!("class=\"markdown-alert markdown-alert-{class}\"")),
            "{html}"
        );
        assert!(
            html.contains(&format!("<p class=\"markdown-alert-title\">{label}</p>")),
            "{html}"
        );
        assert!(html.contains("Café 🧭 <strong>meaning</strong>, <code>code</code>"));
        assert!(html.contains("href=\"/resolved/\""));
        assert!(html.contains("<h2 id=\"heading-0\">A heading</h2>"));
        assert!(html.contains("<li>First</li>"));
        assert!(!html.contains("[!"));
        assert!(
            rendered.body.text.starts_with(&format!("{label}\nCafé 🧭")),
            "{}",
            rendered.body.text
        );
        assert!(!rendered.body.text.contains("[!"));
        assert_eq!(document.syntax(), &before);
        assert_eq!(
            before.identity.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
        let math_document = Comrak
            .parse(&source, Profile::PublicationMath, Limits::default())
            .unwrap();
        let mut math_plan = plan(math_document.syntax());
        math_plan.links[0].destination = "/resolved/".into();
        assert_eq!(math_document.render(&math_plan, None).unwrap(), rendered);
    }
}

#[test]
fn alert_lookalikes_and_regular_quotations_keep_their_meaning_in_html() {
    for source in [
        "> An ordinary **quotation**.\n",
        "> \\[!NOTE]\n> Escaped literal.\n",
        "> &#91;!NOTE]\n> Entity-encoded literal.\n",
        "> `[!TIP]`\n> Inline code.\n",
        "> Earlier text.\n> [!IMPORTANT]\n> Later marker.\n",
        "> Earlier paragraph.\n>\n> [!WARNING]\n> Later paragraph.\n",
        "> [!CUSTOM]\n> Unknown marker.\n",
        "> [!NOTE\n> Incomplete marker.\n",
        "> [!NOTE]]\n> Extra delimiter.\n",
        "> [!CAUTION] Same-line prose.\n",
        "> [!note]\n> Lowercase literal.\n",
        "[!NOTE]\nOrdinary text.\n",
    ] {
        let document = parse(source);
        let rendered = document.render(&plan(document.syntax()), None).unwrap();
        assert_eq!(
            rendered.body.html,
            comrak::markdown_to_html(source, &comrak::Options::default()),
            "{source}"
        );
        assert!(!rendered.body.html.contains("markdown-alert"));
    }
}

#[test]
fn alerts_preserve_nested_quotes_lists_footnotes_and_excerpts() {
    let source = "> Outer quote.\n>\n> > [!TIP]\n> > A nested tip.\n>\n> Still quoted.\n\n- List item.\n\n  > [!WARNING]\n  > A warning in a list.\n\n> [!NOTE]\n>\n> > Ordinary child quote.\n>\n> > [!CAUTION]\n> > A nested danger.\n>\n> A reference[^n].\n>\n> <!-- more -->\n>\n> Later text.\n>\n> ```md\n> > [!IMPORTANT]\n> > Fenced literal.\n> ```\n\n[^n]: Footnote **detail**.\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    selected.excerpt = Some(Excerpt {
        marker: document.syntax().html[0].clone(),
        continuation: "later-content".into(),
    });
    let rendered = document.render(&selected, None).unwrap();
    for kind in ["tip", "warning", "note", "caution"] {
        assert_eq!(
            rendered
                .body
                .html
                .matches(&format!("class=\"markdown-alert markdown-alert-{kind}\""))
                .count(),
            1
        );
    }
    assert_eq!(rendered.body.html.matches("<blockquote>").count(), 2);
    assert_eq!(rendered.body.html.matches("<ul>").count(), 1);
    assert_eq!(
        highlighting::code_text(&rendered.body.html),
        "> [!IMPORTANT]\n> Fenced literal.\n"
    );
    assert!(
        rendered
            .body
            .html
            .contains("Footnote <strong>detail</strong>")
    );
    assert!(rendered.body.html.contains("Later text."));
    let excerpt = rendered.excerpt.unwrap();
    assert!(excerpt.html.contains("A nested danger."));
    assert!(!excerpt.html.contains("Later text."));
    assert!(!excerpt.html.contains("Footnote"));
    assert_eq!(
        excerpt.html.matches("<div class=\"markdown-alert ").count(),
        excerpt.html.matches("</div>").count()
    );
}

#[test]
fn empty_alerts_and_rendered_alert_limits_are_checked() {
    for source in [
        "> [!NOTE]",
        "> [!NOTE]\n>\n",
        "> [!NOTE]\n>\n> A later paragraph.\n",
    ] {
        let document = parse(source);
        let rendered = document.render(&plan(document.syntax()), None).unwrap();
        assert_eq!(
            rendered.body.html.matches("markdown-alert-title").count(),
            1
        );
        assert!(rendered.body.text.starts_with("Note\n"));
        assert_eq!(
            rendered.body.html.matches("<div ").count(),
            rendered.body.html.matches("</div>").count()
        );
    }
    let document = Comrak
        .parse(
            "> [!NOTE]\n> Small.\n",
            Profile::Publication,
            Limits {
                output_bytes: 80,
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        document.render(&plan(document.syntax()), None),
        Err(Error::Limit(_))
    ));
}

#[test]
fn tables_keep_one_native_grid_inside_an_unstyled_overflow_boundary() {
    let source =
        "Before.\n\n| Item | Meaning |\n| :--- | ---: |\n| `alpha` | **Retained** |\n\nAfter.\n";
    let document = parse(source);
    let selected = plan(document.syntax());
    let rendered = document.render(&selected, None).unwrap();
    assert_eq!(
        rendered
            .body
            .html
            .matches("<div data-gazette-table>")
            .count(),
        1
    );
    assert!(
        rendered
            .body
            .html
            .contains("<div data-gazette-table>\n<table>")
    );
    assert!(rendered.body.html.contains("</table>\n</div>"));
    assert!(rendered.body.html.contains("<thead>"));
    assert!(rendered.body.html.contains("<tbody>"));
    assert!(rendered.body.html.contains("<th align=\"left\">Item</th>"));
    assert!(
        rendered
            .body
            .html
            .contains("<th align=\"right\">Meaning</th>")
    );
    assert!(rendered.body.html.contains("<code>alpha</code>"));
    assert!(rendered.body.html.contains("<strong>Retained</strong>"));
    assert!(!rendered.body.html.contains("tabindex"));
    assert!(!rendered.body.html.contains("role="));
    assert_eq!(rendered, document.render(&selected, None).unwrap());
    let markdown = document.render_markdown(&selected, None).unwrap();
    assert!(!markdown.contains("data-gazette-table"));
    assert!(markdown.contains("| Item"));
}

#[test]
fn opening_boundary_comes_from_the_first_emitted_h1_with_exact_inline_markup() {
    let document =
        parse("# Héllo **bold** [guide](./old) {#title .tone}\n\nA paragraph.\n\n## Detail\n");
    let mut selected = plan(document.syntax());
    selected.links[0].destination = "/resolved/".into();
    let rendered = document.render(&selected, None).unwrap();
    let end = rendered.opening.unwrap().heading_end;
    let (heading, remainder) = rendered.body.html.split_at(end);
    assert_eq!(
        heading,
        "<h1 id=\"heading-0\" class=\"tone\">Héllo <strong>bold</strong> <a href=\"/resolved/\">guide</a></h1>\n"
    );
    assert!(remainder.starts_with("<p>A paragraph.</p>"));
    assert_eq!([heading, remainder].concat(), rendered.body.html);
    assert_eq!(document.render(&selected, None).unwrap(), rendered);
}

#[test]
fn opening_does_not_promote_later_nested_or_subordinate_headings() {
    for source in [
        "A paragraph.\n\n# Later\n",
        "> # Nested\n\nA paragraph.\n",
        "## Section\n\nA paragraph.\n",
        "No heading.\n",
    ] {
        let document = parse(source);
        let rendered = document.render(&plan(document.syntax()), None).unwrap();
        assert!(rendered.opening.is_none(), "{source}");
    }
    let document = parse("# Source H1\n");
    let mut selected = plan(document.syntax());
    selected.headings[0].level = 2;
    assert!(document.render(&selected, None).unwrap().opening.is_none());
}

#[test]
fn opening_boundary_does_not_bypass_the_complete_output_budget() {
    let source = "# Title\n\nBody.\n";
    let document = parse(source);
    let full = document.render(&plan(document.syntax()), None).unwrap();
    for (limit, succeeds) in [
        (full.body.html.len(), true),
        (full.body.html.len() - 1, false),
    ] {
        let document = Comrak
            .parse(
                source,
                Profile::Publication,
                Limits {
                    output_bytes: limit,
                    ..Default::default()
                },
            )
            .unwrap();
        let result = document.render(&plan(document.syntax()), None);
        if succeeds {
            assert_eq!(result.unwrap(), full);
        } else {
            assert!(matches!(result, Err(Error::Limit(_))));
        }
    }
}

#[test]
fn subordinate_heading_projection_keeps_source_identity_and_refuses_invalid_levels() {
    let document = parse("# Retained title\n\n## Detail\n");
    let mut selected = plan(document.syntax());
    selected.headings[0].level = 2;
    selected.headings[1].level = 3;
    let rendered = document
        .render(&selected, None)
        .expect("subordinate heading projection");
    assert!(
        rendered
            .body
            .html
            .contains("<h2 id=\"heading-0\">Retained title</h2>")
    );
    assert!(
        rendered
            .body
            .html
            .contains("<h3 id=\"heading-1\">Detail</h3>")
    );
    assert_eq!(document.syntax().headings[0].level, 1);
    for level in [0, 7, u8::MAX] {
        selected.headings[0].level = level;
        assert_eq!(document.render(&selected, None), Err(Error::InvalidPlan));
    }
    selected.headings[0].level = 2;
    assert_eq!(
        document
            .render(&selected, None)
            .expect("same retained source survives refusal"),
        rendered
    );
}

#[test]
fn bound_publication_renders_headings_links_tables_and_formatter_footnotes() {
    let source = "# Héllo {#authored .tone}\n\n[one](./old) and note[^é] again[^é].\n\n| A | B |\n| - | - |\n| ~~old~~ | new |\n\n[^é]: footnote *body*\n";
    let document = parse(source);
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(
        document.syntax().headings[0].id.as_deref(),
        Some("authored")
    );
    let mut selected = plan(document.syntax());
    selected.links[0].destination = "/resolved".into();
    let rendered = document.render(&selected, None).expect("bound rendering");
    assert!(
        rendered
            .body
            .html
            .contains("<h1 id=\"heading-0\" class=\"tone\">Héllo</h1>")
    );
    assert!(rendered.body.html.contains("href=\"/resolved\""));
    assert!(rendered.body.html.contains("<table>"));
    assert!(rendered.body.html.contains("<del>old</del>"));
    assert!(rendered.body.html.contains("id=\"fn-note-0\""));
    assert!(rendered.body.html.contains("id=\"fnref-note-0-2\""));
    assert!(rendered.body.text.contains("footnote body"));
    assert_eq!(
        rendered,
        document
            .render(&selected, None)
            .expect("deterministic repeat")
    );
}

#[test]
fn whole_source_facts_do_not_leak_unused_definitions_into_emitted_text() {
    let document = parse(
        "Visible[^used].\n\n[^unused]: # hidden heading\n\n    [hidden](./hidden)\n\n[^used]: retained note\n",
    );
    let syntax = document.syntax();
    assert_eq!(syntax.footnotes.len(), 2);
    assert!(syntax.text.iter().any(|t| t.value.contains("hidden")));
    assert!(
        syntax
            .links
            .iter()
            .any(|l| l.destination == "./hidden" && !l.emitted)
    );
    assert!(syntax.headings.iter().any(|h| !h.emitted));
    let rendered = document
        .render(&plan(syntax), None)
        .expect("normalized body");
    assert!(!rendered.body.html.contains("hidden"));
    assert!(!rendered.body.text.contains("hidden"));
    assert!(rendered.body.text.contains("retained note"));
}

#[test]
fn excerpt_uses_balanced_formatter_structure_without_footnotes_or_heading_ids() {
    let document = parse(
        "# Top\n\n> Before **bold**[^n].\n>\n> <!-- more -->\n>\n> Tail\n\nAfter\n\n[^n]: note\n",
    );
    let mut selected = plan(document.syntax());
    selected.excerpt = Some(Excerpt {
        marker: document.syntax().html[0].clone(),
        continuation: "continue-reading".into(),
    });
    let rendered = document.render(&selected, None).expect("balanced excerpt");
    assert!(
        rendered
            .body
            .html
            .contains("<span id=\"continue-reading\"></span>")
    );
    assert!(rendered.body.html.contains("Tail"));
    let excerpt = rendered.excerpt.expect("explicit excerpt");
    assert!(excerpt.html.contains("<h1>Top</h1>"));
    assert!(excerpt.html.contains("<blockquote>"));
    assert!(excerpt.html.contains("</blockquote>"));
    assert!(!excerpt.html.contains("footnote"));
    assert!(!excerpt.html.contains("id="));
    assert!(!excerpt.text.contains("Tail"));
    assert!(!excerpt.text.contains("After"));
    assert!(excerpt.text.contains("Before bold"));
}

#[test]
fn forged_partial_duplicate_and_cross_source_plans_are_refused() {
    let document = parse("# Heading\n\n[x](./x)\n");
    let valid = plan(document.syntax());
    let mut invalid = valid.clone();
    invalid.identity.source_sha256 = Sha256Digest::of(b"other");
    assert!(matches!(
        document.render(&invalid, None),
        Err(Error::InvalidPlan)
    ));
    invalid = valid.clone();
    invalid.links.clear();
    assert!(matches!(
        document.render(&invalid, None),
        Err(Error::InvalidPlan)
    ));
    invalid = valid.clone();
    invalid.links.push(invalid.links[0].clone());
    assert!(matches!(
        document.render(&invalid, None),
        Err(Error::InvalidPlan)
    ));
    invalid = valid.clone();
    invalid.headings[0].occurrence.text = "forged".into();
    assert!(matches!(
        document.render(&invalid, None),
        Err(Error::InvalidPlan)
    ));
    invalid = valid.clone();
    invalid.links[0].destination = "javascript:bad()".into();
    assert!(matches!(
        document.render(&invalid, None),
        Err(Error::InvalidPlan)
    ));
    document
        .render(&valid, None)
        .expect("rejections did not mutate retained document");
}

#[test]
fn raw_html_never_becomes_a_forged_summary_admission() {
    for source in [
        "<script>bad()</script>\n",
        "<!-- more -->\n\n<script>bad()</script>\n",
        "![alt <!-- more -->](./image.png)\n",
    ] {
        let document = parse(source);
        assert!(
            document
                .syntax()
                .text
                .iter()
                .any(|piece| piece.value.contains("<!--") || piece.value.contains("<script>"))
        );
        let mut selected = plan(document.syntax());
        selected.excerpt = Some(Excerpt {
            marker: document.syntax().html[0].clone(),
            continuation: "continue".into(),
        });
        assert!(document.render(&selected, None).is_err());
    }
}

#[test]
fn source_coordinates_preserve_unicode_line_endings_and_construct_precision() {
    let source = "# é😀\r\n\r\n[one\r\ntwo](./a)\r\n\r\n\\*literal\\*\rnext\n";
    let document = parse(source);
    let heading = &document.syntax().headings[0];
    assert_eq!(heading.location.span.slice(source), Some("# é😀"));
    assert_eq!(heading.location.precision, Precision::Construct);
    assert_eq!(
        document.syntax().links[0].location.span.slice(source),
        Some("[one\r\ntwo](./a)")
    );
    for piece in &document.syntax().text {
        assert!(piece.location.span.slice(source).is_some());
        assert_eq!(piece.location.precision, Precision::Construct);
    }
}

#[test]
fn recognized_heading_attributes_reject_a_second_id_at_its_exact_source_token() {
    for (source, duplicate) in [
        ("# Choice {#first #last}\n", "#last"),
        (
            "# Héading {.wide #\"café\" data-kind=guide #\"最後\"}\n",
            "#\"最後\"",
        ),
        ("Title {#first .wide #second}\n=====\n", "#second"),
    ] {
        let error = match Comrak.parse(source, Profile::Publication, Limits::default()) {
            Ok(_) => panic!("multiple native ID tokens are ambiguous: {source:?}"),
            Err(error) => error,
        };
        let Error::Unsupported { location, reason } = error else {
            panic!("expected located unsupported syntax for {source:?}");
        };
        assert_eq!(reason, "multiple explicit heading IDs are ambiguous");
        assert_eq!(location.precision, Precision::Exact);
        assert_eq!(location.span.slice(source), Some(duplicate));
    }
}

#[test]
fn recognized_single_ids_preserve_unicode_classes_and_attribute_facts() {
    let source = "# Héading {.wide data-kind=\"guide # text\" #\"café\"}\n";
    let document = parse(source);
    let heading = &document.syntax().headings[0];
    assert_eq!(heading.id.as_deref(), Some("café"));
    assert_eq!(heading.classes, ["wide"]);
    assert_eq!(
        heading.attributes,
        [("data-kind".to_owned(), "guide # text".to_owned())]
    );
}

#[test]
fn duplicate_id_spelling_outside_a_native_heading_attribute_block_is_plain_text() {
    let source = "Paragraph {#first #last}.\n\n`{#code #tokens}`\n";
    let document = parse(source);
    assert!(document.syntax().headings.is_empty());
    assert!(
        document
            .syntax()
            .text
            .iter()
            .any(|piece| piece.value.contains("{#first #last}"))
    );
}

#[test]
fn emitted_footnote_closure_excludes_orphan_and_image_references() {
    let document = parse(
        "root[^used] image ![label[^unused]](image.png)\n\n[^unused]: orphan [hidden](./hidden) refers[^used]\n\n[^used]: target[^nested]\n\n[^nested]: nested\n",
    );
    let syntax = document.syntax();
    let references = &syntax.footnote_references;
    assert_eq!(syntax.footnotes[0].references, 0);
    assert_eq!(syntax.footnotes[1].references, 1);
    assert_eq!(syntax.footnotes[2].references, 1);
    assert_eq!(
        references
            .iter()
            .filter(|reference| reference.emitted)
            .count(),
        2
    );
    assert!(
        references
            .iter()
            .any(|reference| reference.in_image && !reference.emitted)
    );
    assert!(references.iter().any(|reference| {
        reference.definition.as_deref() == Some("unused") && !reference.emitted
    }));
    assert!(
        syntax
            .links
            .iter()
            .any(|link| { link.destination == "./hidden" && !link.emitted })
    );
    let rendered = document
        .render(&plan(syntax), None)
        .expect("reachable footnote closure renders without orphan backlinks");
    assert!(rendered.body.html.contains("target"));
    assert!(rendered.body.html.contains("nested"));
    assert!(!rendered.body.html.contains("orphan refers"));
    assert_eq!(rendered, document.render(&plan(syntax), None).unwrap());
}

#[test]
fn inspected_and_emitted_text_preserve_tasks_cells_and_semantic_breaks() {
    let source = "# Title &amp; `code`\n\nA *bold* [label](https://example.test/private) ![alt](https://example.test/image.png)\\\nnext\nline.\n\n```txt\n[not](url) <tag>\n```\n\n- [x] done\n- [ ] pending\n\n| first | second |\n| --- | --- |\n| ~~old~~ | new |\n";
    let document = parse(source);
    let inspected: String = document
        .syntax()
        .text
        .iter()
        .map(|piece| piece.value.as_str())
        .collect();
    let rendered = document
        .render(&plan(document.syntax()), None)
        .expect("text and HTML projections");
    assert_eq!(inspected, rendered.body.text);
    for expected in [
        "Title & code\nA bold label alt\nnext line.\n[not](url) <tag>\n\n",
        "[x] done\n[ ] pending\n",
        "first\tsecond\t\nold\tnew\t\n",
    ] {
        assert!(
            inspected.contains(expected),
            "missing {expected:?} in {inspected:?}"
        );
    }
    assert!(!inspected.contains("https://"));
    assert!(rendered.body.html.contains("type=\"checkbox\""));
    assert!(rendered.body.html.contains("checked=\"\""));
    assert!(rendered.body.html.contains("<table>"));
}

#[test]
fn repeated_alias_replacements_obey_the_aggregate_string_ceiling() {
    let document = Comrak
        .parse(
            "one[^n] two[^n] three[^n]\n\n[^n]: note\n",
            Profile::Publication,
            Limits {
                output_bytes: 1024,
                ..Limits::default()
            },
        )
        .expect("source facts fit the ceiling");
    let mut selected = plan(document.syntax());
    selected.footnotes[0].alias = "n".repeat(400);
    assert!(matches!(
        document.render(&selected, None),
        Err(Error::Limit("fact string bytes"))
    ));
}

#[test]
fn limits_apply_before_input_parse_after_tree_parse_and_during_output() {
    assert!(matches!(
        Comrak.parse(
            "word",
            Profile::Publication,
            Limits {
                fact_bytes: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("fact string bytes"))
    ));
    assert!(matches!(
        Comrak.parse(
            "too long",
            Profile::Publication,
            Limits {
                source_bytes: 2,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("source bytes"))
    ));
    assert!(matches!(
        Comrak.parse(
            "word",
            Profile::Publication,
            Limits {
                nodes: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("syntax nodes"))
    ));
    assert!(matches!(
        Comrak.parse(
            "**word**",
            Profile::Publication,
            Limits {
                depth: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("syntax depth"))
    ));
    let document = Comrak
        .parse(
            "a",
            Profile::Publication,
            Limits {
                output_bytes: 3,
                ..Limits::default()
            },
        )
        .expect("small facts fit");
    assert!(matches!(
        document.render(&plan(document.syntax()), None),
        Err(Error::Limit("rendered output bytes"))
    ));
    let empty = parse("");
    assert_eq!(
        empty
            .render(&plan(empty.syntax()), None)
            .expect("empty body")
            .body
            .html,
        ""
    );
}
