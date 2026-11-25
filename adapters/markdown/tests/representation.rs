//! Markdown alternates consume the real admitted plan, not retained source export.

use gazette_adapter_markdown::Comrak;
use gazette_core::markdown::{
    ComponentBinding, Error, Excerpt, FootnoteBinding, Fragment, HeadingBinding, Limits,
    LinkAnnotation, LinkBinding, LinkDestinationKind, LinkRelationship, MathBinding, Plan, Profile,
    Syntax,
};
use gazette_core::source::{Location, Precision, Sha256Digest, Span};
use gazette_ports::markdown::{ComponentRenderer, Document, Markdown};
use std::collections::BTreeMap;

// Qualify the complete reader output with the same bounded native tree storage;
// no public adapter API or parallel HTML grammar is introduced for tests.
#[path = "../src/html_tree.rs"]
mod html_tree;

#[derive(Debug, PartialEq, Eq)]
enum Fact {
    Element(
        String,
        String,
        BTreeMap<(String, String), String>,
        Vec<Fact>,
    ),
    Text(String),
    Comment(String),
    Pi(String, String),
    Document(Vec<Fact>),
}

fn fact(node: &html_tree::Handle) -> Fact {
    use html_tree::Kind;
    match &node.kind {
        Kind::Document => Fact::Document(node.children.borrow().iter().map(fact).collect()),
        Kind::Element {
            name,
            attrs,
            template,
            ..
        } => Fact::Element(
            name.ns.to_string(),
            name.local.to_string(),
            attrs
                .borrow()
                .iter()
                .map(|a| {
                    (
                        (a.name.ns.to_string(), a.name.local.to_string()),
                        a.value.to_string(),
                    )
                })
                .collect(),
            template
                .as_ref()
                .unwrap_or(node)
                .children
                .borrow()
                .iter()
                .map(fact)
                .collect(),
        ),
        Kind::Text(value) => Fact::Text(value.borrow().to_string()),
        Kind::Comment(value) => Fact::Comment(value.to_string()),
        Kind::Pi(target, data) => Fact::Pi(target.to_string(), data.to_string()),
    }
}

fn tree_facts(html: &str) -> Vec<Fact> {
    let tree = html_tree::parse(html, Limits::default()).unwrap();
    tree.root.children.borrow().iter().map(fact).collect()
}

fn decoded_text(html: &str) -> String {
    fn collect(value: &Fact, output: &mut String) {
        match value {
            Fact::Text(text) => output.push_str(text),
            Fact::Element(_, _, _, children) | Fact::Document(children) => {
                for child in children {
                    collect(child, output);
                }
            }
            Fact::Comment(_) | Fact::Pi(_, _) => {}
        }
    }
    let mut text = String::new();
    for value in tree_facts(html) {
        collect(&value, &mut text);
    }
    text
}

fn parse(source: &str, profile: Profile) -> Box<dyn Document> {
    Comrak.parse(source, profile, Limits::default()).unwrap()
}

fn plan(syntax: &Syntax) -> Plan {
    Plan {
        identity: syntax.identity,
        headings: syntax.headings.iter().enumerate().map(|(index, h)| HeadingBinding {
            occurrence: h.clone(),
            id: format!("notes-heading-{index}"),
            level: h.level.saturating_add(1).min(6),
        }).collect(),
        links: syntax.links.iter().filter(|link| link.emitted).map(|link| LinkBinding {
                    image_sources: None,
            svg_layout: None,
                    image_width: None,
                    image_dimensions: None,
            image_loading: None,
            image_fetch_priority: None,
            inline_svg: None,
            image_address: None,
            occurrence: link.clone(),
            destination: link.destination.clone(),
            annotation: None,
        }).collect(),
        footnotes: syntax.footnotes.iter().enumerate().map(|(index, note)| FootnoteBinding {
            occurrence: note.clone(),
            alias: format!("notes-note-{index}"),
        }).collect(),
        components: syntax.directives.iter().filter(|d| d.emitted).map(|d| ComponentBinding { container_contents: None, reading_prefix: None,
                presentation_only: false,
            occurrence: d.clone(),
            name: "callout".parse().unwrap(),
            arguments: BTreeMap::new(),
            context: None,
        }).collect(),
        math: syntax.math.iter().filter(|m| m.emitted).map(|m| MathBinding {
            occurrence: m.clone(),
            mathml: format!(
                "<math xmlns=\"http://www.w3.org/1998/Math/MathML\"{}><semantics><mi>x</mi><annotation encoding=\"application/x-tex\">{}</annotation></semantics></math>",
                if m.display { " display=\"block\"" } else { "" }, m.tex
            ),
        }).collect(),
        excerpt: syntax.html.first().map(|marker| Excerpt {
            marker: marker.clone(),
            continuation: "body-continuation".into(),
        }),
    }
}

// A reader parses the alternate as ordinary Markdown + inert HTML. This helper
// does not feed generated HTML back into Gazette's authored-source admission.
fn reader_html(markdown: &str) -> String {
    let arena = comrak::Arena::new();
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.footnotes = true;
    options.render.r#unsafe = true;
    let root = comrak::parse_document(&arena, markdown, &options);
    let mut html = String::new();
    comrak::format_html(root, &options, &mut html).unwrap();
    html
}

struct Renderer;
impl ComponentRenderer for Renderer {
    fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
        Ok(format!("<aside class=\"note\">{}</aside>\n", body.html))
    }
}

#[test]
fn alert_alternates_keep_the_same_semantics_without_reader_extensions() {
    let source = "> [!IMPORTANT]\n> Keep **meaning**, `code` and [details](./old).\n>\n> > An ordinary quote.\n>\n> - First\n> - Second\n\n> [!CAUTION]\n> A second alert.\n";
    let document = parse(source, Profile::Publication);
    let before = document.syntax().clone();
    let mut selected = plan(&before);
    selected.links[0].destination = "https://example.test/details/".into();
    let markdown = document.render_markdown(&selected, None).unwrap();
    assert!(markdown.contains("**Important**"), "{markdown}");
    assert!(markdown.contains("**Caution**"), "{markdown}");
    assert!(!markdown.contains("[!IMPORTANT]"));
    let html = reader_html(&markdown);
    for part in [
        "<strong>meaning</strong>",
        "<code>code</code>",
        "href=\"https://example.test/details/\"",
        "An ordinary quote.",
        "First",
        "Second",
        "A second alert.",
    ] {
        assert!(html.contains(part), "{part}: {html}");
    }
    assert_eq!(html.matches("<li>").count(), 2);
    assert!(!markdown.contains("markdown-alert"));
    assert_eq!(document.syntax(), &before);
}

#[test]
fn native_markdown_formats_resolved_resources_structure_and_subordinate_heading_identity() {
    let source = "# Notes {#authored .tone}\n\n[read](./old) and ![diagram](old.png \"Figure\").\n\n> retained **quote**\n\n- [x] done\n- [ ] pending\n\n| A | B |\n| - | - |\n| ~~old~~ | new |\n\n```rust\nlet n = 1;\n```\n";
    let document = parse(source, Profile::Publication);
    let before = document.syntax().clone();
    let mut selected = plan(&before);
    selected.links[0].destination = "https://example.test/prefix/target/#part".into();
    selected.links[1].destination = "https://example.test/prefix/assets/diagram.png".into();
    let markdown = document.render_markdown(&selected, None).unwrap();
    let html = reader_html(&markdown);
    assert_ne!(markdown, source);
    for exact in [
        "id=\"notes-heading-0\"",
        "<h2>Notes</h2>",
        "href=\"https://example.test/prefix/target/#part\"",
        "src=\"https://example.test/prefix/assets/diagram.png\"",
        "title=\"Figure\"",
        "<blockquote>",
        "<strong>quote</strong>",
        "checked=\"\"",
        "<table>",
        "<del>old</del>",
        "class=\"language-rust\"",
        "let n = 1;",
    ] {
        assert!(html.contains(exact), "{exact}: {html}");
    }
    assert!(!markdown.contains("./old"));
    assert!(!markdown.contains("old.png"));
    assert_eq!(document.syntax(), &before);
    assert_eq!(
        before.identity.source_sha256,
        Sha256Digest::of(source.as_bytes())
    );
    assert_eq!(markdown, document.render_markdown(&selected, None).unwrap());
    assert!(!markdown.contains("class=\"tone\""));
}

#[test]
fn native_markdown_keeps_symbolic_annotations_labels_and_literal_code_distinct() {
    let source = "[[site:target.test/one/|R&amp;D \\| β]] and `[[site:code.test/x/|Code]]`.\n";
    let document = parse(source, Profile::Publication);
    let mut selected = plan(document.syntax());
    selected.links[0].destination = "http://127.0.0.1:4312/prefix/one/#part".into();
    selected.links[0].annotation = Some(LinkAnnotation {
        address: "content:notes#link-1".into(),
        kind: LinkDestinationKind::CrossSite,
        content: Some("content:target".parse().unwrap()),
        site: Some("site:target".parse().unwrap()),
        site_name: Some("Target Publication".into()),
        relationships: vec![LinkRelationship::Related],
        theme: Some("theme:target".parse().unwrap()),
        palette: Some("palette:target".parse().unwrap()),
    });
    let markdown = document.render_markdown(&selected, None).unwrap();
    let html = reader_html(&markdown);
    for exact in [
        "href=\"http://127.0.0.1:4312/prefix/one/#part\"",
        ">R&amp;D | β</a> (on Target Publication)",
        "<code>[[site:code.test/x/|Code]]</code>",
    ] {
        assert!(html.contains(exact), "{exact}: {html}");
    }
    assert!(!markdown.contains("site:target.test"));
    assert!(!markdown.contains("data-gazette"));
}

#[test]
fn native_markdown_components_math_and_repeated_footnotes_share_one_admitted_closure() {
    let source = "# Subordinate notes\n\nOutside[^a] $x$.\n\n::::callout\nInside[^b] again[^a] with $x$.\n\n:::callout\nNested[^b].\n:::\n::::\n\n<!-- more -->\n\nLater[^a].\n\n```math\nx\n```\n\n[^a]: First *note* with [target](./old).\n[^b]: Second **note**.\n[^unused]: private unused body [discarded](./unused) $unused$.\n";
    let document = parse(source, Profile::PublicationMath);
    let before = document.syntax().clone();
    assert_eq!(
        before.directives.len(),
        2,
        "nested fences have distinct lengths"
    );
    let mut selected = plan(&before);
    selected.links[0].destination = "/resolved/#part".into();
    let markdown = document
        .render_markdown(&selected, Some(&Renderer))
        .unwrap();
    let html = reader_html(&markdown);
    assert_eq!(html.matches("First <em>note</em>").count(), 1);
    assert_eq!(html.matches("Second <strong>note</strong>").count(), 1);
    assert!(html.contains("Inside"));
    assert!(html.contains("Nested"));
    for exact in [
        "id=\"notes-heading-0\"",
        "id=\"fn-notes-note-0\"",
        "id=\"fn-notes-note-1\"",
        "id=\"fnref-notes-note-0-3\"",
        "id=\"fnref-notes-note-1-2\"",
        "href=\"#fnref-notes-note-1-2\"",
        "href=\"/resolved/#part\"",
        "id=\"body-continuation\"",
    ] {
        assert!(html.contains(exact), "{exact}: {html}");
    }
    assert_eq!(markdown.matches("$x$").count(), 2);
    assert!(markdown.contains("$$"), "{markdown}");
    for refused in [
        ":::callout",
        "[^a]",
        "<!-- more -->",
        "```math",
        "private unused",
        "./unused",
    ] {
        assert!(!markdown.contains(refused), "{refused}");
    }
    assert_eq!(document.syntax(), &before);
}

#[test]
fn native_markdown_refuses_the_same_forged_partial_or_unsupported_plan_and_recovers() {
    let document = parse(
        "# Notes\n\n[read](./old) again[^n].\n\n[^n]: note\n",
        Profile::Publication,
    );
    let selected = plan(document.syntax());
    let accepted = document.render_markdown(&selected, None).unwrap();
    let mut forged = selected.clone();
    forged.identity.source_sha256 = Sha256Digest::of(b"different");
    let mut partial = selected.clone();
    partial.links.clear();
    let mut duplicate = selected.clone();
    duplicate.links.push(duplicate.links[0].clone());
    let mut unsafe_link = selected.clone();
    unsafe_link.links[0].destination = "javascript:alert(1)".into();
    let mut forged_occurrence = selected.clone();
    forged_occurrence.headings[0].occurrence.level = 6;
    for bad in [forged, partial, duplicate, unsafe_link, forged_occurrence] {
        assert_eq!(
            document.render_markdown(&bad, None),
            Err(Error::InvalidPlan)
        );
        assert_eq!(document.render(&bad, None), Err(Error::InvalidPlan));
    }
    assert_eq!(document.render_markdown(&selected, None).unwrap(), accepted);
    let raw = parse(
        "<div>unadmitted authored HTML</div>\n",
        Profile::Publication,
    );
    assert!(matches!(
        raw.render_markdown(&plan(raw.syntax()), None),
        Err(Error::Unsupported { .. })
    ));
    let component = parse(":::callout\nbody\n:::\n", Profile::Publication);
    assert!(matches!(
        component.render_markdown(&plan(component.syntax()), None),
        Err(Error::Unsupported { .. })
    ));
}

#[test]
fn native_markdown_preserves_blank_code_lines_in_static_components_and_footnote_appendix() {
    let source = ":::callout\n```text\nfirst\n\n**literal**, not emphasis\n```\n:::\n\nReference[^n].\n\n[^n]: Note.\n\n    ```text\n    one\n\n    two\n    ```\n";
    let document = parse(source, Profile::Publication);
    let selected = plan(document.syntax());
    let markdown = document
        .render_markdown(&selected, Some(&Renderer))
        .unwrap();
    let actual = reader_html(&markdown);
    assert!(actual.contains("Note."));
    assert!(actual.contains("href=\"#fn-notes-note-0\""));
    let text = decoded_text(&actual);
    assert!(text.contains("first\n\n**literal**, not emphasis\n"));
    assert!(text.contains("one\n\ntwo\n"));
}

struct StaticHtml(&'static str);
impl ComponentRenderer for StaticHtml {
    fn render(&self, _: &ComponentBinding, _: &Fragment, _: bool) -> Result<String, Error> {
        Ok(self.0.into())
    }
}

#[test]
fn native_markdown_fallback_keeps_exact_foreign_subtrees_without_layout_ancestors() {
    fn foreign(facts: &[Fact]) -> Option<&Fact> {
        facts.iter().find_map(|fact| match fact {
            Fact::Element(namespace, _, _, _) if namespace != "http://www.w3.org/1999/xhtml" => {
                Some(fact)
            }
            Fact::Element(_, _, _, children) | Fact::Document(children) => foreign(children),
            _ => None,
        })
    }
    let document = parse(":::callout\nretained\n:::\n", Profile::Publication);
    let selected = plan(document.syntax());
    for html in [
        "<aside data-multiline=\"one\n\ntwo &amp; β\"><svg viewBox=\"0 0 10 10\"><linearGradient id=\"paint\"></linearGradient><foreignObject><div>HTML **literal**\n\nsecond</div></foreignObject></svg></aside>\n",
        "<aside><math xmlns=\"http://www.w3.org/1998/Math/MathML\"><mtext><span>HTML integration\n\ntext</span></mtext><annotation-xml encoding=\"text/html\"><div data-note=\"one\ntwo\">annotation content</div></annotation-xml></math></aside>\n",
    ] {
        let renderer = StaticHtml(html);
        let markdown = document
            .render_markdown(&selected, Some(&renderer))
            .unwrap();
        let actual = reader_html(&markdown);
        let expected = document
            .render(&selected, Some(&renderer))
            .unwrap()
            .body
            .html;
        let actual = tree_facts(&actual);
        let expected = tree_facts(&expected);
        let expected = foreign(&expected).expect("fixture has a meaningful foreign subtree");
        assert_eq!(foreign(&actual), Some(expected), "alternate: {markdown}");
        assert!(
            !markdown.contains("<aside"),
            "supported layout lowers: {markdown}"
        );
    }
}

#[test]
fn native_markdown_omits_presentation_programs_but_preserves_reader_text() {
    let document = parse(":::callout\nretained\n:::\n", Profile::Publication);
    let selected = plan(document.syntax());
    for html in [
        "<aside><p>Read this.</p><script type=\"application/json\">{\"first\":1,\n\n\"second\":2}</script></aside>",
        "<aside><p>Read this.</p><style>.x {\n \t\n color: red;\n}</style></aside>",
    ] {
        let renderer = StaticHtml(html);
        assert_eq!(
            document
                .render(&selected, Some(&renderer))
                .unwrap()
                .body
                .html,
            html
        );
        let markdown = document
            .render_markdown(&selected, Some(&renderer))
            .unwrap();
        assert_eq!(markdown.trim(), "Read this.");
    }
}

#[test]
fn native_markdown_bounds_output_without_returning_partial_representation() {
    let source = "bounded paragraph ".repeat(64);
    let document = Comrak
        .parse(
            &source,
            Profile::Publication,
            Limits {
                output_bytes: 128,
                ..Limits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        document.render_markdown(&plan(document.syntax()), None),
        Err(Error::Limit(_))
    ));
    let document = parse("$x$\n", Profile::PublicationMath);
    let mut selected = plan(document.syntax());
    selected.math[0].mathml = selected.math[0]
        .mathml
        .replace("<mi>x</mi>", "<script>run()</script>");
    assert_eq!(
        document.render_markdown(&selected, None),
        Err(Error::InvalidPlan)
    );
}

#[test]
fn provider_static_transport_keeps_ordered_notebook_code_json_links_and_anchor_tree() {
    let containing_source = "retained notebook source";
    let location = Location {
        span: Span {
            start: 0,
            end: containing_source.len(),
        },
        precision: Precision::Construct,
    };
    let html = "<section class=\"gazette-notebook\"><div class=\"notebook-markdown\" data-cell-id=\"intro\"><h2 id=\"notebook-cell-0-title\">Retained title</h2><p><a href=\"https://example.test/target/#part\" data-gazette-link=\"content:host#link-2\">resolved label</a></p></div><div class=\"notebook-code\" data-cell-id=\"code\" data-execution-count=\"3\"><pre class=\"notebook-source\"><code>first\n\n**literal**, not emphasis\n</code></pre><pre class=\"notebook-json\">{\n\n  &quot;value&quot;: 42\n}</pre></div><div class=\"notebook-code\" data-cell-id=\"stream\"><pre class=\"notebook-stream\">line one\n\nline two</pre></div></section>\n";
    let transport = Comrak
        .transport_static_html(html, location, Limits::default())
        .unwrap();
    assert_ne!(transport, html);
    assert!(transport.contains("&#10;&#10;**literal**, not emphasis"));
    assert_eq!(tree_facts(&transport), tree_facts(html));
    let read = reader_html(&transport);
    assert_eq!(tree_facts(&read), tree_facts(html));
    let text = decoded_text(&read);
    assert!(text.contains("first\n\n**literal**, not emphasis\n"));
    assert!(text.contains("{\n\n  \"value\": 42\n}"));
    assert!(text.contains("line one\n\nline two"));
    assert!(read.contains("id=\"notebook-cell-0-title\""));
    assert!(read.contains("data-execution-count=\"3\""));
    assert!(read.contains("data-gazette-link=\"content:host#link-2\""));
    let authored = parse(html, Profile::Publication);
    assert!(
        matches!(
            authored.render_markdown(&plan(authored.syntax()), None),
            Err(Error::Unsupported { .. })
        ),
        "the transport capability does not admit authored raw HTML"
    );
}

#[test]
fn provider_static_transport_has_exact_source_diagnostics_and_independent_resource_bounds() {
    let location = Location {
        span: Span { start: 20, end: 90 },
        precision: Precision::Construct,
    };
    let malformed =
        Comrak.transport_static_html("<div attr=\"unterminated", location, Limits::default());
    assert!(
        matches!(malformed, Err(Error::Unsupported { location: bound, reason: "selected HTML does not have an unambiguous native fragment tree" }) if bound == location)
    );
    let unsafe_literal = Comrak.transport_static_html(
        "<section><script type=\"application/json\">{\n\n}</script></section>",
        location,
        Limits::default(),
    );
    assert!(
        matches!(unsafe_literal, Err(Error::Unsupported { location: bound, reason: "selected HTML literal blank lines cannot be preserved in Markdown transport" }) if bound == location)
    );
    assert_eq!(
        Comrak.transport_static_html(
            "<section>too long</section>",
            location,
            Limits {
                output_bytes: 8,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("HTML transport input bytes"))
    );
    assert_eq!(
        Comrak.transport_static_html(
            "<section><div><span>deep</span></div></section>",
            location,
            Limits {
                depth: 2,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("HTML transport depth"))
    );
    assert_eq!(
        Comrak.transport_static_html(
            "<section><div>one</div><div>two</div></section>",
            location,
            Limits {
                nodes: 5,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("HTML transport nodes"))
    );
    let expanded = format!(
        "<section><pre><code>{}</code></pre></section>",
        "x\n".repeat(120)
    );
    assert!(expanded.len() < 512);
    assert_eq!(
        Comrak.transport_static_html(
            &expanded,
            location,
            Limits {
                output_bytes: 512,
                ..Limits::default()
            }
        ),
        Err(Error::Limit("rendered output bytes"))
    );
    assert!(
        Comrak
            .transport_static_html(
                "<section>still healthy</section>",
                location,
                Limits::default()
            )
            .is_ok()
    );
}

#[test]
fn standalone_image_boundaries_preserve_links_mixed_prose_and_tight_lists() {
    for source in [
        "![Diagram](figure.svg)\n",
        "[![Diagram](figure.svg)](https://example.test/large/)\n",
        "- ![Diagram](figure.svg)\n",
    ] {
        let document = parse(source, Profile::Publication);
        let selected = plan(document.syntax());
        let html = document.render(&selected, None).unwrap().body.html;
        assert_eq!(
            html.matches("data-gazette-image-block").count(),
            1,
            "{html}"
        );
        assert!(html.contains("src=\"figure.svg\""), "{html}");
        if source.starts_with("[!") {
            assert!(
                html.contains("href=\"https://example.test/large/\""),
                "{html}"
            );
        }
        if source.starts_with("- ") {
            assert!(
                !html.contains("<p>"),
                "tight-list formatting remains native: {html}"
            );
        }
        let markdown = document.render_markdown(&selected, None).unwrap();
        assert!(!markdown.contains("data-gazette-image-block"));
        assert!(markdown.contains("![Diagram](figure.svg)"));
    }
    for source in [
        "Before ![Diagram](figure.svg) after.\n",
        "![One](one.svg) ![Two](two.svg)\n",
        "[Read ![Diagram](figure.svg)](https://example.test/)\n",
    ] {
        let document = parse(source, Profile::Publication);
        let selected = plan(document.syntax());
        let html = document.render(&selected, None).unwrap().body.html;
        assert!(!html.contains("data-gazette-image-block"), "{html}");
        assert!(html.contains("<img"));
    }
}

#[test]
fn native_markdown_lowers_a_mode_picture_to_its_default_image() {
    let document = parse(":::callout\nretained\n:::\n", Profile::Publication);
    let selected = plan(document.syntax());
    for html in [
        "<aside><p>Before <picture data-gazette-image-modes><source data-gazette-image-mode=\"dark\" media=\"(prefers-color-scheme: dark)\" srcset=\"/dark.webp\"><img data-gazette-image=\"image:one\" src=\"/light.webp\" alt=\"Figure\" /></picture> after</p></aside>",
        "<aside><picture data-gazette-image-modes><source data-gazette-image-mode=\"dark\" media=\"all\" srcset=\"/dark.webp\"><img src=\"/light.webp\" alt=\"Figure\" /></picture></aside>",
    ] {
        let renderer = StaticHtml(html);
        let markdown = document
            .render_markdown(&selected, Some(&renderer))
            .unwrap();
        assert!(markdown.contains("![Figure](/light.webp)"), "{markdown}");
        assert!(!markdown.contains("dark.webp"), "{markdown}");
        assert!(!markdown.contains('<'), "{markdown}");
    }
}
