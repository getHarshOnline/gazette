//! Native directive parsing and complete static-component formatting; no Theme selection here.

#[path = "components/container_contents.rs"]
mod container_contents;

use std::{collections::BTreeMap, sync::Mutex};

use gazette_adapter_markdown::Comrak;
use gazette_core::{
    markdown::{
        ComponentBinding, Error, Excerpt, FootnoteBinding, Fragment, HeadingBinding, Limits,
        LinkBinding, Plan, Profile, Syntax,
    },
    source::{Precision, Sha256Digest},
};
use gazette_ports::markdown::{ComponentRenderer, Document, Markdown};
use serde_json::json;

#[derive(Default)]
struct Renderer {
    calls: Mutex<Vec<(String, String, bool)>>,
}

impl ComponentRenderer for Renderer {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        assert_eq!(binding.name.as_str(), "callout");
        assert_eq!(binding.arguments["tone"], "note");
        self.calls.lock().expect("fixture calls").push((
            body.html.clone(),
            body.text.clone(),
            excerpt,
        ));
        Ok(format!("<aside class=\"note\">{}</aside>\n", body.html))
    }
}

fn parse(source: &str) -> Box<dyn Document> {
    Comrak
        .parse(source, Profile::Publication, Limits::default())
        .expect("retained native parse")
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
                destination: "/resolved".into(),
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
        components: syntax
            .directives
            .iter()
            .filter(|d| d.emitted)
            .map(|d| ComponentBinding {
                container_contents: None,
                reading_prefix: None,
                presentation_only: false,
                occurrence: d.clone(),
                name: "callout".parse().expect("local component name"),
                arguments: BTreeMap::from([("tone".into(), json!("note"))]),
                context: None,
            })
            .collect(),
        math: vec![],
        excerpt: None,
    }
}

#[test]
fn exact_header_bytes_survive_native_unescaping_crlf_unicode_and_containers() {
    let source = "😀 before\r\n\r\n> :::callout {title = \"a&amp;b\", path = \"\\\\native\"}  \r\n> 文 **body**\r\n> :::\r\n";
    let document = parse(source);
    let syntax = document.syntax();
    assert_eq!(syntax.directives.len(), 1);
    let directive = &syntax.directives[0];
    assert_eq!(
        directive.info,
        "callout {title = \"a&amp;b\", path = \"\\\\native\"}  "
    );
    assert_eq!(
        directive.header.span.slice(source),
        Some(directive.info.as_str())
    );
    assert_eq!(directive.header.precision, Precision::Exact);
    assert_eq!(directive.location.precision, Precision::Construct);
    assert!(
        directive
            .location
            .span
            .slice(source)
            .unwrap()
            .starts_with(":::callout")
    );
    let rendered = document
        .render(&plan(syntax), Some(&Renderer::default()))
        .expect("native retained header binding");
    assert!(rendered.body.html.contains("<strong>body</strong>"));
    assert!(rendered.body.text.contains("文 body"));
    assert!(!rendered.body.text.contains("callout"));
}

#[test]
fn nested_components_share_resolved_links_headings_and_native_global_footnotes() {
    let source = "::::callout {tone=\"note\"}\n# Heading\n\nOuter [link](target.md)[^n].\n\n:::callout {tone=\"note\"}\nNested[^n].\n:::\n::::\n\nAfter[^n].\n\n[^n]: Shared definition\n";
    let document = parse(source);
    let selected = plan(document.syntax());
    let renderer = Renderer::default();
    let output = document
        .render(&selected, Some(&renderer))
        .expect("native nested formatting");
    assert_eq!(
        output.body.html.matches("<aside class=\"note\">").count(),
        2
    );
    assert_eq!(output.body.html.matches("</aside>").count(), 2);
    assert!(output.body.html.contains("id=\"heading-0\""));
    assert!(output.body.html.contains("href=\"/resolved\""));
    assert_eq!(output.body.html.matches("id=\"fn-note-0\"").count(), 1);
    for anchor in ["fnref-note-0", "fnref-note-0-2", "fnref-note-0-3"] {
        assert_eq!(
            output
                .body
                .html
                .matches(&format!("id=\"{anchor}\""))
                .count(),
            1
        );
    }
    assert_eq!(output.body.text.matches("Shared definition").count(), 1);
    assert_eq!(renderer.calls.lock().unwrap().len(), 2);
    assert_eq!(
        output,
        document
            .render(&selected, Some(&Renderer::default()))
            .expect("repeat retained formatting")
    );
}

#[test]
fn excerpt_inside_nested_component_uses_clipped_native_children_not_full_body_replacement() {
    let source = "::::callout\n# First\n\n:::callout\nBefore **bold**[^n].\n\n<!-- more -->\n\nHidden tail\n:::\n\nHidden outer\n::::\n\n[^n]: Shared note\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    selected.excerpt = Some(Excerpt {
        marker: document.syntax().html[0].clone(),
        continuation: "continue-reading".into(),
    });
    let renderer = Renderer::default();
    let output = document
        .render(&selected, Some(&renderer))
        .expect("balanced component excerpt");
    assert!(output.body.html.contains("Hidden tail"));
    let excerpt = output.excerpt.expect("explicit excerpt");
    assert_eq!(excerpt.html.matches("<aside class=\"note\">").count(), 2);
    assert_eq!(excerpt.html.matches("</aside>").count(), 2);
    assert!(excerpt.html.contains("<h1>First</h1>"));
    assert!(excerpt.text.contains("Before bold"));
    assert!(!excerpt.html.contains("Hidden"));
    assert!(!excerpt.text.contains("Hidden"));
    assert!(!excerpt.html.contains("id="));
    assert!(!excerpt.text.contains("Shared note"));
    assert_eq!(
        renderer
            .calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.2)
            .count(),
        2
    );
}

#[test]
fn fenced_and_inline_examples_are_literal_and_unused_definitions_do_not_demand_a_renderer() {
    let document = parse("```md\n:::callout {tone=\"note\"}\nbody\n:::\n```\n\n`:::callout`\n");
    assert!(document.syntax().directives.is_empty());
    let output = document
        .render(&plan(document.syntax()), None)
        .expect("literal examples");
    assert!(output.body.text.contains(":::callout"));
    assert!(!output.body.html.contains("<aside"));
    let document = parse("Visible\n\n[^unused]:\n    :::callout\n    Hidden\n    :::\n");
    assert_eq!(document.syntax().directives.len(), 1);
    assert!(document.syntax().directives.iter().all(|d| !d.emitted));
    let output = document
        .render(&plan(document.syntax()), None)
        .expect("unused definition omitted");
    assert!(!output.body.text.contains("Hidden"));
}

#[test]
fn exact_complete_binding_and_renderer_are_required_without_poisoning_reuse() {
    let document = parse(":::callout {tone=\"note\"}\nBody\n:::\n");
    let valid = plan(document.syntax());
    assert!(matches!(
        document.render(&valid, None),
        Err(Error::Unsupported { .. })
    ));
    let mut cases = Vec::new();
    let mut value = valid.clone();
    value.components.clear();
    cases.push(value);
    let mut value = valid.clone();
    value.components.push(value.components[0].clone());
    cases.push(value);
    let mut value = valid.clone();
    value.components[0].occurrence.info.push(' ');
    cases.push(value);
    let mut value = valid.clone();
    value.identity.source_sha256 = Sha256Digest::of(b"different");
    cases.push(value);
    for value in cases {
        assert_eq!(
            document.render(&value, Some(&Renderer::default())),
            Err(Error::InvalidPlan)
        );
    }
    assert!(document.render(&valid, Some(&Renderer::default())).is_ok());
}

#[test]
fn callback_errors_and_oversized_output_are_not_partial_success() {
    struct Refused;
    impl ComponentRenderer for Refused {
        fn render(
            &self,
            binding: &ComponentBinding,
            _: &Fragment,
            _: bool,
        ) -> Result<String, Error> {
            Err(Error::Unsupported {
                location: binding.occurrence.header,
                reason: "fixture selected definition refused",
            })
        }
    }
    struct Oversized;
    impl ComponentRenderer for Oversized {
        fn render(&self, _: &ComponentBinding, _: &Fragment, _: bool) -> Result<String, Error> {
            Ok("x".repeat(1025))
        }
    }
    let document = Comrak
        .parse(
            ":::callout\nBody\n:::\n",
            Profile::Publication,
            Limits {
                output_bytes: 1024,
                ..Default::default()
            },
        )
        .unwrap();
    let selected = plan(document.syntax());
    assert_eq!(
        document.render(&selected, Some(&Refused)),
        Err(Error::Unsupported {
            location: selected.components[0].occurrence.header,
            reason: "fixture selected definition refused"
        })
    );
    assert_eq!(
        document.render(&selected, Some(&Oversized)),
        Err(Error::Limit("component output bytes"))
    );
}

#[test]
fn recursive_work_is_bounded_and_link_rewriting_never_decodes_component_arguments() {
    let source = "::::callout {title=\"a&amp;b\"}\n:::callout\n[body](old.md)\n:::\n::::\n";
    let analyzed = parse(source);
    let document = Comrak
        .parse(
            source,
            Profile::Publication,
            Limits {
                nodes: analyzed.syntax().node_count,
                ..Default::default()
            },
        )
        .expect("source itself fits");
    assert_eq!(
        document.render(&plan(document.syntax()), Some(&Renderer::default())),
        Err(Error::Limit("component rendering work"))
    );
    let unchanged: Vec<_> = analyzed
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
            image_address: None,
            occurrence: link.clone(),
            destination: link.destination.clone(),
            annotation: None,
        })
        .collect();
    assert_eq!(
        analyzed
            .rewrite_links(analyzed.syntax().identity, &unchanged)
            .unwrap(),
        source
    );
    let mut changed = unchanged;
    changed[0].destination = "/changed".into();
    assert!(matches!(
        analyzed.rewrite_links(analyzed.syntax().identity, &changed),
        Err(Error::Unsupported {
            reason: "native Markdown link formatting cannot preserve original component headers",
            ..
        })
    ));
}

#[test]
fn presentation_payload_keeps_html_and_resolution_but_leaves_both_reading_outputs() {
    let source = ":::::callout\nFilm **description**.\n\n::::callout\n[Frame 1](frame.webp)\n\n:::callout\nNested payload.\n:::\n::::\n\nMeaningful caption.\n\n![Accessible still](still.webp)\n:::::\n\nFrame 1 is an ordinary article title.\n";
    let document = parse(source);
    let syntax = document.syntax().clone();
    let mut selected = plan(&syntax);
    assert_eq!(selected.components.len(), 3);
    let original = document
        .render(&selected, Some(&Renderer::default()))
        .unwrap();
    selected.components[1].presentation_only = true;
    let rendered = document
        .render(&selected, Some(&Renderer::default()))
        .unwrap();
    assert_eq!(rendered.body.html, original.body.html);
    assert!(rendered.body.html.contains("href=\"/resolved\""));
    assert!(rendered.body.html.contains("Nested payload."));
    assert!(!rendered.body.text.contains("Nested payload."));
    assert_eq!(rendered.body.text.matches("Frame 1").count(), 1);
    let markdown = document
        .render_markdown(&selected, Some(&Renderer::default()))
        .unwrap();
    assert!(!markdown.contains("Nested payload."));
    assert_eq!(markdown.matches("Frame 1").count(), 1);
    for meaningful in ["description", "Meaningful caption.", "Accessible still"] {
        assert_eq!(rendered.body.text.matches(meaningful).count(), 1);
        assert_eq!(markdown.matches(meaningful).count(), 1);
    }
    let mut incomplete = selected.clone();
    incomplete.links.remove(0);
    assert_eq!(
        document.render_markdown(&incomplete, Some(&Renderer::default())),
        Err(Error::InvalidPlan)
    );
    assert_eq!(document.syntax(), &syntax);
    assert_eq!(
        document
            .render(&selected, Some(&Renderer::default()))
            .unwrap(),
        rendered
    );
}

#[test]
fn presentation_payload_does_not_keep_its_private_notes_or_duplicate_shared_backlinks() {
    let source = ":::callout\nPayload[^only] and shared[^both].\n:::\n\nDescription[^both].\n\n[^only]: Payload-only note.\n\n[^both]: Readable shared note.\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    let original = document
        .render(&selected, Some(&Renderer::default()))
        .unwrap();
    selected.components[0].presentation_only = true;
    let rendered = document
        .render(&selected, Some(&Renderer::default()))
        .unwrap();
    assert_eq!(rendered.body.html, original.body.html);
    assert!(!rendered.body.text.contains("Payload"));
    assert_eq!(
        rendered.body.text.matches("Readable shared note.").count(),
        1
    );
    let markdown = document
        .render_markdown(&selected, Some(&Renderer::default()))
        .unwrap();
    assert!(!markdown.contains("Payload"));
    assert!(!markdown.contains("fnref-note-1-2"));
    assert_eq!(markdown.matches("Readable shared note.").count(), 1);
    assert!(markdown.contains("(#fnref-note-1)"));
}

#[test]
fn presentation_filter_preserves_text_consumed_by_html_templates_and_ordinary_parents() {
    #[derive(Default)]
    struct TextRenderer {
        calls: Mutex<Vec<(bool, bool, String)>>,
    }
    impl ComponentRenderer for TextRenderer {
        fn render(
            &self,
            binding: &ComponentBinding,
            body: &Fragment,
            excerpt: bool,
        ) -> Result<String, Error> {
            self.calls.lock().unwrap().push((
                binding.presentation_only,
                excerpt,
                body.text.clone(),
            ));
            // This fixture contains plain text only; exercise both supported body fields.
            Ok(format!(
                "<section><header>{}</header>{}</section>",
                body.text, body.html
            ))
        }
    }
    let source = "::::callout\nFilm description.\n\n:::callout\nFrame payload.\n:::\n\nCaption.\n\n<!-- more -->\n\nClosing account.\n::::\n";
    let document = parse(source);
    let mut selected = plan(document.syntax());
    selected.excerpt = Some(Excerpt {
        marker: document.syntax().html[0].clone(),
        continuation: "continue-reading".into(),
    });
    let original_renderer = TextRenderer::default();
    let original = document
        .render(&selected, Some(&original_renderer))
        .unwrap();
    selected.components[1].presentation_only = true;
    let renderer = TextRenderer::default();
    let rendered = document.render(&selected, Some(&renderer)).unwrap();
    assert_eq!(rendered.body.html, original.body.html);
    assert_eq!(
        rendered.excerpt.as_ref().unwrap().html,
        original.excerpt.as_ref().unwrap().html
    );
    let calls = renderer.calls.lock().unwrap();
    assert_eq!(calls.len(), 4);
    for ((_, excerpt, text), (_, original_excerpt, original_text)) in calls
        .iter()
        .zip(original_renderer.calls.lock().unwrap().iter())
    {
        assert_eq!(excerpt, original_excerpt);
        assert_eq!(text, original_text);
        assert!(text.contains("Frame payload."));
    }
    assert!(
        calls
            .iter()
            .filter(|(payload, _, _)| !payload)
            .all(|(_, _, text)| text.contains("Film description."))
    );
    for text in [
        &rendered.body.text,
        &rendered.excerpt.as_ref().unwrap().text,
    ] {
        assert!(!text.contains("Frame payload."));
        assert_eq!(text.matches("Caption.").count(), 1);
    }
    let markdown = document
        .render_markdown(&selected, Some(&TextRenderer::default()))
        .unwrap();
    assert!(!markdown.contains("Frame payload."));
    assert!(markdown.contains("Film description."));
    assert!(markdown.contains("Closing account."));
}
