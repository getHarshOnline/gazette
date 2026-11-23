//! Generated navigation has one admitted position and never becomes authored reading text.

use super::{parse, plan};
use gazette_core::markdown::{
    ComponentBinding, ContainerContentsBinding, ContainerContentsKind, ContainerContentsView,
    Error, Fragment,
};
use gazette_ports::markdown::ComponentRenderer;
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct Renderer(AtomicUsize);
impl ComponentRenderer for Renderer {
    fn render(&self, _: &ComponentBinding, body: &Fragment, _: bool) -> Result<String, Error> {
        assert!(body.html.is_empty());
        assert!(body.text.is_empty());
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok("<nav>Generated child</nav>".into())
    }
}

fn payload(markdown: &str) -> ContainerContentsBinding {
    ContainerContentsBinding {
        presentation: ContainerContentsView {
            kind: ContainerContentsKind::Collection,
            documentation: false,
            entries: vec![],
            outline: None,
        },
        reading_markdown: Some(markdown.into()),
    }
}

#[test]
fn slot_uses_native_markdown_without_template_text_or_html_transport() {
    let document = parse("Before.\n\n:::place\n:::\n\nAfter.\n");
    let mut selected = plan(document.syntax());
    selected.components[0].arguments.clear();
    selected.components[0].container_contents = Some(payload(
        "## Contents\n\n- [Exact <child>](https://example.test/child/)\n",
    ));
    // The fragment is compiler-produced Markdown; an unescaped HTML-shaped label
    // must still be refused instead of opening an arbitrary raw-HTML path.
    let renderer = Renderer::default();
    assert!(
        document
            .render_markdown(&selected, Some(&renderer))
            .is_err()
    );
    selected.components[0].container_contents = Some(payload(
        "## Contents\n\n- [Exact \\<child\\>](https://example.test/child/)\n",
    ));
    let html = document.render(&selected, Some(&renderer)).unwrap();
    let baseline = parse("Before.\n\nAfter.\n");
    let baseline = baseline.render(&plan(baseline.syntax()), None).unwrap();
    assert_eq!(html.body.text, baseline.body.text);
    assert!(
        html.body.html.find("Before").unwrap() < html.body.html.find("Generated child").unwrap()
    );
    assert!(
        html.body.html.find("Generated child").unwrap() < html.body.html.find("After").unwrap()
    );
    let markdown = document
        .render_markdown(&selected, Some(&renderer))
        .unwrap();
    assert!(markdown.find("Before").unwrap() < markdown.find("## Contents").unwrap());
    assert!(markdown.find("## Contents").unwrap() < markdown.find("After").unwrap());
    assert!(markdown.contains("https://example.test/child/"));
    assert!(!markdown.contains("Generated child"));
    assert_eq!(renderer.0.load(Ordering::Relaxed), 1);
}

#[test]
fn slot_refuses_nonempty_nested_duplicate_and_raw_html_payloads() {
    for source in [
        ":::place\nBody.\n:::\n",
        "> :::place\n> :::\n",
        "- :::place\n  :::\n",
        "::::outer\n:::place\n:::\n::::\n",
        ":::place\n:::\n\n:::place\n:::\n",
    ] {
        let document = parse(source);
        let mut selected = plan(document.syntax());
        for binding in &mut selected.components {
            binding.arguments.clear();
            binding.container_contents = Some(payload("- [Child](https://example.test/child/)\n"));
        }
        assert!(
            document
                .render(&selected, Some(&Renderer::default()))
                .is_err(),
            "{source}"
        );
    }
    for markdown in [
        "<script>alert(1)</script>",
        "![image](https://example.test/image.png)",
        ":::invented\nBody\n:::\n",
    ] {
        let document = parse(":::place\n:::\n");
        let mut selected = plan(document.syntax());
        selected.components[0].arguments.clear();
        selected.components[0].container_contents = Some(payload(markdown));
        assert!(
            document
                .render_markdown(&selected, Some(&Renderer::default()))
                .is_err(),
            "{markdown}"
        );
    }
}

#[test]
fn generated_navigation_is_absent_from_excerpts_and_capped_before_reading() {
    let document = parse("Before.\n\n:::place\n:::\n\n<!-- more -->\n\nAfter.\n");
    let mut selected = plan(document.syntax());
    selected.components[0].arguments.clear();
    selected.components[0].container_contents = Some(payload("- [Child](https://example.test/)\n"));
    selected.excerpt = Some(gazette_core::markdown::Excerpt {
        marker: document.syntax().html[0].clone(),
        continuation: "continue".into(),
    });
    let rendered = document
        .render(&selected, Some(&Renderer::default()))
        .unwrap();
    let excerpt = rendered.excerpt.unwrap();
    assert!(excerpt.text.contains("Before."));
    assert!(!excerpt.html.contains("Generated child"));
    selected.components[0].container_contents = Some(payload(
        &"x".repeat(gazette_core::markdown::Limits::default().output_bytes + 1),
    ));
    assert!(
        document
            .render_markdown(&selected, Some(&Renderer::default()))
            .is_err()
    );
}
