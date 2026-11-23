//! The reading header borrows the formatter's first H1 without changing any authored output.

use gazette_compiler::{
    authored,
    corpus::{Input, ParsedCorpus},
    workspace,
};
use gazette_core::render::LimitedWriter;
use serde_json::json;

fn configuration() -> workspace::Compilation {
    workspace::compile("[[principals]]\nid='person:author'\nkind='person'\nname='Author'\n[[sites]]\nid='site:one'\npublisher='person:author'\ndeployable=false\n[[sources]]\nid='source:one'\nlocation={kind='filesystem',root='/not-read'}\n[[brands]]\nid='brand:one'\nname='One'\n[[brands.publications]]\nsite='site:one'\npublic_base_url='https://example.test/'\nauthored=[{source='source:one',target='content'}]\n",Default::default()).unwrap()
}
fn source(body: &str) -> String {
    format!(
        "+++\n[content]\nid='article:one'\nsite='site:one'\ntitle='Declared title'\nlifecycle='published'\nroute={{path='/article/'}}\nattributions=[{{principal='person:author',role='author'}}]\nschema_type='Article'\nkeywords=['authored phrase']\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n{body}"
    )
}
fn corpus(source: &str) -> ParsedCorpus<'_> {
    ParsedCorpus::parse(
        [Input {
            path: "content/article.md".parse().unwrap(),
            bytes: source.as_bytes(),
        }],
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .unwrap()
}

fn assert_serialized_page_schema(model: &serde_json::Value) {
    let schema = schemars::generate::SchemaSettings::draft2020_12()
        .for_serialize()
        .into_generator()
        .into_root_schema_for::<authored::PageProjection<'static>>();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    validator.validate(model).expect("actual serialized page");
    for field in ["opening", "summary"] {
        let mut malformed = model.clone();
        malformed[field] = json!("not a structured projection");
        assert!(!validator.is_valid(&malformed), "malformed {field}");
    }
}

#[test]
fn opening_borrows_exact_formatter_html_while_metadata_and_search_keep_the_full_article() {
    let source = source(
        "# *Heading* [jump](#next) `code` {#authored-title}\n\nLead with a note[^n].\n\n<!-- more -->\n\n## Next\n\n| A | B |\n| - | - |\n| one | two |\n\n[^n]: Footnote body.\n",
    );
    let config = configuration();
    let corpus = corpus(&source);
    let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
    let id = "article:one".parse().unwrap();
    let page = resolved
        .project_page(
            &id,
            &gazette_core::raster::unavailable,
            &gazette_adapter_markdown::Comrak,
        )
        .unwrap();
    let opening = page.opening().unwrap();
    assert!(
        opening
            .heading_html
            .starts_with("<h1 id=\"authored-title\">")
    );
    assert!(opening.heading_html.contains("<em>Heading</em>"));
    assert!(opening.heading_html.contains("<code>code</code>"));
    assert!(
        opening
            .heading_html
            .contains("href=\"https://example.test/article/#next\"")
    );
    assert!(opening.remainder_html.contains("<table>"));
    assert!(opening.remainder_html.contains("Footnote body."));
    assert_eq!(opening.heading_html.as_ptr(), page.body_html.as_ptr());
    assert_eq!(
        [opening.heading_html, opening.remainder_html].concat(),
        page.body_html
    );
    assert_eq!(page.data.headings[0].id, "authored-title");
    let model = json!(page);
    assert!(model.get("summary").is_some());
    assert_serialized_page_schema(&model);
    let search = json!(page.search());
    let ld = json!(page.json_ld());
    assert_eq!(model["opening"]["heading_html"], opening.heading_html);
    assert_eq!(search["body"], page.body_text);
    assert!(search.get("opening").is_none());
    assert_eq!(ld["headline"], "Declared title");
    let discovery = resolved.discovery().unwrap();
    let public = json!(page.discovery_view(&discovery).unwrap());
    assert_eq!(public["opening"], model["opening"]);
    // The extra wire representation remains covered by the existing complete-model bound.
    let mut without = model.clone();
    without.as_object_mut().unwrap().remove("opening");
    let budget = serde_json::to_vec(&without).unwrap().len();
    let mut writer = LimitedWriter::new(std::io::sink(), budget);
    assert!(serde_json::to_writer(&mut writer, &page).is_err());
    assert!(writer.exceeded());
}

#[test]
fn absent_or_later_h1_has_no_opening_projection_or_fabricated_title() {
    let config = configuration();
    for body in [
        "Only prose.\n",
        "Prose first.\n\n# Later\n",
        "> # Nested\n",
        "## Section\n",
    ] {
        let source = source(body);
        let corpus = corpus(&source);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let page = resolved
            .project_page(
                &"article:one".parse().unwrap(),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        assert!(page.opening().is_none());
        let model = json!(page);
        assert!(model.get("opening").is_none());
        assert!(model.get("summary").is_none());
        assert_serialized_page_schema(&model);
        assert_eq!(page.data.content.title, "Declared title");
    }
}

#[test]
fn opening_and_summary_are_independent_optional_serialized_projections() {
    let config = configuration();
    for (body, opening, summary) in [
        ("# Leading heading\n\nWhole body.\n", true, false),
        ("Lead prose.\n\n<!-- more -->\n\nRemainder.\n", false, true),
    ] {
        let source = source(body);
        let corpus = corpus(&source);
        let resolved = authored::resolve(&config, &corpus, Default::default()).unwrap();
        let page = resolved
            .project_page(
                &"article:one".parse().unwrap(),
                &gazette_core::raster::unavailable,
                &gazette_adapter_markdown::Comrak,
            )
            .unwrap();
        let model = json!(page);
        assert_eq!(model.get("opening").is_some(), opening);
        assert_eq!(model.get("summary").is_some(), summary);
        assert_serialized_page_schema(&model);
    }
}
