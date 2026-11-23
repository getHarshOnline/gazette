//! Caller resolution changes true link destinations without reparsing retained source.

#[path = "support/html_text.rs"]
mod html_text;

use gazette_compiler::markdown::{Error, Limits, LinkKind, ParsedMarkdown};
use gazette_core::source::Span;

#[test]
fn symbolic_completion_slots_edit_only_native_verbatim_destinations() {
    let source = "Préface 😀 [[site:example.test/guide/#missing|Visible β label]]\r\n\r\n[[site:]]\r\n\r\n`[[site:example.test/code/|Code]]`\r\n\r\n```md\r\n[[site:example.test/fenced/|Fenced]]\r\n```\r\n\r\n![image](site:example.test/image/)\r\n\r\n[ordinary](site:example.test/ordinary/)\r\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .unwrap();
    let slots = parsed.symbolic_completion_slots().unwrap();
    assert_eq!(
        slots
            .iter()
            .map(|span| span.slice(source).unwrap())
            .collect::<Vec<_>>(),
        ["site:example.test/guide/#missing", "site:"]
    );
    let mut edited = source.to_owned();
    edited.replace_range(
        slots[0].start..slots[0].end,
        "site:other.test/chosen/#heading",
    );
    assert!(edited.contains("[[site:other.test/chosen/#heading|Visible β label]]"));
    assert_eq!(parsed.source(), source);
    for span in slots {
        assert!(source.is_char_boundary(span.start) && source.is_char_boundary(span.end));
    }
    let bounded = ParsedMarkdown::parse(
        source,
        Limits {
            output_bytes: 1,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    assert!(matches!(
        bounded.symbolic_completion_slots(),
        Err(Error::Limit("symbolic completion slots"))
    ));
}

#[test]
fn wikilink_inspection_uses_native_occurrences_not_code_or_a_source_scanner() {
    let source = "[[site:example.test/guide/#part|Visible label]]\n\n`[[site:example.test/code/|Code]]`\n\n```md\n[[site:example.test/fenced/|Fenced]]\n```\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("actual retained Comrak syntax");
    let inspected = parsed.inspect().expect("whole-source native facts");
    assert_eq!(inspected.links.len(), 1);
    let link = &inspected.links[0];
    assert_eq!(link.kind, LinkKind::Link);
    assert_eq!(link.destination, "site:example.test/guide/#part");
    assert!(link.emitted);
    assert_eq!(
        &source[link.span.start..link.span.end],
        "[[site:example.test/guide/#part|Visible label]]"
    );
    assert!(inspected.text.contains("Visible label"));
    assert_eq!(parsed.source(), source);
    assert_eq!(parsed.inspect().expect("unchanged AST"), inspected);
}

#[test]
fn nested_footnote_reference_mapping_preserves_caller_resolution() {
    let source = "link [mapped](/ok) and note[^a]\n\n[^a]: detail[^b]\n\n[^b]: target";
    let parsed = gazette_compiler::markdown::ParsedMarkdown::parse(
        source,
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("whole-source parse");
    let inspection = parsed
        .inspect()
        .expect("whole-source evidence remains inspectable");
    assert_eq!(inspection.links.len(), 1);
    let mut seen = Vec::new();
    let result = parsed
        .render_resolving_references(|kind, destination, span| {
            seen.push((kind, destination.to_owned(), span));
            Ok(Some("/resolved/".into()))
        })
        .expect("nested footnotes use the same retained link resolution");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, LinkKind::Link);
    assert_eq!(seen[0].1, "/ok");
    assert!(result.body_html.contains("href=\"/resolved/\""));
    assert!(result.body_html.contains("detail"));
    assert!(result.body_html.contains("target"));
}

#[test]
fn resolution_visits_only_real_links_and_keeps_code_images_and_source_unchanged() {
    let source = "# Native guide\r\n\r\n[one](guide.md \"title\") and [two][ref].\r\n\r\n![asset](guide.md)\r\n\r\n`[code](guide.md)`\r\n\r\n```md\r\n[example](guide.md)\r\n```\r\n\r\n[ref]: guide.md\r\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("retained syntax");
    let before = parsed.inspect().expect("original syntax evidence");
    let mut seen = Vec::new();
    let html = parsed
        .render_html_resolving_links(|destination, span| {
            seen.push((destination.to_owned(), span));
            Ok(Some(format!(
                "/docs/guide/?occurrence={}&safe=yes",
                seen.len()
            )))
        })
        .expect("resolved HTML");
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].0, "guide.md");
    assert_eq!(seen[1].0, "guide.md");
    assert_eq!(
        &source[seen[0].1.start..seen[0].1.end],
        "[one](guide.md \"title\")"
    );
    assert_eq!(&source[seen[1].1.start..seen[1].1.end], "[two][ref]");
    assert!(html.contains("href=\"/docs/guide/?occurrence=1&amp;safe=yes\" title=\"title\""));
    assert!(html.contains("href=\"/docs/guide/?occurrence=2&amp;safe=yes\""));
    assert!(html.contains("<img src=\"guide.md\" alt=\"asset\""));
    assert!(html.contains("<code>[code](guide.md)</code>"));
    assert!(html_text::text(&html).contains("[example](guide.md)"));
    assert_eq!(parsed.inspect().expect("AST unchanged"), before);
    assert_eq!(parsed.source(), source);
    assert!(std::ptr::eq(parsed.source().as_ptr(), source.as_ptr()));
    assert!(!format!("{parsed:?}").contains("Native guide"));
}

#[test]
fn absent_mapping_is_identical_to_the_existing_renderer() {
    for source in [
        "",
        "# Heading\n\n**bold** [link](../guide.md#part) ![image](a.png)\n",
        "<https://example.invalid/> and [email](mailto:someone@example.invalid)",
        "[one][ref] [two][ref]\n\n[ref]: other.md\n",
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("parse");
        assert_eq!(
            parsed
                .render_html_resolving_links(|_, _| Ok(None))
                .expect("unchanged links"),
            parsed.render_html().expect("default renderer")
        );
    }
}

#[test]
fn all_original_safety_checks_finish_before_any_resolution_callback() {
    for suffix in [
        "[bad](javascript&#58;alert)",
        "[bad](file:///private/path)",
        "![bad](data:image/svg+xml,payload)",
        "raw <script>bad()</script>",
        "<!-- hidden -->",
    ] {
        let source = format!("[safe](guide.md)\n\n{suffix}");
        let parsed = ParsedMarkdown::parse(
            &source,
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("syntax");
        let original = parsed.render_html().expect_err("unsafe original source");
        let mut calls = 0;
        let error = parsed
            .render_html_resolving_links(|_, _| {
                calls += 1;
                Ok(Some("/safe/".into()))
            })
            .expect_err("mapping cannot launder unsafe input");
        assert_eq!(calls, 0, "{suffix}");
        assert_eq!(error, original, "same native safety failure");
    }
}

#[test]
fn mapped_destinations_must_pass_the_same_safety_gate() {
    let source = "prefix [safe](guide.md) suffix";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("syntax");
    let expected = Span { start: 7, end: 23 };
    assert_eq!(&source[expected.start..expected.end], "[safe](guide.md)");
    for mapped in [
        "javascript:alert(1)",
        "JaVaScRiPt:alert",
        "data:text/html,payload",
        "file:///private",
        "\\server\\share",
        "/safe/\nunsafe",
        "java\tscript:alert",
    ] {
        assert_eq!(
            parsed
                .render_html_resolving_links(|_, span| {
                    assert_eq!(span, expected);
                    Ok(Some(mapped.into()))
                })
                .expect_err("mapped unsafe URL"),
            Error::UnsafeUrl(expected),
            "{mapped:?}"
        );
    }
    let html = parsed
        .render_html_resolving_links(|_, _| {
            Ok(Some("https://example.invalid/docs/?a=1&b=2#part".into()))
        })
        .expect("safe mapped URL");
    assert!(html.contains("href=\"https://example.invalid/docs/?a=1&amp;b=2#part\""));
}

#[test]
fn callback_failure_keeps_exact_original_span_and_stops_later_callbacks() {
    let source = "[first](a.md) [second](missing.md) [last](c.md)";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("syntax");
    let mut calls = 0;
    let mut failure = None;
    let error = parsed
        .render_html_resolving_links(|destination, span| {
            calls += 1;
            if destination == "missing.md" {
                assert_eq!(&source[span.start..span.end], "[second](missing.md)");
                let error = Error::LinkResolution {
                    span,
                    reason: "configured target missing",
                };
                failure = Some(error.clone());
                Err(error)
            } else {
                Ok(Some("/resolved/".into()))
            }
        })
        .expect_err("no partial fragment");
    assert_eq!(calls, 2);
    assert_eq!(Some(error.clone()), failure);
    assert_eq!(error.code(), "markdown_link_resolution");
    assert_eq!(
        error.to_string(),
        "Markdown link at bytes 14..34 could not resolve: configured target missing; no HTML returned"
    );
    assert!(!error.to_string().contains("missing.md"));
}

#[test]
fn mapping_and_html_output_have_independent_aggregate_bounds() {
    let parsed = ParsedMarkdown::parse(
        "[a](a) [b](b) [c](c)",
        Limits {
            output_bytes: 128,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded source");
    let mut calls = 0;
    let error = parsed
        .render_html_resolving_links(|_, _| {
            calls += 1;
            Ok(Some("x".repeat(65)))
        })
        .expect_err("aggregate mappings exceed limit");
    assert_eq!(
        calls, 2,
        "do not resolve after the mapping ceiling is exhausted"
    );
    assert_eq!(error, Error::Limit("render plan string bytes"));

    let parsed = ParsedMarkdown::parse(
        "[a](a)",
        Limits {
            output_bytes: 128,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded source");
    assert_eq!(
        parsed
            .render_html_resolving_links(|_, _| Ok(Some("x".repeat(129))))
            .expect_err("one mapping exceeds ceiling"),
        Error::Limit("render plan string bytes")
    );
    assert_eq!(
        parsed
            .render_html_resolving_links(|_, _| Ok(Some("x".repeat(128))))
            .expect_err("exact mapping ceiling fits, but HTML framing does not"),
        Error::Limit("rendered output bytes")
    );
    let source = "[a](a)";
    let expected = "<p><a href=\"/docs/\">a</a></p>\n";
    for (limit, succeeds) in [(expected.len(), true), (expected.len() - 1, false)] {
        let parsed = ParsedMarkdown::parse(
            source,
            Limits {
                output_bytes: limit,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak,
        )
        .expect("bounded source");
        let rendered = parsed.render_html_resolving_links(|_, _| Ok(Some("/docs/".into())));
        if succeeds {
            assert_eq!(rendered.expect("exact output ceiling"), expected);
        } else {
            assert_eq!(
                rendered.expect_err("one byte under"),
                Error::Limit("rendered output bytes")
            );
        }
    }
}

#[test]
fn resolver_receives_parser_decoded_urls_not_markdown_source_spellings() {
    let source = "[link](guide&#46;md?one=1&amp;two=2)";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("syntax");
    let mut calls = 0;
    let html = parsed
        .render_html_resolving_links(|url, span| {
            calls += 1;
            assert_eq!(url, "guide.md?one=1&two=2");
            assert_eq!(
                span,
                Span {
                    start: 0,
                    end: source.len()
                }
            );
            Ok(Some("/resolved/".into()))
        })
        .expect("resolved decoded reference");
    assert_eq!(calls, 1);
    assert_eq!(html, "<p><a href=\"/resolved/\">link</a></p>\n");
}

#[test]
fn general_reference_resolution_retains_kind_order_spans_labels_and_original_inspection() {
    let source = "# Shared heading\n\n[first](old.md) ![A *styled* alt](old&#46;png \"caption\") [second][ref]\n\n`![code](absent.png)`\n\n```md\n[example](absent.md)\n```\n\n[ref]: other.md\n";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("retained AST");
    let before = parsed.inspect().expect("original inspection");
    let mut seen = Vec::new();
    let html = parsed
        .render_resolving_references(|kind, destination, span| {
            seen.push((
                kind,
                destination.to_owned(),
                source[span.start..span.end].to_owned(),
            ));
            Ok(Some(match kind {
                LinkKind::Link => "/docs/resolved/".into(),
                LinkKind::Image => "/assets/resolved.png?a=1&b=2".into(),
            }))
        })
        .expect("one general reference renderer")
        .body_html;
    assert_eq!(
        seen,
        vec![
            (LinkKind::Link, "old.md".into(), "[first](old.md)".into()),
            (
                LinkKind::Image,
                "old.png".into(),
                "![A *styled* alt](old&#46;png \"caption\")".into()
            ),
            (LinkKind::Link, "other.md".into(), "[second][ref]".into()),
        ]
    );
    assert!(html.contains("<a href=\"/docs/resolved/\">first</a>"));
    assert!(html.contains("<a href=\"/docs/resolved/\">second</a>"));
    assert!(html.contains(
        "src=\"/assets/resolved.png?a=1&amp;b=2\" alt=\"A styled alt\" title=\"caption\""
    ));
    assert!(html.contains("![code](absent.png)"));
    assert!(html_text::text(&html).contains("[example](absent.md)"));
    assert_eq!(parsed.inspect().expect("immutable syntax"), before);
    assert_eq!(parsed.source(), source);
    let mut links_only = Vec::new();
    let legacy = parsed
        .render_html_resolving_links(|destination, _| {
            links_only.push(destination.to_owned());
            Ok(Some("/links-only/".into()))
        })
        .expect("links-only delegation");
    assert_eq!(links_only, ["old.md", "other.md"]);
    assert!(legacy.contains("src=\"old.png\" alt=\"A styled alt\" title=\"caption\""));
    assert_eq!(
        parsed
            .render_resolving_references(|_, _, _| Ok(None))
            .expect("identity resolution")
            .body_html,
        parsed.render_html().expect("default renderer")
    );
}

#[test]
fn general_reference_mapping_enforces_image_specific_safety_and_exact_errors() {
    let source = "![asset](old.png)";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("image syntax");
    let expected = Span {
        start: 0,
        end: source.len(),
    };
    for destination in [
        "mailto:person@example.invalid",
        "javascript:alert",
        "data:image/png,payload",
        "file:///private/image.png",
        "/safe/\nunsafe",
    ] {
        let error = parsed
            .render_resolving_references(|kind, _, span| {
                assert_eq!(kind, LinkKind::Image);
                assert_eq!(span, expected);
                Ok(Some(destination.into()))
            })
            .expect_err("image mappings cannot use navigational-only or unsafe schemes");
        assert_eq!(error, Error::UnsafeUrl(expected));
    }
    let link = ParsedMarkdown::parse(
        "[contact](old.md)",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("link syntax");
    assert!(
        link.render_resolving_references(|kind, _, _| {
            assert_eq!(kind, LinkKind::Link);
            Ok(Some("mailto:person@example.invalid".into()))
        })
        .expect("mailto is navigational only")
        .body_html
        .contains("href=\"mailto:person@example.invalid\"")
    );
    let failure = Error::LinkResolution {
        span: expected,
        reason: "retained image absent",
    };
    assert_eq!(
        parsed
            .render_resolving_references(|_, _, _| Err(failure.clone()))
            .expect_err("exact image callback error"),
        failure
    );
}

#[test]
fn general_reference_callbacks_cannot_launder_any_unsafe_original() {
    for suffix in [
        "![bad](mailto:person@example.invalid)",
        "![bad](data:image/png,payload)",
        "[bad](javascript&#58;alert)",
        "<script>unsafe()</script>",
        "<!-- raw HTML -->",
        "![safe [bad](javascript&#58;alert)](actual.png)",
    ] {
        let source = format!("![safe](old.png) [safe](old.md)\n\n{suffix}");
        let parsed = ParsedMarkdown::parse(
            &source,
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .expect("syntax");
        let expected = parsed.render_html().expect_err("original safety gate");
        let mut callbacks = 0;
        let error = parsed
            .render_resolving_references(|_, _, _| {
                callbacks += 1;
                Ok(Some("/safe.png".into()))
            })
            .expect_err("all original checks must precede callbacks");
        assert_eq!(callbacks, 0, "{suffix}");
        assert_eq!(error, expected);
    }
}

#[test]
fn image_mappings_share_aggregate_destination_and_independent_html_limits() {
    let parsed = ParsedMarkdown::parse(
        "![a](a.png) [b](b.md) ![c](c.png)",
        Limits {
            output_bytes: 128,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded AST");
    let mut kinds = Vec::new();
    let error = parsed
        .render_resolving_references(|kind, _, _| {
            kinds.push(kind);
            Ok(Some("x".repeat(65)))
        })
        .expect_err("images and links use one aggregate mapping budget");
    assert_eq!(
        kinds,
        [LinkKind::Image],
        "resolved image metadata shares the render-plan budget before the next reference is allocated"
    );
    assert_eq!(error, Error::Limit("render plan string bytes"));
    let parsed = ParsedMarkdown::parse(
        "![a](a.png)",
        Limits {
            output_bytes: 128,
            ..Limits::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .expect("bounded image AST");
    for (bytes, expected) in [
        (129, Error::Limit("render plan string bytes")),
        (128, Error::Limit("render plan string bytes")),
    ] {
        assert_eq!(
            parsed
                .render_resolving_references(|_, _, _| Ok(Some("x".repeat(bytes))))
                .expect_err("image budget failure"),
            expected
        );
    }
    assert_eq!(
        parsed
            .render_resolving_references(|_, _, _| Ok(Some(format!("{}.png", "&".repeat(24)))))
            .expect_err("escaped HTML still has its independent output ceiling"),
        Error::Limit("rendered output bytes")
    );
    let expected =
        "<div data-gazette-image-block>\n<p><img src=\"/asset.png\" alt=\"alt\" /></p>\n</div>\n";
    for (limit, succeeds) in [(expected.len(), true), (expected.len() - 1, false)] {
        let parsed = ParsedMarkdown::parse(
            "![alt](old.png)",
            Limits {
                output_bytes: limit,
                ..Limits::default()
            },
            &gazette_adapter_markdown::Comrak,
        )
        .expect("bounded original");
        let result = parsed.render_resolving_references(|_, _, _| Ok(Some("/asset.png".into())));
        if succeeds {
            assert_eq!(
                result.expect("exact image HTML ceiling").body_html,
                expected
            );
        } else {
            assert_eq!(
                result.expect_err("one byte under"),
                Error::Limit("rendered output bytes")
            );
        }
    }
}

#[test]
fn image_alt_references_remain_syntax_without_becoming_emitted_destinations() {
    for (source, nested_kind, nested_url, alt) in [
        (
            "![foo [download](missing.pdf)](actual.png)",
            LinkKind::Link,
            "missing.pdf",
            "foo download",
        ),
        (
            "![foo ![nested](missing.png)](actual.png)",
            LinkKind::Image,
            "missing.png",
            "foo nested",
        ),
    ] {
        let parsed =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .expect("nested image syntax");
        let before = parsed.inspect().expect("complete syntactic references");
        assert_eq!(before.links.len(), 2);
        assert_eq!(before.links[0].kind, LinkKind::Image);
        assert_eq!(before.links[0].destination, "actual.png");
        assert_eq!(before.links[1].kind, nested_kind);
        assert_eq!(before.links[1].destination, nested_url);
        let mut seen = Vec::new();
        let html = parsed
            .render_resolving_references(|kind, destination, span| {
                seen.push((kind, destination.to_owned()));
                assert_eq!(&source[span.start..span.end], source);
                Ok(Some("/retained/actual.png".into()))
            })
            .expect("only the outer image has an emitted URL")
            .body_html;
        assert_eq!(seen, [(LinkKind::Image, "actual.png".into())]);
        assert_eq!(
            html,
            format!(
                "<div data-gazette-image-block>\n<p><img src=\"/retained/actual.png\" alt=\"{alt}\" /></p>\n</div>\n"
            )
        );
        assert!(!html.contains(nested_url));
        let mut link_calls = 0;
        let links_only = parsed
            .render_html_resolving_links(|_, _| {
                link_calls += 1;
                Err(Error::LinkResolution {
                    span: Span {
                        start: 0,
                        end: source.len(),
                    },
                    reason: "alt text is not navigation",
                })
            })
            .expect("link-only caller must not see image-alt syntax");
        assert_eq!(link_calls, 0);
        assert_eq!(
            links_only,
            format!(
                "<div data-gazette-image-block>\n<p><img src=\"actual.png\" alt=\"{alt}\" /></p>\n</div>\n"
            )
        );
        assert_eq!(
            parsed.inspect().expect("full original syntax retained"),
            before
        );
    }
}

#[test]
fn linked_image_resolves_both_outer_navigation_and_emitted_image() {
    let source = "[![cover](actual.png)](target.md)";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("linked image");
    let before = parsed.inspect().expect("original syntax");
    let mut seen = Vec::new();
    let html = parsed
        .render_resolving_references(|kind, destination, _| {
            seen.push((kind, destination.to_owned()));
            Ok(Some(
                match kind {
                    LinkKind::Link => "/target/",
                    LinkKind::Image => "/assets/actual.png",
                }
                .into(),
            ))
        })
        .expect("both actual destinations resolve")
        .body_html;
    assert_eq!(
        seen,
        [
            (LinkKind::Link, "target.md".into()),
            (LinkKind::Image, "actual.png".into())
        ]
    );
    assert_eq!(
        html,
        "<div data-gazette-image-block>\n<p><a href=\"/target/\"><img src=\"/assets/actual.png\" alt=\"cover\" /></a></p>\n</div>\n"
    );
    let mut links_only = Vec::new();
    let html = parsed
        .render_html_resolving_links(|destination, _| {
            links_only.push(destination.to_owned());
            Ok(Some("/target/".into()))
        })
        .expect("outer link still belongs to links-only caller");
    assert_eq!(links_only, ["target.md"]);
    assert_eq!(
        html,
        "<div data-gazette-image-block>\n<p><a href=\"/target/\"><img src=\"actual.png\" alt=\"cover\" /></a></p>\n</div>\n"
    );
    assert_eq!(parsed.inspect().expect("unchanged inspection"), before);
}

#[test]
fn email_autolinks_resolve_effective_mailto_without_double_prefix_or_changed_labels() {
    let source = "<downloads@example.pdf>";
    let parsed =
        ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
            .expect("email autolink");
    let before = parsed.inspect().expect("original syntax inspection");
    assert_eq!(before.links.len(), 1);
    assert_eq!(before.links[0].kind, LinkKind::Link);
    assert_eq!(before.links[0].destination, "mailto:downloads@example.pdf");
    let native = "<p><a href=\"mailto:downloads@example.pdf\">downloads@example.pdf</a></p>\n";
    for replacement in [
        None,
        Some("https://example.invalid/downloads.pdf"),
        Some("mailto:alternate@example.invalid"),
    ] {
        let mut calls = 0;
        let html = parsed
            .render_resolving_references(|kind, destination, span| {
                calls += 1;
                assert_eq!(kind, LinkKind::Link);
                assert_eq!(destination, "mailto:downloads@example.pdf");
                assert_eq!(
                    span,
                    Span {
                        start: 0,
                        end: source.len()
                    }
                );
                Ok(replacement.map(str::to_owned))
            })
            .expect("email autolink resolution")
            .body_html;
        assert_eq!(calls, 1);
        let expected = replacement.map_or_else(
            || native.to_owned(),
            |url| format!("<p><a href=\"{url}\">downloads@example.pdf</a></p>\n"),
        );
        assert_eq!(html, expected);
        assert_eq!(parsed.inspect().expect("syntax not rewritten"), before);
    }
    assert_eq!(
        parsed.render_html().expect("unmapped native rendering"),
        native
    );
    let mapped = parsed
        .render_html_resolving_links(|destination, _| {
            assert_eq!(destination, "mailto:downloads@example.pdf");
            Ok(Some("https://example.invalid/contact".into()))
        })
        .expect("links-only API shares email normalization");
    assert_eq!(
        mapped,
        "<p><a href=\"https://example.invalid/contact\">downloads@example.pdf</a></p>\n"
    );
}
