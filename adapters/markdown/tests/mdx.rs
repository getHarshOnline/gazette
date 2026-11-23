//! Real `CommonMark` inputs that must remain inert and readable in an MDX target.

use gazette_adapter_markdown::Comrak;
use gazette_core::{
    documentation::projection::manual_heading_bindings,
    markdown::{Error, Limits, LinkBinding, Profile},
    source::Sha256Digest,
};
use gazette_ports::markdown::{Document, Markdown};

fn parse(source: &str) -> Box<dyn Document> {
    Comrak
        .parse(source, Profile::Publication, Limits::default())
        .unwrap()
}

fn links(document: &dyn Document) -> Vec<LinkBinding> {
    document
        .syntax()
        .links
        .iter()
        .map(|link| LinkBinding {
            image_sources: None,
            svg_layout: None,
            image_width: None,
            image_dimensions: None,
            image_loading: None,
            image_fetch_priority: None,
            inline_svg: None,
            occurrence: link.clone(),
            destination: link.destination.clone(),
            image_address: None,
            annotation: None,
        })
        .collect()
}

#[test]
fn comments_indented_json_and_autolinks_keep_their_readable_meaning() {
    let source = "# Example\n\n<!-- generated-marker:begin -->\n\n[Next](./next.md)\n\n    {\n      \"workspace\": {\"path\": \"example.toml\"}\n    }\n\n<https://example.test/api>\n\nUse {value} and `keep {code}`.\n";
    let document = parse(source);
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    let mut selected = links(document.as_ref());
    selected[0].destination = "/module/next".into();
    let output = document
        .transport_mdx(
            document.syntax().identity,
            &selected,
            &headings,
            headings.first(),
        )
        .unwrap();
    let projected = parse(&output);
    assert!(
        projected.syntax().headings.is_empty(),
        "the target renders its configured title"
    );
    assert!(output.contains("<a id=\"example\"></a>"));
    assert!(!output.contains("<!--"));
    assert!(
        output.contains("```"),
        "indented code needs fenced MDX transport"
    );
    let code_blocks = |source: &str| {
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, source, &comrak::Options::default());
        root.descendants()
            .filter_map(|node| match &node.data().value {
                comrak::nodes::NodeValue::CodeBlock(code) => Some(code.literal.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(code_blocks(&output), code_blocks(source));
    assert!(!output.contains("<https://"));
    let destinations: Vec<_> = projected
        .syntax()
        .links
        .iter()
        .map(|link| link.destination.as_str())
        .collect();
    assert_eq!(destinations, ["/module/next", "https://example.test/api"]);
    let html = comrak::markdown_to_html(&output, &comrak::Options::default());
    assert!(html.contains("Use {value} and <code>keep {code}</code>."));
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(
        document
            .rewrite_links(document.syntax().identity, &links(document.as_ref()))
            .unwrap(),
        source
    );
}

#[test]
fn explicit_bound_heading_ids_survive_title_removal_and_duplicate_text() {
    let source = "# Overview\n\n## Overview\n\n## Config.json\n";
    let document = parse(source);
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    let output = document
        .transport_mdx(document.syntax().identity, &[], &headings, headings.first())
        .unwrap();
    let projected = parse(&output);
    let ids: Vec<_> = projected
        .syntax()
        .headings
        .iter()
        .map(|heading| heading.id.as_deref())
        .collect();
    assert_eq!(ids, [Some("overview-1"), Some("config-json")]);
    assert!(output.contains("<a id=\"overview\"></a>"));
}

#[test]
fn safe_unchanged_plain_reference_preserves_original_bytes() {
    let source = "A plain reference.\r\n";
    let document = parse(source);
    assert_eq!(
        document
            .transport_mdx(document.syntax().identity, &[], &[], None)
            .unwrap(),
        source
    );
}

#[test]
fn email_autolinks_are_markdown_links_in_the_target() {
    let source = "Contact <person@example.test>.\n";
    let document = parse(source);
    let selected = links(document.as_ref());
    let output = document
        .transport_mdx(document.syntax().identity, &selected, &[], None)
        .unwrap();
    assert!(!output.contains("<person@"));
    let projected = parse(&output);
    assert_eq!(
        projected.syntax().links[0].destination,
        "mailto:person@example.test"
    );
}

#[test]
fn inert_html_keeps_tags_attributes_and_literal_text_without_expressions() {
    let source = "<div data-note=\"{literal}\">\nRaw *stars* and {value}<br>\n</div>\n\nPress <kbd>Ctrl</kbd>.\n";
    let document = parse(source);
    let output = document
        .transport_mdx(document.syntax().identity, &[], &[], None)
        .unwrap();
    assert!(output.contains("<br />"));
    assert!(output.contains("<kbd>Ctrl</kbd>"));
    assert!(output.contains("data-note=\"&#123;literal&#125;\""));
    assert!(output.contains("&#42;stars&#42;"));
    assert!(output.contains("&#123;value&#125;"));
}

#[test]
fn wrong_source_and_incomplete_or_forged_heading_plans_fail_closed() {
    let document = parse("# Title\n\n[Next](./next)\n");
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    let selected = links(document.as_ref());
    let mut wrong = document.syntax().identity;
    wrong.source_sha256 = Sha256Digest::of(b"other");
    assert_eq!(
        document.transport_mdx(wrong, &selected, &headings, None),
        Err(Error::InvalidPlan)
    );
    assert_eq!(
        document.transport_mdx(document.syntax().identity, &[], &headings, None),
        Err(Error::InvalidPlan)
    );
    assert_eq!(
        document.transport_mdx(document.syntax().identity, &selected, &[], None),
        Err(Error::InvalidPlan)
    );
    let mut forged = headings.clone();
    forged[0].occurrence.text = "Other".into();
    assert_eq!(
        document.transport_mdx(document.syntax().identity, &selected, &forged, None),
        Err(Error::InvalidPlan)
    );
}

#[test]
fn title_transport_does_not_discard_an_authored_link() {
    let document = parse("# [Title](https://example.test/)\n");
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    assert!(matches!(
        document.transport_mdx(
            document.syntax().identity,
            &links(document.as_ref()),
            &headings,
            headings.first()
        ),
        Err(Error::Unsupported { .. })
    ));
}

#[test]
fn module_syntax_stays_literal_on_unchanged_and_reformatted_paths() {
    for statement in [
        "import value from 'example-module'",
        "export const pi = 3.14",
    ] {
        for change_link in [false, true] {
            let source = if change_link {
                format!("{statement}\n\n[Next](./next)\n")
            } else {
                format!("{statement}\n")
            };
            let document = parse(&source);
            let mut selected = links(document.as_ref());
            if change_link {
                selected[0].destination = "/resolved/next".into();
            }
            let output = document
                .transport_mdx(document.syntax().identity, &selected, &[], None)
                .unwrap();
            assert!(!output.trim_start().starts_with("import"));
            assert!(!output.trim_start().starts_with("export"));
            let projected = parse(&output);
            let text: String = projected
                .syntax()
                .text
                .iter()
                .map(|part| part.value.as_str())
                .collect();
            assert!(
                text.contains(statement),
                "literal module example changed: {text:?}"
            );
        }
    }
}

#[test]
fn html_boolean_presence_is_not_an_empty_jsx_string() {
    let source = "<details open=\"false\"><summary>More</summary>Text</details>\n\n<ol reversed=\"0\"><li>Two</li><li>One</li></ol>\n\n<div data-note=\"\"></div>\n";
    let document = parse(source);
    let output = document
        .transport_mdx(document.syntax().identity, &[], &[], None)
        .unwrap();
    assert!(output.contains("<details open>"));
    assert!(output.contains("<ol reversed>"));
    assert!(output.contains("data-note=\"\""));
    assert!(!output.contains("open=\"\""));
}

#[test]
fn unclosed_or_expression_like_html_fails_without_a_successful_subset() {
    for source in [
        "<p>One<p>Two\n",
        "<div><span>Text</div></span>\n",
        "<div><x.y>Text</x.y></div>\n",
    ] {
        let document = parse(source);
        assert!(
            matches!(
                document.transport_mdx(document.syntax().identity, &[], &[], None),
                Err(Error::Unsupported { .. })
            ),
            "{source:?}"
        );
    }
}

#[test]
fn a_preamble_prevents_title_removal_even_with_a_matching_heading_fact() {
    let document = parse("A substantive preamble.\n\n# Title\n");
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    assert_eq!(
        document.transport_mdx(document.syntax().identity, &[], &headings, headings.first()),
        Err(Error::InvalidPlan)
    );
}

#[test]
fn expanded_literal_output_is_bounded_without_partial_success() {
    let source = "Use {value}.\n";
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: source.len(),
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        document.transport_mdx(document.syntax().identity, &[], &[], None),
        Err(Error::Limit(_))
    ));
}

#[test]
fn leading_repository_alerts_have_readable_labels_without_provider_components() {
    for (marker, label) in [
        ("NOTE", "Note"),
        ("TIP", "Tip"),
        ("IMPORTANT", "Important"),
        ("WARNING", "Warning"),
        ("CAUTION", "Caution"),
    ] {
        let source =
            format!("> [!{marker}]\n> Keep {{value}}, `code` and [details](./details.md).\n");
        let document = parse(&source);
        let mut selected = links(document.as_ref());
        selected[0].destination = "/guide/details".into();
        let output = document
            .transport_mdx(document.syntax().identity, &selected, &[], None)
            .unwrap();
        assert!(
            output.contains(&format!("<strong>{label}</strong>")),
            "{output}"
        );
        assert!(!output.contains(&format!("[!{marker}]")));
        let projected = parse(&output);
        assert_eq!(projected.syntax().links[0].destination, "/guide/details");
        assert!(output.contains("`code`"));
        assert!(output.contains("\\{value\\}"));
        // Ordinary repository rewriting retains the original source dialect.
        assert_eq!(
            document
                .rewrite_links(document.syntax().identity, &links(document.as_ref()))
                .unwrap(),
            source
        );
    }
}

#[test]
fn heading_free_alerts_do_not_take_the_unchanged_mdx_fast_path() {
    let document = parse("> [!IMPORTANT]\n> This is a scaffold.\n");
    let output = document
        .transport_mdx(document.syntax().identity, &[], &[], None)
        .unwrap();
    assert!(output.contains("<strong>Important</strong>"));
    assert!(
        comrak::markdown_to_html(&output, &comrak::Options::default())
            .contains("This is a scaffold.")
    );
}

#[test]
fn alert_lookalikes_remain_literal_even_when_mdx_reformats_the_page() {
    for source in [
        "> \\[!IMPORTANT]\n> Escaped literal.\n",
        "> `[!IMPORTANT]`\n> Inline code.\n",
        "> Earlier text.\n> [!IMPORTANT]\n> Not the leading marker.\n",
        "> Earlier paragraph.\n>\n> [!IMPORTANT]\n> Not the first paragraph.\n",
        "> [!CUSTOM]\n> Unknown marker.\n",
        "> [!IMPORTANT] Same-line prose.\n",
        "[!IMPORTANT]\nOrdinary text.\n",
        "```md\n> [!IMPORTANT]\n> Fenced example.\n```\n",
    ] {
        // A changed link forces transport even for otherwise inert lookalikes.
        let source = format!("{source}\n[Next](./next.md)\n");
        let document = parse(&source);
        let mut selected = links(document.as_ref());
        selected[0].destination = "/next".into();
        let output = document
            .transport_mdx(document.syntax().identity, &selected, &[], None)
            .unwrap();
        assert!(!output.contains("<strong>"), "{source:?}: {output}");
        assert!(output.contains("IMPORTANT") || output.contains("CUSTOM"));
    }
}

#[test]
fn list_boundaries_use_target_comments_without_rewriting_code() {
    for source in [
        "# Lists\n\n- First\n\n1. Second\n",
        "# Lists\n\n- First\n\n```html\n<!-- end list -->\n```\n",
        "# Lists\n\n> - First\n>\n> 1. Second\n",
    ] {
        let document = parse(source);
        let headings = manual_heading_bindings(document.syntax(), "lists").unwrap();
        let output = document
            .transport_mdx(document.syntax().identity, &[], &headings, None)
            .unwrap();
        assert!(output.contains("{/* list boundary */}"), "{output}");
        assert_eq!(
            output.matches("<!-- end list -->").count(),
            usize::from(source.contains("```html")),
            "{output}"
        );
        let arena = comrak::Arena::new();
        let root = comrak::parse_document(&arena, &output, &comrak::Options::default());
        let code: Vec<_> = root
            .descendants()
            .filter_map(|node| match &node.data().value {
                comrak::nodes::NodeValue::CodeBlock(code) => Some(code.literal.clone()),
                _ => None,
            })
            .collect();
        if source.contains("```html") {
            assert_eq!(code, ["<!-- end list -->\n"]);
        }
    }
}

#[test]
fn native_math_keeps_formula_literals_in_mdx() {
    let source = "# Formula\n\nUse $D(\\cdot \\| \\cdot)$ and $B = (M_s, M_b)$, with $0 < \\varepsilon < 1$.\n\n$$\n\\phi(x) = \\theta_1 + x^2\n$$\n\n```math\nU(r) = \\frac{u(r)}{C(r)}\n```\n\n```tex\n$keep_{code}$\n<!-- end list -->\n```\n";
    let document = Comrak
        .parse(source, Profile::PublicationMath, Limits::default())
        .unwrap();
    let headings = manual_heading_bindings(document.syntax(), "formula").unwrap();
    let output = document
        .transport_mdx(document.syntax().identity, &[], &headings, None)
        .unwrap();
    let projected = Comrak
        .parse(&output, Profile::PublicationMath, Limits::default())
        .unwrap();
    let formulas = |document: &dyn Document| {
        document
            .syntax()
            .math
            .iter()
            .map(|math| (math.tex.trim().to_owned(), math.display))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        formulas(document.as_ref()),
        formulas(projected.as_ref()),
        "{output}"
    );
    assert!(output.contains("$D(\\cdot \\| \\cdot)$"), "{output}");
    assert!(!output.contains("```math"), "{output}");
    assert!(
        output.contains("```tex\n$keep_{code}$\n<!-- end list -->\n```"),
        "{output}"
    );
}

#[test]
fn display_math_keeps_its_native_inline_container() {
    for source in [
        "# Math\n\nBefore $$x_i^2$$ after.\n",
        "# Math\n\n*Before $$x_i^2$$ after.*\n",
        "# Math\n\n| Formula |\n| --- |\n| $$x_i^2$$ |\n",
    ] {
        let document = Comrak
            .parse(source, Profile::PublicationMath, Limits::default())
            .unwrap();
        let headings = manual_heading_bindings(document.syntax(), "math").unwrap();
        let output = document
            .transport_mdx(document.syntax().identity, &[], &headings, None)
            .unwrap();
        assert!(output.contains("$$x_i^2$$"), "{output}");
    }
}

#[test]
fn code_math_cannot_expose_target_syntax_through_dollar_delimiters() {
    for source in [
        "```math\nx\n$$\n<script>alert(1)</script>\n```\n",
        "$`x$ {expression}`$\n",
        r"$`x \$ {expression}`$",
        r"$x \$ {expression}$",
    ] {
        let document = Comrak
            .parse(source, Profile::PublicationMath, Limits::default())
            .unwrap();
        assert!(
            matches!(
                document.transport_mdx(document.syntax().identity, &[], &[], None),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
    let document = Comrak
        .parse(
            "```math\nx + \\$\n```\n",
            Profile::PublicationMath,
            Limits::default(),
        )
        .unwrap();
    let output = document
        .transport_mdx(document.syntax().identity, &[], &[], None)
        .unwrap();
    assert!(output.contains("x + \\$"), "{output}");
}

#[test]
fn qualified_deep_headings_keep_inline_children_and_projected_ids_without_changing_default_mdx() {
    let source = "##### content *strong* [link](https://example.test) and `code` {#content}\n\n###### content\n";
    let document = parse(source);
    let anchors = gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
    .unwrap();
    let original = manual_heading_bindings(document.syntax(), "deep").unwrap();
    let mut headings = original.clone();
    for heading in &mut headings {
        heading.id = anchors.id(&heading.id);
    }
    let output = document
        .transport_mdx_with_anchors(
            document.syntax().identity,
            &links(document.as_ref()),
            &headings,
            None,
            &anchors,
        )
        .unwrap();
    assert!(
        output.contains("<Heading level={5} id=\"gazette&#45;anchor&#45;content\" className=\"scroll-mt-(--scroll-mt)\">"),
        "{output}"
    );
    assert!(
        output.contains(
            "<Heading level={6} id=\"content&#45;1\" className=\"scroll-mt-(--scroll-mt)\">"
        ),
        "{output}"
    );
    assert!(output.contains("*strong*"));
    assert!(output.contains("[link](https://example.test)"));
    assert!(output.contains("`code`"));
    assert!(!output.contains("{#"));
    let legacy = document
        .transport_mdx(
            document.syntax().identity,
            &links(document.as_ref()),
            &original,
            None,
        )
        .unwrap();
    assert!(!legacy.contains("<Heading"));
    assert!(legacy.contains("#####"));
}

#[test]
fn qualified_target_refuses_authored_components_and_raw_deep_html_without_silent_loss() {
    let anchors = gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
    .unwrap();
    for source in [
        "<h5 id=\"deep\">Label</h5>\n",
        "<h6>Label</h6>\n",
        "<Heading level={5} id=\"deep\">Label</Heading>\n",
    ] {
        let document = parse(source);
        assert!(
            matches!(
                document.transport_mdx_with_anchors(
                    document.syntax().identity,
                    &[],
                    &[],
                    None,
                    &anchors
                ),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn qualified_heading_construction_is_bounded_before_a_result_is_returned() {
    let source = "##### Small\n";
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: 32,
                ..Limits::default()
            },
        )
        .unwrap();
    let headings = manual_heading_bindings(document.syntax(), "deep").unwrap();
    let anchors = gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
    .unwrap();
    assert!(matches!(
        document.transport_mdx_with_anchors(
            document.syntax().identity,
            &[],
            &headings,
            None,
            &anchors
        ),
        Err(Error::Limit(_))
    ));
}

fn mintlify_transport(document: &dyn Document, selected: &[LinkBinding]) -> Result<String, Error> {
    let anchors = gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
    .unwrap();
    let mut headings = manual_heading_bindings(document.syntax(), "callouts").unwrap();
    for heading in &mut headings {
        heading.id = anchors.id(&heading.id);
    }
    document.transport_mdx_with_anchors(
        document.syntax().identity,
        selected,
        &headings,
        None,
        &anchors,
    )
}

fn transported_html(source: &str) -> String {
    let mut options = comrak::Options::default();
    // Inspect the already transported inert HTML as text too; Comrak's default
    // HTML renderer would omit it. This string projection executes nothing.
    options.render.r#unsafe = true;
    comrak::markdown_to_html(source, &options)
}

#[test]
fn mintlify_alerts_use_fixed_native_callouts_with_checked_children() {
    for (marker, component, legacy_label) in [
        ("NOTE", "Note", "Note"),
        ("TIP", "Tip", "Tip"),
        ("IMPORTANT", "Info", "Important"),
        ("WARNING", "Warning", "Warning"),
        ("CAUTION", "Danger", "Caution"),
    ] {
        let source = format!(
            "> [!{marker}]\n> Keep {{value}}, *emphasis*, `code` and [details](./details.md).\n>\n> Another paragraph.\n>\n> - A child item.\n"
        );
        let document = parse(&source);
        let mut selected = links(document.as_ref());
        selected[0].destination = "/guide/details".into();
        let output = mintlify_transport(document.as_ref(), &selected).unwrap();
        assert!(output.contains(&format!("<{component}>")), "{output}");
        assert!(output.contains(&format!("</{component}>")), "{output}");
        assert!(!output.contains("[!"), "{output}");
        assert!(!output.contains("<strong>"), "{output}");
        assert!(output.contains("\\{value\\}"), "{output}");
        assert!(output.contains("*emphasis*"), "{output}");
        assert!(output.contains("`code`"), "{output}");
        let html = transported_html(&output);
        assert!(html.contains("Another paragraph."), "{output}");
        assert!(html.contains("A child item."), "{output}");
        let projected = parse(&output);
        assert_eq!(projected.syntax().links[0].destination, "/guide/details");
        let legacy = document
            .transport_mdx(document.syntax().identity, &selected, &[], None)
            .unwrap();
        assert!(legacy.contains(&format!("<strong>{legacy_label}</strong>")));
        assert!(!legacy.contains(&format!("<{component}>")));
        assert_eq!(
            document
                .rewrite_links(document.syntax().identity, &links(document.as_ref()))
                .unwrap(),
            source
        );
        assert_eq!(
            document.syntax().identity.source_sha256,
            Sha256Digest::of(source.as_bytes())
        );
    }
}

#[test]
fn mintlify_nested_alerts_keep_ordinary_quotes_lists_and_fenced_literals() {
    let source = "> Outer quote.\n>\n> > [!TIP]\n> > A nested tip.\n>\n> Still quoted.\n\n- List item.\n\n  > [!WARNING]\n  > A warning in a list.\n\n> [!NOTE]\n>\n> > Ordinary child quote.\n>\n> > [!CAUTION]\n> > A nested danger.\n>\n> ```md\n> > [!IMPORTANT]\n> > Fenced literal.\n> ```\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &[]).unwrap();
    for component in ["Tip", "Warning", "Note", "Danger"] {
        assert_eq!(
            output.matches(&format!("<{component}>")).count(),
            1,
            "{output}"
        );
        assert_eq!(
            output.matches(&format!("</{component}>")).count(),
            1,
            "{output}"
        );
    }
    let html = transported_html(&output);
    for text in [
        "Outer quote.",
        "Still quoted.",
        "Ordinary child quote.",
        "A nested tip.",
        "A warning in a list.",
        "A nested danger.",
        "List item.",
    ] {
        assert_eq!(html.matches(text).count(), 1, "{output}");
    }
    let arena = comrak::Arena::new();
    let tree = comrak::parse_document(&arena, &output, &comrak::Options::default());
    assert_eq!(
        tree.descendants()
            .filter(|node| matches!(node.data().value, comrak::nodes::NodeValue::BlockQuote))
            .count(),
        2,
        "the outer ordinary quote and ordinary child quote must remain intact: {output}"
    );
    assert_eq!(
        tree.descendants()
            .filter(|node| matches!(node.data().value, comrak::nodes::NodeValue::List(_)))
            .count(),
        1,
        "{output}"
    );
    let code: Vec<_> = tree
        .descendants()
        .filter_map(|node| match &node.data().value {
            comrak::nodes::NodeValue::CodeBlock(code) => Some(code.literal.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(code, ["> [!IMPORTANT]\n> Fenced literal.\n"]);
}

#[test]
fn mintlify_marker_only_and_empty_first_paragraph_alerts_remain_balanced() {
    for source in [
        "> [!NOTE]\n",
        "> [!NOTE]\n>\n",
        "> [!NOTE]\n>\n> A later paragraph.\n",
    ] {
        let document = parse(source);
        let output = mintlify_transport(document.as_ref(), &[]).unwrap();
        assert_eq!(output.matches("<Note>").count(), 1, "{output}");
        assert_eq!(output.matches("</Note>").count(), 1, "{output}");
        assert!(!output.contains("[!NOTE]"));
        if source.contains("A later paragraph.") {
            let html = transported_html(&output);
            assert!(html.contains("A later paragraph."), "{output}");
        }
    }
}

#[test]
fn mintlify_alert_lookalikes_never_become_provider_components() {
    for (source, preserved) in [
        ("> \\[!NOTE]\n> Escaped literal.\n", "Escaped literal."),
        ("> `[!TIP]`\n> Inline code.\n", "Inline code."),
        (
            "> Earlier text.\n> [!IMPORTANT]\n> Later marker.\n",
            "Later marker.",
        ),
        (
            "> Earlier paragraph.\n>\n> [!WARNING]\n> Later paragraph.\n",
            "Later paragraph.",
        ),
        ("> [!CUSTOM]\n> Unknown marker.\n", "Unknown marker."),
        ("> [!NOTE\n> Incomplete marker.\n", "Incomplete marker."),
        ("> [!NOTE]]\n> Extra delimiter.\n", "Extra delimiter."),
        ("> [!CAUTION] Same-line prose.\n", "Same-line prose."),
        ("[!NOTE]\nOrdinary text.\n", "Ordinary text."),
        (
            "```md\n> [!NOTE]\n> Fenced example.\n```\n",
            "Fenced example.",
        ),
        (
            "<Note>Authored text and {expression}.</Note>\n",
            "Authored text",
        ),
    ] {
        let source = format!("{source}\n[Next](./next.md)\n");
        let document = parse(&source);
        let mut selected = links(document.as_ref());
        selected[0].destination = "/next".into();
        let output = mintlify_transport(document.as_ref(), &selected).unwrap();
        for component in ["Note", "Tip", "Info", "Warning", "Danger"] {
            assert!(
                !output.contains(&format!("<{component}>")),
                "{source:?}: {output}"
            );
        }
        assert!(output.contains("[Next](/next)"), "{output}");
        let html = transported_html(&output);
        assert!(html.contains(preserved), "{output}");
    }
}

#[test]
fn mintlify_alerts_keep_crlf_unicode_and_reject_incomplete_child_link_plans() {
    let source = "> [!IMPORTANT]\r\n> Café 🧭 [Next](./next.md).\r\n";
    let document = parse(source);
    assert_eq!(
        mintlify_transport(document.as_ref(), &[]),
        Err(Error::InvalidPlan)
    );
    let selected = links(document.as_ref());
    let output = mintlify_transport(document.as_ref(), &selected).unwrap();
    assert!(output.contains("<Info>"), "{output}");
    assert!(output.contains("Café 🧭"), "{output}");
    assert_eq!(parse(&output).syntax().links[0].destination, "./next.md");
    assert_eq!(
        document.syntax().identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
}

#[test]
fn mintlify_callout_wrappers_consume_the_complete_output_budget() {
    let source = "> [!NOTE]\n> Small.\n";
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: source.len(),
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        mintlify_transport(document.as_ref(), &[]),
        Err(Error::Limit(_))
    ));
}

#[test]
fn explicit_layouts_preserve_checked_links_headings_code_and_nested_disclosures() {
    let source = "# Architecture\n\n:::::tabs\n::::tab \"Rust\"\n\n## Client\n\nUse [the guide](./guide.md#client) and `Config { enabled: true }`.\n\n:::accordion \"Why capture once?\"\n\nRead one version before assembling.\n\n> [!TIP]\n> Keep the selected input.\n\n:::\n\n::::\n::::tab \"JavaScript\"\n\n```js Client.js\nconst client = { enabled: true };\n```\n\n::::\n:::::\n";
    let document = parse(source);
    let mut selected = links(document.as_ref());
    selected[0].destination = "/reference/guide#client".into();
    let output = mintlify_transport(document.as_ref(), &selected).unwrap();
    for tag in [
        "Tabs",
        r#"Tab title="Rust" id="gazette-layout-0""#,
        r#"Tab title="JavaScript" id="gazette-layout-2""#,
        r#"Accordion title="Why capture once&#92;&#63;" id="gazette-layout-1" className="gazette-layout-disclosure" _disabled"#,
        "Tip",
    ] {
        assert!(output.contains(&format!("<{tag}>")), "{output}");
    }
    assert!(
        output.contains("[the guide](/reference/guide#client)"),
        "{output}"
    );
    assert!(output.contains("## Client {#client}"), "{output}");
    assert!(output.contains("`Config { enabled: true }`"), "{output}");
    assert!(
        output.contains("const client = { enabled: true };"),
        "{output}"
    );
    assert!(!output.contains("::::"), "{output}");
    assert_eq!(
        mintlify_transport(document.as_ref(), &[]),
        Err(Error::InvalidPlan)
    );
    assert!(matches!(
        document.transport_mdx(
            document.syntax().identity,
            &selected,
            &manual_heading_bindings(document.syntax(), "layout").unwrap(),
            None
        ),
        Err(Error::Unsupported { .. })
    ));
    assert_eq!(
        document
            .rewrite_links(document.syntax().identity, &links(document.as_ref()))
            .unwrap(),
        source
    );
}

#[test]
fn explicit_steps_code_groups_frames_and_accordions_keep_their_children() {
    let source = ":::::steps\n::::step \"Capture\"\n\nRead the input.\n\n::::\n::::step \"Assemble\"\n\n:::code-group\n```rust Rust\nlet enabled = true;\n```\n```javascript JavaScript\nconst enabled = true;\n```\n:::\n\n::::\n:::::\n\n::::accordion-group\n:::accordion \"Recovery\"\n\nKeep [the source](./source.md).\n\n:::\n::::\n\n:::frame\n\n![Architecture](./architecture.svg)\n\n:::\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &links(document.as_ref())).unwrap();
    for component in ["Steps", "CodeGroup", "AccordionGroup", "Frame"] {
        assert!(output.contains(&format!("<{component}>")), "{output}");
        assert!(output.contains(&format!("</{component}>")), "{output}");
    }
    assert!(
        output.contains(r#"<Step title="Capture" id="gazette-layout-0">"#),
        "{output}"
    );
    assert!(
        output.contains(r#"<Step title="Assemble" id="gazette-layout-1">"#),
        "{output}"
    );
    assert!(output.contains("[the source](./source.md)"), "{output}");
    assert!(
        output.contains("![Architecture](./architecture.svg)"),
        "{output}"
    );
}

#[test]
fn theme_reference_renders_steps_and_preserves_its_tera_examples() {
    let source = include_str!("../../../docs/theme-reference.md");
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &links(document.as_ref())).unwrap();
    assert_eq!(output.matches("<Steps>").count(), 1);
    assert_eq!(output.matches("</Steps>").count(), 1);
    for title in [
        "Declare a page entry",
        "Write the page template",
        "Render a page",
    ] {
        assert!(
            output.contains(&format!("<Step title=\"{title}\"")),
            "{title}"
        );
    }
    assert_eq!(output.matches("</Step>").count(), 3);
    let examples: Vec<_> = source
        .split("```html\n")
        .skip(1)
        .map(|block| block.split("```").next().unwrap())
        .collect();
    assert_eq!(examples.len(), 2);
    for example in examples {
        assert!(
            output.contains(example),
            "Tera example changed during documentation rendering"
        );
    }
}

#[test]
fn cards_use_the_checked_markdown_destination_and_preserve_description() {
    let source = "::::card-group\n:::card\n[Build the SDK](./sdk.md#build)\n\nCompile the library and run your first integration.\n:::\n:::card\n[Connect an agent](./agent.md)\n\nUse your local server.\n:::\n::::\n";
    let document = parse(source);
    let mut selected = links(document.as_ref());
    selected[0].destination = "http://127.0.0.1:3909/library?view=code#build".into();
    let output = mintlify_transport(document.as_ref(), &selected).unwrap();
    assert!(output.contains("<CardGroup>"), "{output}");
    assert!(output.contains("title=\"Build the SDK\""), "{output}");
    assert!(output.contains("href=\"http&#58;&#47;&#47;127&#46;0&#46;0&#46;1&#58;3909&#47;library&#63;view&#61;code&#35;build\""), "{output}");
    let visible = comrak::markdown_to_html(&output, &comrak::Options::default());
    assert!(
        visible.contains("Compile the library and run your first integration."),
        "{visible}"
    );
    assert!(visible.contains("Use your local server."));
    assert_eq!(output.matches("<Card ").count(), 2);
    assert_eq!(output.matches("</Card>").count(), 2);
    assert!(!output.contains("[Build the SDK]"));
    assert_eq!(
        mintlify_transport(document.as_ref(), &[]),
        Err(Error::InvalidPlan)
    );
    assert_eq!(
        document
            .rewrite_links(document.syntax().identity, &links(document.as_ref()))
            .unwrap(),
        source
    );
}

#[test]
fn cards_refuse_ambiguous_navigation_and_untrusted_component_headers() {
    for source in [
        ":::card\nNo destination.\n:::\n",
        ":::card {\"href\":\"https://example.test\"}\n[Read](./read.md)\n:::\n",
        ":::card\n[One](./one.md) and [Two](./two.md)\n:::\n",
        ":::card\n[Read](./read.md)\n\nFollow [another target](./other.md).\n:::\n",
        "::::card-group\nA paragraph instead of cards.\n::::\n",
    ] {
        let document = parse(source);
        assert!(
            matches!(
                mintlify_transport(document.as_ref(), &links(document.as_ref())),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn layout_ids_do_not_steal_bound_heading_or_entity_encoded_html_anchors() {
    let source = "# IDs\n\n## Gazette layout 0\n\n<a id=\"gazette&#45;layout&#45;1\"></a>\n\n::::tabs\n:::tab \"IDs\"\n\n## IDs\n\nKept heading.\n\n:::\n::::\n\n:::accordion \"Gazette layout 0\"\n\nKept detail.\n\n:::\n\n::::steps\n:::step \"IDs\"\n\nKept action.\n\n:::\n::::\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &[]).unwrap();
    assert!(
        output.contains("## Gazette layout 0 {#gazette-layout-0}"),
        "{output}"
    );
    assert!(
        output.contains("<a id=\"gazette&#45;layout&#45;1\"></a>"),
        "{output}"
    );
    assert!(
        output.contains(r#"<Tab title="IDs" id="gazette-layout-2">"#),
        "{output}"
    );
    assert!(
        output.contains(r#"<Accordion title="Gazette layout 0" id="gazette-layout-3" className="gazette-layout-disclosure" _disabled>"#),
        "{output}"
    );
    assert!(
        output.contains(r#"<Step title="IDs" id="gazette-layout-4">"#),
        "{output}"
    );
    assert!(output.contains("## IDs {#ids-1}"), "{output}");
}

#[test]
fn layout_names_in_html_are_refused_only_for_the_qualified_target() {
    let anchors = gazette_core::documentation::projection::AnchorProjection::new(
        "gazette-anchor-",
        &["content", "page-title"],
    )
    .unwrap();
    for name in [
        "Tab",
        "Tabs",
        "Step",
        "Steps",
        "CodeGroup",
        "code-group",
        "Accordion",
        "AccordionGroup",
        "accordion-group",
        "Frame",
        "Card",
        "CardGroup",
        "card-group",
    ] {
        let source = format!("<{name} title={{value}}>A body</{name}>\n");
        let document = parse(&source);
        assert!(
            matches!(
                document.transport_mdx_with_anchors(
                    document.syntax().identity,
                    &[],
                    &[],
                    None,
                    &anchors,
                ),
                Err(Error::Unsupported { .. })
            ),
            "{name}"
        );
    }
    // Generic MDX retains its existing inert-HTML behavior. The additional
    // refusal is a qualification of the Mintlify renderer, not a JSX parser.
    let document = parse("<Tab title={value}>A body</Tab>\n");
    let output = document
        .transport_mdx(document.syntax().identity, &[], &[], None)
        .unwrap();
    assert!(
        output.contains("<tab title=\"&#123;value&#125;\">A body</tab>"),
        "{output}"
    );
}

#[test]
fn qualified_layouts_preserve_inert_html_attribute_values_as_data() {
    let source = ":::accordion \"Details\"\n\n<span title={value}>Literal value</span>\n\n:::\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &[]).unwrap();
    assert!(
        output.contains("<span title=\"&#123;value&#125;\">Literal value</span>"),
        "{output}"
    );
    assert!(!output.contains("title={value}"), "{output}");
}

#[test]
fn layout_headers_are_data_and_cannot_supply_component_properties_or_expressions() {
    let source = ":::accordion \"C++ \\\"client\\\" {value} 🧭 [Guide](./guide.md) <em>literal</em>\"\n\nKept body.\n\n:::\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &[]).unwrap();
    assert!(
        output.contains(
            r#"title="C&#92;&#43;&#92;&#43; &#92;&#34;client&#92;&#34; &#92;&#123;value&#92;&#125; 🧭 &#92;&#91;Guide&#92;&#93;&#92;&#40;&#92;&#46;&#92;&#47;guide&#92;&#46;md&#92;&#41; &#92;&#60;em&#92;&#62;literal&#92;&#60;&#92;&#47;em&#92;&#62;""#
        ),
        "{output}"
    );
    for source in [
        ":::tab {\"title\":\"Rust\",\"onClick\":\"run()\"}\nBody\n:::\n",
        ":::accordion {expression}\nBody\n:::\n",
        ":::accordion \"\"\nBody\n:::\n",
        ":::tabs sync={false}\nBody\n:::\n",
        ":::unknown \"Title\"\nBody\n:::\n",
    ] {
        let document = parse(source);
        assert!(
            matches!(
                mintlify_transport(document.as_ref(), &[]),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn invalid_layout_nesting_and_unclosed_groups_fail_instead_of_swallowing_content() {
    for source in [
        ":::tab \"Orphan\"\nBody\n:::\n",
        ":::step \"Orphan\"\nBody\n:::\n",
        "::::tabs\nA paragraph is not a tab.\n::::\n",
        "::::tabs\n:::tab \"Same\"\nOne\n:::\n:::tab \"Same\"\nTwo\n:::\n::::\n",
        ":::code-group\n```rust\nlet a = 1;\n```\n:::\n",
        ":::code-group\nProse is not a code sample.\n:::\n",
        "::::tabs\n:::tab \"Open\"\nA body without closing fences.\n",
        "::::tabs\n:::tab \"Open\"\nA shared final fence.\n::::\n",
        ":::accordion \"Open\"\nA non-closing lookalike.\n:::\u{a0}\n",
    ] {
        let document = parse(source);
        assert!(
            matches!(
                mintlify_transport(document.as_ref(), &[]),
                Err(Error::Unsupported { .. })
            ),
            "{source}"
        );
    }
}

#[test]
fn layout_blocks_keep_native_quote_list_and_crlf_containers() {
    let source = "> :::accordion \"Café 🧭\"\r\n>\r\n> Read [the guide](./guide.md).\r\n>\r\n> :::\r\n\r\n- A list item.\r\n\r\n  :::frame\r\n\r\n  Keep this figure in the list.\r\n\r\n  :::\r\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &links(document.as_ref())).unwrap();
    assert!(
        output.contains(r#"<Accordion title="Café 🧭" id="gazette-layout-0" className="gazette-layout-disclosure" _disabled>"#),
        "{output}"
    );
    assert!(output.contains("<Frame>"), "{output}");
    let html = transported_html(&output);
    assert!(html.contains("<blockquote>"), "{output}");
    assert!(html.contains("<li>"), "{output}");
    assert!(html.contains("Keep this figure in the list."), "{output}");
    assert_eq!(parse(&output).syntax().links[0].destination, "./guide.md");
}

#[test]
fn fenced_and_escaped_layout_examples_remain_literal_and_layout_output_is_bounded() {
    let source = "```md\n:::accordion \"Example\"\nBody\n:::\n```\n\n\\:::tabs\n";
    let document = parse(source);
    let output = mintlify_transport(document.as_ref(), &[]).unwrap();
    assert!(!output.contains("<Accordion"), "{output}");
    assert!(!output.contains("<Tabs>"), "{output}");
    assert!(output.contains(":::accordion"), "{output}");
    let source = ":::accordion \"A long title\"\nBody.\n:::\n";
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                output_bytes: source.len(),
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        mintlify_transport(document.as_ref(), &[]),
        Err(Error::Limit(_))
    ));
}

#[test]
fn admitted_projection_text_keeps_native_meaning_and_omits_unused_footnotes() {
    let source = "# Title\n\nVisible **words** and `literal {code}`.[^used]\n\n<div>HTML &amp; meaning</div>\n\n[^used]: Retained note.\n\n[^unused]: Hidden unused note.\n";
    let document = parse(source);
    let headings = manual_heading_bindings(document.syntax(), "fixture").unwrap();
    let projected = document
        .project_mdx(
            document.syntax().identity,
            &links(document.as_ref()),
            &headings,
            headings.first(),
            None,
        )
        .unwrap();
    let old = document
        .transport_mdx(
            document.syntax().identity,
            &links(document.as_ref()),
            &headings,
            headings.first(),
        )
        .unwrap();
    assert_eq!(projected.body, old);
    let text = projected.text.unwrap();
    assert!(
        text.contains("Title")
            && text.contains("Visible words")
            && text.contains("literal {code}")
            && text.contains("Retained note."),
        "{text}"
    );
    assert_eq!(text.matches("HTML & meaning").count(), 1, "{text}");
    assert!(
        !text.contains("Hidden unused") && !text.contains("<div>"),
        "{text}"
    );
}

#[test]
fn admitted_projection_text_retains_layout_titles_without_constructor_syntax() {
    let source = "::::tabs\n:::tab \"First panel\"\nPanel words.\n:::\n::::\n";
    let document = parse(source);
    let anchors =
        gazette_core::documentation::projection::AnchorProjection::new("owned-", &["content"])
            .unwrap();
    let output = document
        .project_mdx(document.syntax().identity, &[], &[], None, Some(&anchors))
        .unwrap();
    let text = output.text.unwrap();
    assert!(
        text.contains("First panel") && text.contains("Panel words."),
        "{text}"
    );
    assert!(!text.contains(":::tab") && !text.contains("<Tabs"));
}
