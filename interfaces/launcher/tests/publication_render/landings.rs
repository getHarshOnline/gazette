//! Structured YAML enters the native captured-site build, not a pre-build converter.
use super::*;

const LANDING: &str = r#"format: gazette-landing/v1
collections: ['collection:home']
related: ['content:other']
content:
  id: content:landing
  site: site:one
  title: Landing 🦀
  description: Ordered content with shared references.
  lifecycle: published
  route: {path: /landing/}
  structure: {kind: page, family: {kind: utility}}
sections:
  - component: callout
    arguments: {tone: note}
    heading: {level: 2, text: Repeat}
    body: 'First passage with a note.[^one] [[site:other.example/elsewhere/#other-body|Other]]'
    sections:
      - component: callout
        arguments: {tone: warning}
        heading: {level: 2, text: Repeat}
        body: 'Second passage 🦀 and [evidence][proof]. ![Retained](image.png) [Download](evidence.pdf)'
  - component: callout
    arguments: {tone: note}
  - body: |
      [proof]: https://example.invalid/evidence
      [^one]: A note defined in the closing section.
"#;

#[test]
fn captured_yaml_sections_render_with_native_routes_components_and_machine_views() {
    let manifest = components::MANIFEST.replace(
        "render={page='page.html'}",
        "render={page='page.html',alias='alias.html'}",
    );
    let fixture = Fixture::new(&[
        ("one/landing/index.landing.yaml", LANDING.as_bytes()),
        (
            "one/landing/image.png",
            b"\x89PNG\r\n\x1a\n\0transport fixture",
        ),
        ("one/landing/evidence.pdf", b"%PDF-1.7\ntransport fixture"),
        ("theme/theme.toml", manifest.as_bytes()),
        (
            "theme/templates/components/callout.html",
            components::COMPONENT.as_bytes(),
        ),
    ]);
    let config = fixture
        .config()
        .replace(
            "authored=[{source='source:one',target='one'}]",
            "authored=[{source='source:one',target='one',markdown_profile='publication-math',static_assets={route_prefix='/media/'}}]",
        )
        .replacen(
            "public_base_url=",
            &format!(
                "machine={{search='lexical',sitemap=true,{}}}\npublic_base_url=",
                curated_llms("content:landing", &[("Landing", &["content:landing"])])
            ),
            1,
        );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("native YAML build");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let page = &model["pages"]["content:landing"];
    assert_eq!(page["components"].as_array().unwrap().len(), 3);
    let html = std::str::from_utf8(bytes(tree, "landing/index.html")).unwrap();
    for text in [
        "callout note",
        "callout warning",
        "id=\"repeat\"",
        "id=\"repeat-1\"",
        "https://other.example/elsewhere/#other-body",
        "A note defined in the closing section.",
    ] {
        assert!(html.contains(text), "{text}: {html}");
    }
    assert!(
        html.contains("https://one.example/mount/landing/image.png"),
        "{html}"
    );
    assert!(html.contains("https://one.example/mount/landing/evidence.pdf"));
    assert_eq!(
        bytes(tree, "landing/evidence.pdf"),
        b"%PDF-1.7\ntransport fixture"
    );
    let alternate = std::str::from_utf8(bytes(tree, "landing/index.md")).unwrap();
    assert!(alternate.contains("Second passage 🦀"));
    assert!(alternate.contains("https://other.example/elsewhere/#other-body"));
    assert!(!alternate.contains("format: gazette-landing"));
    let search = std::str::from_utf8(bytes(tree, "search.json")).unwrap();
    assert!(search.contains("Second passage"));
    assert!(
        std::str::from_utf8(bytes(tree, "sitemap.xml"))
            .unwrap()
            .contains("https://one.example/mount/landing/")
    );
}
