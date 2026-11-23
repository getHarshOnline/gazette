//! Figure guards preserve the admitted inline SVG profile through real projection.

use super::*;
use gazette_compiler::{
    markdown::{ComponentPresentation, validate_figure_html},
    theme::ComponentSemantic,
    theme_package::ParsedManifest,
};
use gazette_core::markdown::{ComponentBinding, Error, Fragment};
use gazette_ports::markdown::ComponentRenderer;

const THEME: &str = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.figure]\ntemplate='figure.html'\nsemantic={kind='figure'}\n[components.figure-caption]\ntemplate='caption.html'\nsemantic={kind='figure-caption'}\n";
// Width, labelled text and nested graphics are admitted by native InlineSvg,
// while generic Theme HTML intentionally supports a smaller inert SVG profile.
const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="420" height="910" viewBox="0 0 420 910" role="img" aria-labelledby="title desc"><title id="title">Shared interface</title><desc id="desc">Different model state.</desc><g fill="var(--color-ink, #252c35)" font-family="var(--type-family-reading, sans-serif)"><text x="105" y="32">Caller &amp; model</text><rect x="10" y="50" width="100" height="40" fill="none" stroke="currentColor"/></g></svg>"##;

struct Renderer;
impl ComponentRenderer for Renderer {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        let (tag, semantic) = match binding.name.as_str() {
            "figure" => ("figure", ComponentSemantic::Figure),
            "figure-caption" => ("figcaption", ComponentSemantic::FigureCaption),
            _ => return Err(Error::InvalidPlan),
        };
        let rendered = format!("<{tag}>{}</{tag}>", body.html);
        if !excerpt {
            if matches!(semantic, ComponentSemantic::Figure) {
                assert!(body.html.contains("data-gazette-image-ref="));
                assert!(
                    matches!(
                        gazette_compiler::html::inspect(
                            body.html.as_bytes(),
                            Default::default(),
                            |_, _| Ok::<_, ()>(())
                        ),
                        Err(gazette_compiler::html::Error::Unsupported { .. })
                    ),
                    "fixture must reproduce the unrelated HTML SVG profile rejection"
                );
                for changed in [
                    rendered.replace("Caller &amp; model", "Invented model"),
                    rendered.replace("<figure>", "<figure onclick='run()'>"),
                    rendered.replace(
                        "<figure>",
                        "<figure style='background:url(https://example.test/tracker)'>",
                    ),
                    rendered.replace("</figure>", "<script>run()</script></figure>"),
                    rendered.replace("figure.svg", "other.svg"),
                ] {
                    assert!(
                        validate_figure_html(
                            binding,
                            &semantic,
                            body,
                            &changed,
                            Default::default()
                        )
                        .is_err()
                    );
                }
                assert!(
                    validate_figure_html(
                        binding,
                        &semantic,
                        body,
                        &rendered,
                        gazette_compiler::html::Limits {
                            input_bytes: rendered.len() - 1,
                            ..Default::default()
                        }
                    )
                    .is_err()
                );
            }
            validate_figure_html(binding, &semantic, body, &rendered, Default::default())?;
        }
        Ok(rendered)
    }
}

#[test]
fn native_inline_svg_keeps_caption_exact_body_and_scoped_accessibility() {
    let source = format!(
        "{}\n::::figure\n![Original alt](figure.svg \"Original title\")\n\n:::figure-caption\nThe **same** interface keeps different model state.\n:::\n::::\n",
        page_with(
            "content:guide",
            "/guide/",
            "[[images]]\ndefault='figure.svg'\ninline=true\n"
        )
    );
    let files = vec![
        ("authored/one/guide/index.md".into(), source.into_bytes()),
        (
            "authored/one/guide/figure.svg".into(),
            SVG.as_bytes().to_vec(),
        ),
    ];
    let corpus = parse(&files);
    let compilation = compilation();
    let resolved = resolve(&compilation, &corpus);
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    let page = resolved
        .project_page_with(
            &key("content:guide"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
        )
        .unwrap();
    let image = &page.data.images[0];
    let address = image.address.as_ref().unwrap();
    let scoped = format!("gazette-svg-{}-title", Sha256Digest::of(address.as_bytes()));
    assert!(
        page.body_html
            .contains(&format!("aria-labelledby=\"{scoped} "))
    );
    assert!(page.body_html.contains("Caller &amp; model"));
    assert!(
        page.body_html
            .contains("<figcaption><p>The <strong>same</strong> interface")
    );
    assert_eq!(
        page.body_text
            .matches("The same interface keeps different model state.")
            .count(),
        1
    );
    let json = serde_json::to_value(page.json_ld()).unwrap();
    assert_eq!(json["image"][0]["description"], "Original alt");
    assert_eq!(json["image"][0]["name"], "Original title");
    assert_eq!(
        json["image"][0]["caption"].as_str().unwrap().trim(),
        "The same interface keeps different model state."
    );
    assert_eq!(
        json["image"][0]["contentUrl"],
        "https://example.test/guide/figure.svg"
    );

    let mut active = files;
    active[1].1 = SVG.replace("<text ", "<text onclick='run()' ").into_bytes();
    let corpus = parse(&active);
    assert!(
        authored::resolve(&compilation, &corpus, Default::default()).is_err(),
        "caption guards must not loosen native SVG admission"
    );
}
