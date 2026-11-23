//! Rich reader facts survive the public boundary after native component projection.

use gazette_compiler::{authored, markdown::ComponentPresentation, theme_package::ParsedManifest};
use gazette_core::markdown::{ComponentBinding, Error, Fragment};
use gazette_ports::markdown::ComponentRenderer;
use serde_json::json;

struct Renderer;
impl ComponentRenderer for Renderer {
    fn render(
        &self,
        binding: &ComponentBinding,
        body: &Fragment,
        _: bool,
    ) -> Result<String, Error> {
        let tag = match binding.name.as_str() {
            "figure" => "figure",
            "caption" => "figcaption",
            "questions" | "decoration" => "section",
            "answer" => {
                return Ok(format!(
                    "<details><summary>{}</summary>{}</details>",
                    binding.arguments["prompt"].as_str().unwrap(),
                    body.html
                ));
            }
            _ => return Err(Error::InvalidPlan),
        };
        Ok(format!("<{tag}>{}</{tag}>", body.html))
    }
}

#[test]
fn captions_images_and_ordered_rich_faq_answers_remain_reader_facts() {
    let theme = "package='theme:one'\nnamespace='one'\ntemplates='templates'\nrender={page='page.html'}\n\
        [components.figure]\ntemplate='figure.html'\nsemantic={kind='figure'}\n\
        [components.caption]\ntemplate='caption.html'\nsemantic={kind='figure-caption'}\n\
        [components.questions]\ntemplate='faq.html'\nsemantic={kind='faq'}\n\
        [components.answer]\ntemplate='answer.html'\nsemantic={kind='faq-item',question_argument='prompt'}\narguments={type='object',required=['prompt'],additionalProperties=false,properties={prompt={type='string'}}}\n\
        [components.decoration]\ntemplate='decoration.html'\npresentation_only=true\n";
    let body = "::::figure\n![Actual alt](https://media.test/figure.png \"Actual title\")\n\n:::caption\nThe **actual** figure caption.\n:::\n::::\n\n\
        :::decoration\n![Frame](https://media.test/frame.png)\n:::\n\n\
        ::::questions\n:::answer {prompt='First question?'}\nKeep **this** [answer](https://example.test/answer).\n:::\n\n:::answer {prompt='Second question?'}\nSecond answer.\n:::\n::::\n";
    let source = super::doc(
        "content:rich",
        "site:a",
        "published",
        "",
        "kind='page'\nfamily={kind='utility'}",
        body,
    );
    let files = vec![("rich.md", source)];
    let compilation = super::super::configuration();
    let corpus = super::parse(&files, Default::default());
    let resolved = authored::resolve(&compilation, &corpus, Default::default()).unwrap();
    let manifest = ParsedManifest::parse(theme, Default::default()).unwrap();
    let page = resolved
        .project_page_with(
            &super::key("content:rich"),
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
            None,
            Some(ComponentPresentation {
                definitions: manifest.components(),
                renderer: &Renderer,
            }),
        )
        .unwrap();
    let value = json!(page.json_ld().public());
    super::standards_only(&value);
    assert_eq!(value["image"], json!(page.json_ld())["image"]);
    assert_eq!(value["image"].as_array().unwrap().len(), 1);
    assert_eq!(
        value["image"][0]["contentUrl"],
        "https://media.test/figure.png"
    );
    assert_eq!(value["image"][0]["description"], "Actual alt");
    assert_eq!(value["image"][0]["name"], "Actual title");
    assert_eq!(
        value["image"][0]["caption"].as_str().unwrap().trim(),
        "The actual figure caption."
    );
    assert_eq!(value["hasPart"], json!(page.json_ld())["hasPart"]);
    assert_eq!(value["hasPart"][0]["@type"], "FAQPage");
    let questions = value["hasPart"][0]["mainEntity"].as_array().unwrap();
    assert_eq!(questions.len(), 2);
    assert_eq!(questions[0]["name"], "First question?");
    assert_eq!(questions[1]["name"], "Second question?");
    let answer = questions[0]["acceptedAnswer"]["text"].as_str().unwrap();
    assert!(answer.contains("<strong>this</strong>"));
    assert!(answer.contains("href=\"https://example.test/answer\""));
    assert!(page.body_html.contains(answer));
}
