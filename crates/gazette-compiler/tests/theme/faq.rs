use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    html,
    markdown::{ComponentPresentation, Limits, ParsedMarkdown, validate_faq_html},
    theme::ComponentSemantic,
    theme_package::ParsedManifest,
    workspace,
};
use gazette_core::markdown::{ComponentBinding, Error, Fragment};
use gazette_ports::markdown::ComponentRenderer;
use serde_json::Value;

const THEME: &str = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n[components.questions]\ntemplate='group.html'\nsemantic={kind='faq'}\n[components.answer]\ntemplate='item.html'\nsemantic={kind='faq-item',question_argument='prompt'}\narguments={type='object',required=['prompt'],additionalProperties=false,properties={prompt={type='string'}}}\n[components.decoration]\ntemplate='decoration.html'\npresentation_only=true\n";

fn manifest(source: &str) -> ParsedManifest<'_> {
    ParsedManifest::parse(source, Default::default()).unwrap()
}
struct Renderer;
impl ComponentRenderer for Renderer {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        _: bool,
    ) -> Result<String, Error> {
        Ok(
            if let Some(question) = binding.arguments.get("prompt").and_then(Value::as_str) {
                format!(
                    "<details><summary>{}</summary><div>{}</div></details>",
                    question
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;"),
                    body.html
                )
            } else {
                format!("<section>{}</section>", body.html)
            },
        )
    }
}

#[test]
fn faq_questions_and_rich_answers_come_from_the_checked_component_body_in_source_order() {
    let manifest = manifest(THEME);
    let source = "::::questions\n## Before starting\n\n:::answer {prompt='Why & how?'}\nKeep **one** [answer](https://example.test/details).\n\n- First\n- Second\n:::\n\n:::answer {prompt='Next?'}\nAnother answer.\n:::\n::::\n";
    let body = ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
        .unwrap();
    let rendered = body
        .render_with_components(
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
            |_, _, _| Ok(None),
        )
        .unwrap();
    let faq = rendered.components[0].faq.as_ref().unwrap();
    assert_eq!(faq.questions.len(), 2);
    assert_eq!(faq.questions[0].question, "Why & how?");
    assert_eq!(faq.questions[1].question, "Next?");
    assert!(
        faq.questions[0]
            .answer_html
            .contains("<strong>one</strong>")
    );
    assert!(
        faq.questions[0]
            .answer_html
            .contains("href=\"https://example.test/details\"")
    );
    assert!(faq.questions[0].answer_html.contains("<li>Second</li>"));
    assert!(rendered.body_html.contains(&faq.questions[0].answer_html));
    assert!(rendered.body_html.contains("Why &amp; how?"));
    assert!(rendered.body_text.contains("Why & how?\nKeep one answer."));
    assert!(rendered.body_text.contains("Next?\nAnother answer."));
    assert_eq!(rendered.body_text.matches("Why & how?").count(), 1);
    assert_eq!(rendered.body_text.matches("Next?").count(), 1);
    assert!(rendered.components[1].faq.is_none());
    assert_eq!(body.source(), source);
}

#[test]
fn faq_reading_text_keeps_template_inputs_and_text_rendering_unchanged() {
    #[derive(Default)]
    struct TextRenderer(std::sync::Mutex<Vec<(String, String)>>);
    impl ComponentRenderer for TextRenderer {
        fn render(
            &self,
            binding: &ComponentBinding,
            body: &Fragment,
            excerpt: bool,
        ) -> Result<String, Error> {
            self.0
                .lock()
                .unwrap()
                .push((binding.name.to_string(), body.text.clone()));
            let escape = |text: &str| {
                text.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            };
            let (output, semantic) = match binding.name.as_str() {
                "answer" => (
                    format!(
                        "<details><summary>{}</summary><p>{}</p></details>",
                        escape(binding.arguments["prompt"].as_str().unwrap()),
                        escape(&body.text)
                    ),
                    Some(ComponentSemantic::FaqItem {
                        question_argument: "prompt".into(),
                    }),
                ),
                "wrapper" => (format!("<aside>{}</aside>", escape(&body.text)), None),
                _ => (
                    Renderer.render(binding, body, excerpt)?,
                    Some(ComponentSemantic::Faq),
                ),
            };
            if let Some(semantic) = semantic {
                validate_faq_html(binding, &semantic, body, &output, html::Limits::default())?;
            }
            Ok(output)
        }
    }
    let selected = format!("{THEME}\n[components.wrapper]\ntemplate='wrapper.html'\n");
    let ordinary = selected.replace("semantic={kind='faq'}\n", "").replace(
        "semantic={kind='faq-item',question_argument='prompt'}\n",
        "",
    );
    for wrapped in [false, true] {
        let source = "::::questions\n:::answer {prompt='Why?'}\nAnswer.\n:::\n::::\n";
        let source = if wrapped {
            format!(":::::wrapper\n{source}:::::\n")
        } else {
            source.into()
        };
        let body = ParsedMarkdown::parse(
            &source,
            Limits::default(),
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
        let render = |definition: &str| {
            let manifest = manifest(definition);
            let renderer = TextRenderer::default();
            let rendered = body
                .render_with_components(
                    Some(ComponentPresentation {
                        definitions: manifest.components(),
                        renderer: &renderer,
                    }),
                    |_, _, _| Ok(None),
                )
                .unwrap();
            (rendered, renderer.0.into_inner().unwrap())
        };
        let (rendered, inputs) = render(&selected);
        let (baseline, original_inputs) = render(&ordinary);
        assert_eq!(inputs, original_inputs);
        assert!(inputs.iter().all(|(_, text)| !text.contains("Why?")));
        assert_eq!(rendered.body_html, baseline.body_html);
        assert_eq!(rendered.body_text, "Why?\nAnswer.\n");
        assert_eq!(baseline.body_text, "Answer.\n");
    }
}

#[test]
fn faq_admission_refuses_orphan_nested_empty_or_hidden_groups_and_invalid_questions() {
    let manifest = manifest(THEME);
    for source in [
        ":::answer {prompt='Orphan?'}\nAnswer.\n:::\n",
        ":::questions\nNo answers.\n:::\n",
        "::::questions\n:::answer {prompt=''}\nAnswer.\n:::\n::::\n",
        "::::questions\n:::answer {prompt=42}\nAnswer.\n:::\n::::\n",
        "::::questions\n:::answer {prompt='Empty answer?'}\n:::\n::::\n",
        ":::::questions\n::::questions\n:::answer {prompt='Nested?'}\nAnswer.\n:::\n::::\n:::::\n",
        ":::::decoration\n::::questions\n:::answer {prompt='Hidden?'}\nAnswer.\n:::\n::::\n:::::\n",
    ] {
        let body =
            ParsedMarkdown::parse(source, Limits::default(), &gazette_adapter_markdown::Comrak)
                .unwrap();
        assert!(
            body.render_with_components(
                Some(ComponentPresentation {
                    definitions: manifest.components(),
                    renderer: &Renderer
                }),
                |_, _, _| Ok(None)
            )
            .is_err(),
            "{source}"
        );
    }
    assert!(
        ParsedManifest::parse(
            &THEME.replace(
                "semantic={kind='faq'}",
                "semantic={kind='faq'}\npresentation_only=true"
            ),
            Default::default()
        )
        .is_err()
    );
    assert!(
        ParsedManifest::parse(
            &THEME.replace("question_argument='prompt'", "question_argument=''"),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn actual_item_markup_must_keep_the_question_and_answer_words() {
    let manifest = manifest(THEME);
    let body = ParsedMarkdown::parse(
        ":::answer {prompt='What & why?'}\nKeep **this** answer.\n:::\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let binding = manifest
        .components()
        .bind(body.directives(), Default::default())
        .unwrap()
        .remove(0);
    let answer = Fragment {
        link_destinations: Vec::new(),
        images: Vec::new(),
        html: "<p>Keep <strong>this</strong> answer.</p>\n".into(),
        text: "Keep this answer.\n".into(),
    };
    let expected = Renderer.render(&binding, &answer, false).unwrap();
    validate_faq_html(
        &binding,
        &ComponentSemantic::FaqItem {
            question_argument: "prompt".into(),
        },
        &answer,
        &expected,
        html::Limits::default(),
    )
    .unwrap();
    for wrong in [
        expected.replace("this", "another"),
        expected.replace("What &amp; why?", "Different?"),
        expected.replace(&answer.html, ""),
        format!("{expected}<p>Extra promise.</p>"),
    ] {
        assert!(
            validate_faq_html(
                &binding,
                &ComponentSemantic::FaqItem {
                    question_argument: "prompt".into()
                },
                &answer,
                &wrong,
                html::Limits::default()
            )
            .is_err()
        );
    }
    assert!(
        validate_faq_html(
            &binding,
            &ComponentSemantic::FaqItem {
                question_argument: "prompt".into()
            },
            &answer,
            &expected,
            html::Limits {
                input_bytes: 8,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(matches!(
        manifest
            .components()
            .definition(&"answer".parse().unwrap())
            .unwrap()
            .semantic,
        Some(ComponentSemantic::FaqItem { .. })
    ));
}

#[test]
fn faq_json_ld_preserves_collection_description_children_and_navigation() {
    let config = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:one'\npublisher='owner:one'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/private/input'}\n[[brands]]\nid='brand:one'\nname='Example'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{source='source:one',target='one'}]\nmachine={json_ld=true,markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\n";
    let compilation = workspace::compile(config, Default::default()).unwrap();
    let source = "format: gazette-landing/v1\ncontent:\n  id: content:home\n  site: site:one\n  title: Product collection\n  description: The original collection description.\n  lifecycle: draft\n  route: {path: /}\n  structure: {kind: collection}\nsections:\n  - component: questions\n    heading: {level: 2, text: Before starting}\n    sections:\n      - component: answer\n        arguments: {prompt: 'First question?'}\n        body: 'The **first** answer.'\n      - component: answer\n        arguments: {prompt: 'Second question?'}\n        body: 'Read [the guide](https://example.test/guide/).'\n";
    let child = "+++\n[content]\nid='content:child'\nsite='site:one'\ntitle='Existing child collection'\nlifecycle='draft'\nroute={path='/child/'}\nstructure={kind='collection',parent='content:home'}\n+++\nChild account.\n";
    let corpus = ParsedCorpus::parse(
        [
            Input {
                path: "one/index.landing.yaml".parse().unwrap(),
                bytes: source.as_bytes(),
            },
            Input {
                path: "one/child.md".parse().unwrap(),
                bytes: child.as_bytes(),
            },
        ],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let manifest = manifest(THEME);
    let page = resolved
        .project_page_with(
            &"content:home".parse().unwrap(),
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
    assert_eq!(json["@type"], "CollectionPage");
    assert_eq!(json["description"], "The original collection description.");
    assert_eq!(json["url"], "https://example.test/");
    assert_eq!(json["hasPart"][0]["@type"], "CollectionPage");
    assert_eq!(json["hasPart"][0]["identifier"], "content:child");
    assert_eq!(json["hasPart"][1]["@type"], "FAQPage");
    let questions = json["hasPart"][1]["mainEntity"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    for (item, expected) in questions
        .iter()
        .zip(["First question?", "Second question?"])
    {
        assert_eq!(item["@type"], "Question");
        assert_eq!(item["name"], expected);
        assert_eq!(item["acceptedAnswer"]["@type"], "Answer");
        assert!(
            page.body_html
                .contains(item["acceptedAnswer"]["text"].as_str().unwrap())
        );
    }
    assert!(!json.to_string().contains("/private/input"));
    assert!(
        page.body_markdown
            .as_ref()
            .unwrap()
            .contains("First question?")
    );
    assert_eq!(json["hasPart"][1]["isPartOf"]["@id"], json["@id"]);
    let expected_text =
        "Before starting\nFirst question?\nThe first answer.\nSecond question?\nRead the guide.\n";
    assert_eq!(json["text"], expected_text);
    let search = serde_json::to_value(page.search()).unwrap();
    assert_eq!(search["body"], expected_text);
    let ordinary = THEME.replace("semantic={kind='faq'}\n", "").replace(
        "semantic={kind='faq-item',question_argument='prompt'}\n",
        "",
    );
    let baseline_manifest = ParsedManifest::parse(&ordinary, Default::default()).unwrap();
    let baseline = resolved
        .project_page_with(
            &"content:home".parse().unwrap(),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(ComponentPresentation {
                definitions: baseline_manifest.components(),
                renderer: &Renderer,
            }),
        )
        .unwrap();
    assert_eq!(page.body_html, baseline.body_html);
    let baseline_json = serde_json::to_value(baseline.json_ld()).unwrap();
    let mut existing = json.clone();
    existing["hasPart"].as_array_mut().unwrap().pop();
    assert!(!baseline.body_text.contains("First question?"));
    existing["text"] = baseline_json["text"].clone();
    assert_eq!(existing, baseline_json);
}

#[test]
fn faq_word_agreement_keeps_inline_words_and_native_block_boundaries() {
    let selected = manifest(THEME);
    let source = ParsedMarkdown::parse(
        ":::answer {prompt='Why?'}\nAnswer.\n:::\n",
        Limits::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap();
    let binding = selected
        .components()
        .bind(source.directives(), Default::default())
        .unwrap()
        .remove(0);
    let semantic = ComponentSemantic::FaqItem {
        question_argument: "prompt".into(),
    };
    let body = Fragment {
        link_destinations: Vec::new(),
        images: Vec::new(),
        html: "<p>A pre<strong>fix</strong>.</p><p>Next paragraph.</p>".into(),
        text: "A prefix.\nNext paragraph.".into(),
    };
    let good = "<details><summary>Why?</summary><div><p>A prefix.</p><p>Next paragraph.</p></div></details>";
    validate_faq_html(&binding, &semantic, &body, good, html::Limits::default()).unwrap();
    assert!(
        validate_faq_html(
            &binding,
            &semantic,
            &body,
            &good.replace("prefix", "pre fix"),
            html::Limits::default()
        )
        .is_err()
    );
    assert!(
        validate_faq_html(
            &binding,
            &semantic,
            &body,
            &good.replace(".</p><p>Next", ".Next"),
            html::Limits::default()
        )
        .is_err()
    );
    let group = Fragment {
        link_destinations: Vec::new(),
        images: Vec::new(),
        html: good.into(),
        text: String::new(),
    };
    validate_faq_html(
        &binding,
        &ComponentSemantic::Faq,
        &group,
        &format!("<section>{good}</section>"),
        html::Limits::default(),
    )
    .unwrap();
    assert!(
        validate_faq_html(
            &binding,
            &ComponentSemantic::Faq,
            &group,
            "<section></section>",
            html::Limits::default()
        )
        .is_err()
    );
}
