//! Literal authored-source composition through real compilers, not a complete publication API.

use std::{collections::BTreeMap, error::Error as StdError};

use gazette_adapter_tera::TemplateSet;
use gazette_core::source::{CapturedSource, Content, EntryPath, InventoryBuilder, Sha256Digest};
use gazette_sdk::{
    authored, corpus,
    publication::{Key, content},
    toml::PathSegment,
    workspace,
};
use serde_json::{Value, json};

const WORKSPACE: &str = "[[principals]]\nid='owner:one'\nkind='person'\n[[sites]]\nid='site:home'\npublisher='owner:one'\ndeployable=false\n[[sites]]\nid='site:other'\npublisher='owner:one'\ndeployable=true\n";
const COLLECTION: &str = "+++\n[content]\nid='collection:one'\nsite='site:home'\ntitle='Retained inquiry'\nlifecycle='published'\nstructure={kind='collection'}\n+++\n";
const JOURNEY: &str = "+++\n[content]\nid='journey:one'\nsite='site:home'\ntitle='Reading <map> & Ω'\nlifecycle='published'\n[content.structure]\nkind='journey'\ncollection='collection:one'\nmembers=[{content='article:z', primary=true}, {content='article:a', primary=true}, {content='article:m', primary=true}]\n+++\n# Journey introduction\n";
const ARCHIVED: &str = "+++\nrelated=['article:foreign']\n[content]\nid='article:z'\nsite='site:home'\ntitle='Archive <tag>'\nlifecycle='archived'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n# Archived\n\n**Keep** & remember.\n";
const DRAFT: &str = "+++\r\n[content]\r\nid='article:a'\r\nsite='site:home'\r\ntitle='Same title'\r\nlifecycle='draft'\r\n[content.structure]\r\nkind='page'\r\nfamily={kind='documentation'}\r\n+++\r\n# Draft é\r\n\r\n[Guide](/guide)\r\n";
const FOREIGN: &str = "+++\n[content]\nid='article:foreign'\nsite='site:other'\ntitle='Foreign reference'\nlifecycle='published'\nstructure={kind='page',family={kind='article'}}\n+++\nForeign authored body.\n";
const PUBLISHED: &str = "+++\n[content]\nid='article:m'\nsite='site:home'\ntitle='Same title'\nlifecycle='published'\n[content.structure]\nkind='page'\nfamily={kind='article'}\n+++\n# Current\n\n`<code>` stays text.\n";
const ASSET: &[u8] = &[0xff, 0x80, 0, 42];
const TEMPLATE: &str = concat!(
    "<main data-id=\"{{ journey.id }}\"><h1>{{ journey.title }}</h1>",
    "{% for step in steps %}<article data-id=\"{{ step.id }}\" data-site=\"{{ step.site }}\" ",
    "data-state=\"{{ step.lifecycle }}\"><h2>{{ step.title }}</h2>",
    "{{ bodies[step.id] | safe }}</article>{% endfor %}</main>"
);

fn key(value: &str) -> Key {
    value.parse().expect("fixture identity")
}

fn sources() -> [(&'static str, &'static [u8]); 7] {
    [
        ("other/z-draft.md", DRAFT.as_bytes()),
        ("home/a-current.md", PUBLISHED.as_bytes()),
        ("home/journey.md", JOURNEY.as_bytes()),
        ("home/image.bin", ASSET),
        ("home/m-history.md", ARCHIVED.as_bytes()),
        ("home/collection.md", COLLECTION.as_bytes()),
        ("other/foreign.md", FOREIGN.as_bytes()),
    ]
}

fn parse<'s>(sources: &[(&str, &'s [u8])]) -> corpus::ParsedCorpus<'s> {
    corpus::ParsedCorpus::parse(
        sources.iter().map(|(path, bytes)| corpus::Input {
            path: path.parse().expect("logical source path"),
            bytes,
        }),
        Default::default(),
        &gazette_adapter_markdown::Comrak,
    )
    .expect("real retained Markdown and TOML syntax")
}

fn templates(template: &str) -> Result<TemplateSet, gazette_adapter_tera::Error> {
    let path: EntryPath = "journey.html".parse().expect("template path");
    let mut inventory = InventoryBuilder::new(Default::default());
    inventory
        .insert(
            path.clone(),
            Content::File {
                bytes: template.len() as u64,
                sha256: Sha256Digest::of(template.as_bytes()),
            },
        )
        .expect("template inventory");
    let captured = CapturedSource::from_parts(
        inventory.finish(),
        BTreeMap::from([(path, template.as_bytes().to_vec())]),
    )
    .expect("complete captured template");
    TemplateSet::compile(&captured, Default::default())
}

// Compose production seams in this fixture only; no new admission/build behavior is implied.
fn project(
    corpus: &corpus::ParsedCorpus<'_>,
    template: &str,
) -> Result<(Vec<u8>, Vec<u8>), Box<dyn StdError>> {
    let workspace = workspace::compile(WORKSPACE, Default::default())?;
    let resolved = authored::resolve(&workspace, corpus, content::Limits::default())?;
    let view = resolved
        .content()
        .graph()
        .journey_view(&key("journey:one"))
        .expect("journey");
    let bodies = view
        .steps
        .iter()
        .map(|step| {
            let document = resolved.document(&step.id).expect("same resolved document");
            // Only the real Markdown renderer's successful fragments receive template `safe`.
            document.render_html().map(|body| (step.id.as_str(), body))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let mut context = serde_json::to_value(&view)?;
    context["bodies"] = serde_json::to_value(bodies)?;
    let machine = serde_json::to_vec(&context)?;
    let html = templates(template)?.render(&"journey.html".parse()?, &context)?;
    Ok((html, machine))
}

#[test]
fn authored_files_share_exact_navigation_and_retained_markdown_across_html_and_json() {
    let inputs = sources();
    let corpus = parse(&inputs);
    let workspace = workspace::compile(WORKSPACE, Default::default()).expect("workspace");
    let resolved =
        authored::resolve(&workspace, &corpus, content::Limits::default()).expect("authored graph");
    assert!(std::ptr::eq(resolved.corpus(), &corpus));
    assert_eq!(resolved.corpus().files().len(), 7);
    let corpus::File::Asset(bytes) = &resolved.corpus().files()["home/image.bin"] else {
        panic!("opaque source must remain present");
    };
    assert_eq!(*bytes, ASSET);
    let corpus::File::Document(original) = &corpus.files()["other/z-draft.md"] else {
        panic!("literal framed document");
    };
    let retained = resolved
        .document(&key("article:a"))
        .expect("exact document");
    assert!(std::ptr::eq(retained, original.as_ref()));
    assert_eq!(retained.source_sha256(), Sha256Digest::of(DRAFT.as_bytes()));
    let origin = resolved
        .origin(
            &key("article:a"),
            &[
                PathSegment::Key("content".into()),
                PathSegment::Key("title".into()),
            ],
        )
        .expect("native field origin");
    let span = origin.span.expect("explicit title range");
    assert_eq!(origin.path.as_str(), "other/z-draft.md");
    assert_eq!(origin.source_sha256, Sha256Digest::of(DRAFT.as_bytes()));
    assert_eq!(&DRAFT[span.start..span.end], "'Same title'");
    assert_eq!(resolved.content().graph().related().len(), 1);
    let graph = resolved.content().graph();
    let related = graph
        .related()
        .iter()
        .next()
        .expect("retained related edge");
    assert_eq!(related.source, key("article:z"));
    assert_eq!(related.target, key("article:foreign"));
    assert_ne!(
        graph.node(&related.source).expect("source").site,
        graph.node(&related.target).expect("foreign target").site,
        "relatedness can cross sites without becoming containment"
    );
    assert_eq!(
        graph
            .editorial_parent(&key("journey:one"))
            .expect("collection")
            .id,
        key("collection:one")
    );
    assert_eq!(
        graph
            .primary_journey(&key("article:a"))
            .expect("sole journey")
            .id,
        key("journey:one")
    );

    let (html, machine) = project(&corpus, TEMPLATE).expect("connected projection");
    assert_eq!(
        String::from_utf8(html.clone()).expect("UTF-8"),
        concat!(
            "<main data-id=\"journey:one\"><h1>Reading &lt;map&gt; &amp; Ω</h1>",
            "<article data-id=\"article:z\" data-site=\"site:home\" data-state=\"archived\"><h2>Archive &lt;tag&gt;</h2>",
            "<h1 id=\"archived\">Archived</h1>\n<p><strong>Keep</strong> &amp; remember.</p>\n</article>",
            "<article data-id=\"article:a\" data-site=\"site:home\" data-state=\"draft\"><h2>Same title</h2>",
            "<h1 id=\"draft-e\">Draft é</h1>\n<p><a href=\"/guide\">Guide</a></p>\n</article>",
            "<article data-id=\"article:m\" data-site=\"site:home\" data-state=\"published\"><h2>Same title</h2>",
            "<h1 id=\"current\">Current</h1>\n<p><code>&lt;code&gt;</code> stays text.</p>\n</article></main>"
        )
    );
    let machine_view: Value = serde_json::from_slice(&machine).expect("valid JSON");
    assert_eq!(machine_view["journey"]["id"], "journey:one");
    assert_eq!(machine_view["journey"]["title"], "Reading <map> & Ω");
    let steps = machine_view["steps"].as_array().expect("ordered steps");
    assert_eq!(
        steps
            .iter()
            .map(|step| json!([step["id"], step["site"], step["title"], step["lifecycle"],]))
            .collect::<Vec<_>>(),
        vec![
            json!(["article:z", "site:home", "Archive <tag>", "archived"]),
            json!(["article:a", "site:home", "Same title", "draft"]),
            json!(["article:m", "site:home", "Same title", "published"]),
        ]
    );
    for step in steps {
        let id = step["id"].as_str().expect("exact identity");
        let body = resolved
            .document(&key(id))
            .expect("retained body")
            .render_html()
            .expect("body renderer");
        assert_eq!(machine_view["bodies"][id], body);
    }
    let mut reversed = inputs;
    reversed.reverse();
    assert_eq!(
        (html, machine),
        project(&parse(&reversed), TEMPLATE).expect("ordering independence")
    );
}

#[test]
fn authored_site_and_member_failures_refuse_output_before_the_renderer() {
    let invalid_template = "{% if %}";
    assert!(
        project(&parse(&sources()), invalid_template)
            .expect_err("syntax control")
            .downcast_ref::<gazette_adapter_tera::Error>()
            .is_some()
    );
    for (path, changed, expected_code, source, target) in [
        (
            "other/foreign.md",
            FOREIGN.replace("site:other", "site:missing"),
            "content_site_reference",
            "article:foreign",
            "site:missing",
        ),
        (
            "home/journey.md",
            JOURNEY.replace("article:z", "article:missing"),
            "content_reference",
            "journey:one",
            "article:missing",
        ),
    ] {
        let mut files = sources();
        files
            .iter_mut()
            .find(|(name, _)| *name == path)
            .expect("source")
            .1 = changed.as_bytes();
        let parsed = parse(&files);
        let error = project(&parsed, invalid_template).expect_err("no partial output");
        let Some(authored::Error::Structure {
            error: content::Error::Invalid(diagnostics),
            declarations,
        }) = error.downcast_ref::<authored::Error>()
        else {
            panic!("authored structural failure must precede Tera: {error}");
        };
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, expected_code);
        assert_eq!(diagnostics[0].source, key(source));
        assert_eq!(diagnostics[0].target, Some(key(target)));
        assert_eq!(declarations[&key(source)][0].path.as_str(), path);
        assert_eq!(
            declarations[&key(source)][0].source_sha256,
            Sha256Digest::of(changed.as_bytes())
        );
    }
}

#[test]
fn unknown_metadata_and_plain_markdown_cannot_become_partial_authored_success() {
    for (changed, has_metadata) in [
        (
            PUBLISHED.replacen("[content]", "unsupported='not silently lost'\n[content]", 1),
            true,
        ),
        ("# Plain native documentation\n".to_owned(), false),
    ] {
        let mut files = sources();
        files[1].1 = changed.as_bytes();
        let parsed = parse(&files);
        let error = project(&parsed, "{% if %}").expect_err("no admitted subset");
        let Some(authored::Error::Files(failures)) = error.downcast_ref::<authored::Error>() else {
            panic!("metadata failure must precede Tera: {error}");
        };
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].origin.path.as_str(), "home/a-current.md");
        assert_eq!(
            failures[0].origin.source_sha256,
            Sha256Digest::of(changed.as_bytes())
        );
        assert!(matches!(
            (&failures[0].error, has_metadata),
            (authored::FileError::Metadata(_), true)
                | (authored::FileError::NativeBindingRequired, false)
        ));
    }
}

#[test]
fn template_safe_does_not_bypass_retained_markdown_raw_html_policy() {
    let changed = PUBLISHED.replace("`<code>` stays text.", "<script>unsafe()</script>");
    let mut files = sources();
    files[1].1 = changed.as_bytes();
    let parsed = parse(&files);
    let error =
        project(&parsed, TEMPLATE).expect_err("raw HTML must not reach safe template insertion");
    assert!(matches!(
        error.downcast_ref::<gazette_sdk::document::Error>(),
        Some(gazette_sdk::document::Error::Markdown {
            error: gazette_sdk::markdown::Error::RawHtml(_),
            ..
        })
    ));
}
