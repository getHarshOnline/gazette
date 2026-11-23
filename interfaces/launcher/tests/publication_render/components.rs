//! Static components compose captured Theme presentation with one resolved Markdown body.

use super::*;
use gazette_sdk::{authored, publication::content::Disposition, workspace::Key};
use serde_json::json;

pub(super) const MANIFEST: &str = "package='theme:one'\nnamespace='fixture'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html'}\n[components.callout]\ntemplate='components/callout.html'\narguments={type='object',required=['tone'],additionalProperties=false,properties={tone={type='string',enum=['note','warning']},label={type='string'}}}\n";
pub(super) const COMPONENT: &str = "<aside class=\"callout {{ component.arguments.tone }}\" data-name=\"{{ component.name }}\" data-excerpt=\"{{ component.excerpt }}\"{% if component.arguments.label is defined %} data-label=\"{{ component.arguments.label }}\"{% endif %}>{{ component.body.html | safe }}</aside>";
const PAGE_TEMPLATE: &str = "{% if summary is defined %}<section id=\"excerpt\">{{ summary.html | safe }}</section>{% endif %}<main>{{ body_html | safe }}</main>";
// Authored links use public URLs; source-relative Markdown routing is native-doc behavior.
const BODY: &str = "# Opening 🦀\n\n::::callout {tone = \"note\", label = \"&amp; 🦀\"}\n## Inside {#inside}\n\nLead **strong** [Other](https://other.example/elsewhere/#other-body), [here](#inside) and reference[^note].\n\n:::callout {tone = \"warning\"}\nNested body.\n:::\n\n<!-- more -->\n\nRemaining body inside outer component.\n::::\n\n```markdown\n:::unknown {tone = \"literal\"}\nLiteral example, not an invocation.\n:::\n```\n\n[^note]: Retained footnote outside excerpt.\n";

fn fixture(body: &str, manifest: &str) -> Fixture {
    let manifest = manifest.replace(
        "render={page='page.html'}",
        "render={page='page.html',alias='page.html'}",
    );
    let page = PAGE
        .replace("**Retained** body\n", body)
        .replace("lifecycle='draft'", "lifecycle='published'");
    Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/theme.toml", manifest.as_bytes()),
        (
            "theme/templates/components/callout.html",
            COMPONENT.as_bytes(),
        ),
        ("theme/templates/page.html", PAGE_TEMPLATE.as_bytes()),
    ])
}

fn configured(fixture: &Fixture) -> String {
    fixture.config().replacen("public_base_url=", "machine={json_ld=true,search='lexical',feeds={identity_base='https://identity.example/',channels=[{id='feed:json',channel='collection:home',path='/feed.json',format='json',scope={kind='site'}}]}}\npublic_base_url=", 1)
}

fn staged(config: &str, parent: &std::path::Path) -> Output {
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.to_str().expect("owned stage parent"),
    ]);
    cli(config, &arguments)
}

fn assert_no_stage(parent: &std::path::Path, output: &Output) {
    assert!(
        !output.status.success(),
        "refused invocation reported success"
    );
    assert!(output.stdout.is_empty(), "no partial successful receipt");
    assert_eq!(fs::read_dir(parent).expect("stage contents").count(), 1);
    assert_eq!(
        fs::read(parent.join("keep")).expect("unrelated sentinel"),
        b"untouched"
    );
}

#[test]
fn nested_components_share_body_excerpt_machine_outputs_and_retained_native_stage() {
    let body = BODY.replace('\n', "\r\n");
    let fixture = fixture(&body, MANIFEST);
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete static component publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    let page = &model["pages"]["content:one"];
    assert_eq!(
        page["components"],
        json!([
            {
                "context":{
                    "address":"content:one#component-1",
                    "occurrence":1,
                    "host":{
                        "content":"content:one",
                        "site":"site:one",
                        "surface":{"kind":"page","family":{"kind":"article"}}
                    }
                },
                "name":"callout",
                "arguments":{"tone":"note","label":"&amp; 🦀"}
            },
            {
                "context":{
                    "address":"content:one#component-2",
                    "occurrence":2,
                    "host":{
                        "content":"content:one",
                        "site":"site:one",
                        "surface":{"kind":"page","family":{"kind":"article"}}
                    }
                },
                "name":"callout",
                "arguments":{"tone":"warning"}
            }
        ]),
        "ordered authored arguments retain typed public host context without private source coordinates"
    );
    let html = page["body_html"].as_str().expect("component body HTML");
    let excerpt = page["summary"]["html"].as_str().expect("component excerpt");
    assert!(html.contains("class=\"callout note\""));
    assert!(html.contains("class=\"callout warning\""));
    assert_eq!(
        html.matches("<aside ").count(),
        2,
        "literal fence does not invoke a component"
    );
    assert!(html.contains("data-name=\"callout\" data-excerpt=\"false\""));
    assert!(
        html.contains("data-label=\"&amp;amp; 🦀\""),
        "header values retain original entity text: {html}"
    );
    let (_, code) = html
        .split_once("<code class=\"language-markdown\">")
        .expect("literal Markdown code element");
    let (code, _) = code.split_once("</code>").expect("closed code element");
    let mut literal = String::new();
    gazette_sdk::html::inspect(code.as_bytes(), Default::default(), |kind, value| {
        if kind == gazette_sdk::html::TextKind::Text {
            literal.push_str(value);
        }
        Ok::<_, std::convert::Infallible>(())
    })
    .expect("literal code text through syntax spans");
    assert_eq!(
        literal,
        ":::unknown {tone = \"literal\"}\nLiteral example, not an invocation.\n:::\n"
    );
    assert!(html.contains("id=\"inside\""));
    assert!(excerpt.contains("<h2>Inside</h2>"));
    assert!(
        !excerpt.contains("id=\"inside\""),
        "excerpt does not duplicate heading IDs"
    );
    for fragment in [
        "https://other.example/elsewhere/#other-body",
        "https://one.example/mount/guide/#inside",
        "Nested body.",
    ] {
        assert!(html.contains(fragment), "body lost {fragment}: {html}");
        assert!(
            excerpt.contains(fragment),
            "excerpt lost {fragment}: {excerpt}"
        );
    }
    assert_eq!(excerpt.matches("<aside ").count(), 2);
    assert!(excerpt.contains("data-excerpt=\"true\""));
    assert!(!excerpt.contains("Remaining body"));
    assert!(!excerpt.contains("Retained footnote"));
    assert!(html.contains("Remaining body inside outer component."));
    assert!(html.contains("Retained footnote outside excerpt."));
    let text = page["body_text"].as_str().expect("authored plain text");
    assert!(text.contains("Lead strong Other, here and reference"));
    assert!(text.contains("Nested body."));
    assert!(!text.contains("<!-- more -->"));
    let summary = &page["summary"];
    let start = summary["marker"]["start"].as_u64().expect("source start") as usize;
    let end = summary["marker"]["end"].as_u64().expect("source end") as usize;
    assert_eq!(
        body[start..end].trim(),
        "<!-- more -->",
        "CRLF and non-BMP offsets bind original bytes"
    );
    assert_eq!(page["json_ld"]["abstract"], summary["text"]);
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|value| value["content"]["id"] == "content:one")
        .expect("search page");
    assert_eq!(record["body"], page["body_text"]);
    assert_eq!(record["components"], page["components"]);
    assert_eq!(record["summary"], summary["text"]);
    let feed: Value = serde_json::from_slice(bytes(tree, "feed.json")).expect("JSON Feed");
    assert_eq!(feed["items"][0]["content_html"], page["body_html"]);
    assert_eq!(feed["items"][0]["content_text"], page["body_text"]);
    assert_eq!(feed["items"][0]["summary"], summary["text"]);
    let page_html = std::str::from_utf8(bytes(tree, "guide/index.html")).expect("Theme page");
    assert!(page_html.contains(&format!("<main>{html}</main>")));
    assert!(page_html.contains(&format!("<section id=\"excerpt\">{excerpt}</section>")));
    let parent = tempdir().expect("fresh stage parent");
    let native = staged(&config, parent.path());
    assert!(
        native.status.success(),
        "{}",
        String::from_utf8_lossy(&native.stderr)
    );
    let receipt: Value = serde_json::from_slice(&native.stdout).expect("CLI receipt");
    assert_eq!(receipt["inventory"], json!(tree.inventory()));
    let stage = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for entry in tree.inventory().entries() {
        if let Some(expected) = tree.file(&entry.path) {
            assert_eq!(
                fs::read(stage.join(entry.path.as_str())).expect("native staged bytes"),
                expected
            );
        }
    }
    fixture
        .directory
        .close()
        .expect("remove only owned source fixture");
    let replay = render(&inputs).expect("retained components never reopen native Theme or source");
    assert_eq!(
        replay.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        bytes(replay.site().output(), "model.json"),
        bytes(tree, "model.json")
    );
}

#[test]
fn unknown_and_invalid_component_arguments_refuse_complete_site_without_staging() {
    for header in [
        "unknown {tone = \"note\"}",
        "callout {tone = }",
        "callout {tone = \"note\", extra = 1}",
        "callout {tone = 7}",
        "callout {tone = \"unsupported\"}",
        "callout {}",
        "callout {tone = \"note\", tone = \"warning\"}",
    ] {
        let body = format!("Valid prefix.\n\n<!-- more -->\n\n:::{header}\nLate content.\n:::\n");
        let fixture = fixture(&body, MANIFEST);
        let config = configured(&fixture);
        let inputs = acquire(&config);
        let error = render(&inputs).expect_err("excerpt must not hide invalid component");
        assert_eq!(
            error.code(),
            "markdown_adapter",
            "must reach authored argument admission: {error}"
        );
        let parent = tempdir().expect("preserved stage parent");
        fs::write(parent.path().join("keep"), b"untouched").expect("sentinel");
        let output = staged(&config, parent.path());
        assert_no_stage(parent.path(), &output);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(error.code()),
            "SDK/CLI refusal disagreed for {header}"
        );
    }
}

#[test]
fn missing_component_template_and_exhausted_projection_bounds_never_yield_partial_output() {
    let missing = MANIFEST.replace("components/callout.html", "components/missing.html");
    let fixture = fixture(":::callout {tone='note'}\nRetained body.\n:::\n", &missing);
    let config = configured(&fixture);
    assert_eq!(
        render(&acquire(&config))
            .expect_err("selected missing component template")
            .code(),
        "template_binding"
    );
    let parent = tempdir().expect("preserved stage parent");
    fs::write(parent.path().join("keep"), b"untouched").expect("sentinel");
    assert_no_stage(parent.path(), &staged(&config, parent.path()));

    let fixture = self::fixture(BODY, MANIFEST);
    let config = configured(&fixture);
    let inputs = acquire(&config);
    for exhaust_output in [false, true] {
        let mut limits: build::PublicationRenderLimits = Default::default();
        if exhaust_output {
            limits.output.render.output_bytes = 1;
        } else {
            limits.corpus.document.markdown.nodes = 1;
        }
        assert!(
            build::render_publication(
                &inputs,
                build::SiteRenderOptions {
                    render_year: None,
                    graph: None,
                    model_output: Some(&"model.json".parse().expect("model")),
                    limits: Default::default(),
                },
                limits
            )
            .is_err(),
            "bounded component path produced partial success"
        );
    }
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--max-file-bytes",
        "1",
        "--output-parent",
        parent.path().to_str().expect("parent"),
    ]);
    assert_no_stage(parent.path(), &cli(&config, &arguments));
}

#[test]
fn private_review_and_selected_candidate_share_component_authored_meaning() {
    let fixture = fixture(BODY, MANIFEST);
    let config = configured(&fixture);
    let inputs = acquire(&config);
    let parsed =
        build::parse_publication(&inputs, Default::default()).expect("retained component syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("publication graph");
    let private = build::review(&resolved, Default::default())
        .expect("private review admits selected Theme component contracts");
    let selection = authored::Selection {
        content: resolved
            .authored()
            .content()
            .graph()
            .nodes()
            .map(|node| (node.id.clone(), Disposition::Include))
            .collect(),
        assets: Default::default(),
    };
    let digest =
        source::Sha256Digest::of(&serde_json::to_vec(&selection).expect("exact candidate intent"));
    let selected = resolved
        .into_selected(selection)
        .expect("complete non-draft candidate");
    let review = build::review_selected(&selected, Default::default())
        .expect("selected review renders component meaning");
    assert_eq!(review.request().selection_sha256(), Some(digest));
    assert_eq!(private.request().selection_sha256(), None);
    let rendered = build::render_selected_publication(
        &selected,
        build::SiteRenderOptions {
            render_year: None,
            model_output: Some(&"model.json".parse().expect("model")),
            graph: None,
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("selected component output");
    let model: Value = serde_json::from_slice(bytes(rendered.site().output(), "model.json"))
        .expect("selected model");
    let key: Key = "content:one".parse().expect("page key");
    let page = review.request().surfaces().iter().find(|surface| matches!(surface.target(), build::ReviewTarget::Page {content} if content == &key)).expect("private selected page").value();
    assert_eq!(
        page["components"],
        model["pages"]["content:one"]["components"]
    );
    assert_eq!(
        page["components"]
            .as_array()
            .expect("validated occurrences")
            .len(),
        2
    );
    assert_eq!(page["components"][0]["arguments"]["label"], "&amp; 🦀");
    let unselected = private.request().surfaces().iter().find(|surface| matches!(surface.target(), build::ReviewTarget::Page {content} if content == &key)).expect("private unselected page").value();
    assert_eq!(unselected["components"], page["components"]);
    assert_eq!(
        page["body_text"],
        model["pages"]["content:one"]["body_text"]
    );
    assert_eq!(
        page["summary"]["text"],
        model["pages"]["content:one"]["summary"]["text"]
    );
    assert!(
        !page["body_text"]
            .as_str()
            .expect("private text")
            .contains("<aside")
    );
}
