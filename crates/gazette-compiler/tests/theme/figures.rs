use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    markdown::{ComponentPresentation, Limits, ParsedMarkdown, validate_figure_html},
    theme::ComponentSemantic,
    theme_package::ParsedManifest,
    workspace,
};
use gazette_core::markdown::{ComponentBinding, Error, Fragment};
use gazette_ports::markdown::ComponentRenderer;

const THEME: &str = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.figure]\ntemplate='figure.html'\nsemantic={kind='figure'}\n[components.figure-caption]\ntemplate='caption.html'\nsemantic={kind='figure-caption'}\n[components.decoration]\ntemplate='decoration.html'\npresentation_only=true\n[components.questions]\ntemplate='faq.html'\nsemantic={kind='faq'}\n[components.answer]\ntemplate='answer.html'\nsemantic={kind='faq-item',question_argument='question'}\narguments={type='object',required=['question'],properties={question={type='string'}}}\n";
const FIGURE: &str = "::::figure\n[![Diagram alt](https://example.test/image.png \"Image title\")](https://example.test/source)\n\n:::figure-caption\nAn **authored** caption with [evidence](https://example.test/evidence).\n:::\n::::\n";

struct Renderer;
impl ComponentRenderer for Renderer {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        excerpt: bool,
    ) -> Result<String, Error> {
        let (tag, semantic) = match binding.name.as_str() {
            "figure" => ("figure", Some(ComponentSemantic::Figure)),
            "figure-caption" => ("figcaption", Some(ComponentSemantic::FigureCaption)),
            _ => ("section", None),
        };
        let output = format!("<{tag}>{}</{tag}>", body.html);
        if !excerpt && let Some(semantic) = semantic {
            validate_figure_html(binding, &semantic, body, &output, Default::default())?;
        }
        Ok(output)
    }
}

fn render(
    source: &str,
) -> Result<gazette_compiler::markdown::RenderedMarkdown, gazette_compiler::markdown::Error> {
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)?
        .render_with_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
            |_, _, _| Ok(None),
        )
}

#[test]
fn declared_caption_binds_only_its_image_and_preserves_link_alt_title_and_reading_text() {
    let source = format!(
        "Mixed ![inline](https://example.test/inline.png) prose.\n\n{FIGURE}\n![ordinary](https://example.test/ordinary.png)\n\nUnbound following paragraph.\n"
    );
    let result = render(&source).unwrap();
    assert_eq!(result.images.len(), 3);
    assert!(result.images[0].caption.is_none());
    assert!(result.images[2].caption.is_none());
    let image = &result.images[1];
    assert_eq!(image.alt, "Diagram alt");
    assert_eq!(image.title.as_deref(), Some("Image title"));
    assert_eq!(
        image.caption.as_deref().map(str::trim),
        Some("An authored caption with evidence.")
    );
    assert_eq!(result.body_html.matches("<figure>").count(), 1);
    assert!(
        result
            .body_html
            .contains("<figcaption><p>An <strong>authored</strong> caption")
    );
    assert!(
        result
            .body_html
            .contains("href=\"https://example.test/source\"")
    );
    assert_eq!(
        result
            .body_text
            .matches("An authored caption with evidence.")
            .count(),
        1
    );
    assert_eq!(
        result
            .body_text
            .matches("Unbound following paragraph.")
            .count(),
        1
    );
}

#[test]
fn repeated_image_urls_keep_distinct_caption_occurrences_and_projection_bounds() {
    let second = FIGURE.replace("An **authored** caption", "A **different** caption");
    let result = render(&format!("{FIGURE}\n{second}")).unwrap();
    assert_eq!(result.images[0].destination, result.images[1].destination);
    assert_ne!(result.images[0].caption, result.images[1].caption);
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    let bounded_source = "::::figure\n![x](x)\n\n:::figure-caption\nx\n:::\n::::\n".repeat(20);
    let body = ParsedMarkdown::parse(
        &bounded_source,
        Limits {
            output_bytes: 512,
            ..Default::default()
        },
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let error = body
        .render_with_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
            |_, _, _| Ok(None),
        )
        .unwrap_err();
    assert!(
        matches!(
            error,
            gazette_compiler::markdown::Error::Limit("figure projection bytes")
        ),
        "{error:?}"
    );
}

#[test]
fn malformed_ambiguous_hidden_and_nested_figures_are_rejected() {
    for source in [
        ":::figure-caption\nOrphan.\n:::\n".into(),
        ":::figure\n![only](https://example.test/one.png)\n:::\n".into(),
        "::::figure\n:::figure-caption\nNo image.\n:::\n::::\n".into(),
        FIGURE.replace(
            "[![Diagram alt]",
            "![extra](https://example.test/extra.png)\n\n[![Diagram alt]",
        ),
        FIGURE.replace(
            "An **authored** caption with [evidence](https://example.test/evidence).",
            "",
        ),
        FIGURE.replace(
            "An **authored** caption",
            "![caption image](https://example.test/caption.png) caption",
        ),
        FIGURE.replace("::::\n", ":::figure-caption\nSecond caption.\n:::\n::::\n"),
        format!(":::::figure\n{FIGURE}:::::\n"),
        format!(":::::decoration\n{FIGURE}:::::\n"),
        FIGURE.replace(
            "An **authored** caption",
            "::::decoration\nHidden\n::::\nCaption",
        ),
    ] {
        assert!(render(&source).is_err(), "accepted {source}");
    }
    assert!(
        ParsedManifest::parse(
            &THEME.replace(
                "semantic={kind='figure'}",
                "semantic={kind='figure'}\npresentation_only=true"
            ),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn figures_inside_faq_answers_leave_faq_collection_and_excerpt_intact() {
    let source = format!(
        "Before.\n\n<!-- more -->\n\n::::::questions\n:::::answer {{question='What does it show?'}}\n{FIGURE}:::::\n::::::\n"
    );
    let result = render(&source).unwrap();
    let faq = result
        .components
        .iter()
        .find_map(|component| component.faq.as_ref())
        .unwrap();
    assert_eq!(faq.questions.len(), 1);
    assert!(faq.questions[0].answer_html.contains("<figure>"));
    assert_eq!(
        result.images[0].caption.as_deref().map(str::trim),
        Some("An authored caption with evidence.")
    );
    assert!(result.summary.unwrap().html.contains("Before."));
}

#[test]
fn caption_projects_once_to_json_ld_search_and_markdown_without_invented_dates() {
    let config = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/private/input'}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{source='source:one',target='one'}]\nmachine={json_ld=true,markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\n";
    let compilation = workspace::compile(config, Default::default()).unwrap();
    let source = format!(
        "+++\n[content]\nid='content:page'\nsite='site:one'\ntitle='Page'\nlifecycle='draft'\nroute={{path='/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{FIGURE}"
    );
    let corpus = ParsedCorpus::parse(
        [Input {
            path: "one/index.md".parse().unwrap(),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    let page = resolved
        .project_page_with(
            &"content:page".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
        )
        .unwrap();
    let json = serde_json::to_value(page.json_ld()).unwrap();
    assert_eq!(json["@type"], "Article");
    assert_eq!(
        json["image"][0]["caption"].as_str().unwrap().trim(),
        "An authored caption with evidence."
    );
    assert_eq!(json["image"][0]["description"], "Diagram alt");
    assert_eq!(json["image"][0]["name"], "Image title");
    assert!(json.get("datePublished").is_none());
    assert_eq!(
        page.body_markdown
            .as_ref()
            .unwrap()
            .matches("caption with")
            .count(),
        1
    );
    let search = serde_json::to_value(page.search()).unwrap();
    assert_eq!(
        search["body"]
            .as_str()
            .unwrap()
            .matches("An authored caption with evidence.")
            .count(),
        1
    );
}

#[test]
fn notebook_attachment_captions_stay_visible_without_publishing_data_uri_metadata() {
    use gazette_compiler::notebook::ParsedNotebook;
    use gazette_core::{raster, source::Sha256Digest};
    let source = "::::figure\n![Attachment alt](attachment:plot.png)\n\n:::figure-caption\nThe retained notebook caption.\n:::\n::::\n";
    let bytes = serde_json::to_vec(&serde_json::json!({
        "nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{
            "cell_type":"markdown","id":"figure","metadata":{},"source":source,
            "attachments":{"plot.png":{"image/png":"aGVsbG8="}}
        }]
    }))
    .unwrap();
    let notebook = ParsedNotebook::parse(&bytes, Default::default()).unwrap();
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    // This provider checks byte transport; native raster tests own pixel decoding.
    let provider = |format, bytes: &[u8], _| {
        Ok(raster::Inspection {
            display: None,
            format,
            width: 2,
            height: 3,
            bytes: bytes.len(),
            sha256: Sha256Digest::of(bytes),
            tool: "test.transport-only",
        })
    };
    let rendered = notebook
        .render_with_components(
            "page",
            Default::default(),
            &provider,
            &gazette_adapter_markdown::Comrak,
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
            |_, _, _| panic!("cell attachment must not leave its local resolver"),
        )
        .unwrap();
    assert!(
        rendered
            .html
            .contains("<figcaption><p>The retained notebook caption.</p>")
    );
    assert_eq!(
        rendered
            .text
            .matches("The retained notebook caption.")
            .count(),
        1
    );
    assert!(rendered.image_uses.is_empty());
    assert_eq!(notebook.source(), bytes);
}

#[test]
fn figure_template_guard_rejects_changed_images_captions_wrappers_and_oversized_output() {
    struct Guard;
    impl ComponentRenderer for Guard {
        fn render(
            &self,
            binding: &ComponentBinding,
            body: &Fragment,
            excerpt: bool,
        ) -> Result<String, Error> {
            let output = Renderer.render(binding, body, excerpt)?;
            if binding.name.as_str() == "figure-caption" {
                let semantic = ComponentSemantic::FigureCaption;
                for bad in [
                    output.replace("authored", "invented"),
                    output.replace("figcaption", "div"),
                    output.replace("<figcaption>", "<figcaption-extra>"),
                    output.replace("<figcaption>", "<figcaption\u{a0}class='caption'>"),
                    output.replace("<p>", "<p>Extra "),
                ] {
                    assert!(
                        validate_figure_html(binding, &semantic, body, &bad, Default::default())
                            .is_err()
                    );
                }
                assert!(
                    validate_figure_html(
                        binding,
                        &semantic,
                        body,
                        &output,
                        gazette_compiler::html::Limits {
                            input_bytes: 1,
                            ..Default::default()
                        }
                    )
                    .is_err()
                );
                for opening in [
                    "<figcaption class=\"caption\">",
                    "<figcaption title=\"one > two\">",
                    "<figcaption title='one > two'>",
                ] {
                    let styled = output.replace("<figcaption>", opening);
                    validate_figure_html(binding, &semantic, body, &styled, Default::default())?;
                }
            } else if binding.name.as_str() == "figure" {
                let changed = output.replace("image.png", "other.png");
                assert!(
                    validate_figure_html(
                        binding,
                        &ComponentSemantic::Figure,
                        body,
                        &changed,
                        Default::default()
                    )
                    .is_err()
                );
            }
            Ok(output)
        }
    }
    let manifest = ParsedManifest::parse(THEME, Default::default()).unwrap();
    ParsedMarkdown::parse(FIGURE, Limits::default(), &gazette_adapter_markdown::Comrak)
        .unwrap()
        .render_with_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Guard,
            }),
            |_, _, _| Ok(None),
        )
        .unwrap();
}
