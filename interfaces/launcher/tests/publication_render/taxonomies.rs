//! Explicit taxonomy navigation joins native membership and discovery without synthetic content.

use std::{convert::Infallible, path::Path};

use gazette_sdk::html::{self, TextKind};
use serde_json::json;

use super::{
    Fixture, OTHER, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, Value, acquire, bytes, cli, fs, render,
    source, tempdir,
};

const TEMPLATE: &[u8] = br#"<!doctype html><html><head><title>{{ title }}</title><link rel="canonical" href="{{ canonical_url }}">{% if json_ld is defined %}<script type="application/ld+json">{{ json_ld | json_script }}</script>{% endif %}</head><body data-theme="{{ presentation.theme }}"><h1>{{ title }}</h1><script type="application/json">{{ taxonomy | json_script }}</script><script type="application/json">{{ groups | json_script }}</script>{% for group in groups %}<h2>{{ group.term.label }}</h2>{% if group.canonical_url is defined %}<a data-term="{{ group.term.id }}" href="{{ group.canonical_url }}">{{ group.term.label }}</a>{% endif %}{% for page in group.pages %}<a data-page="{{ page.id }}" href="{{ page.canonical_url }}">{{ page.title }}</a>{% endfor %}{% endfor %}</body></html>"#;

fn classified(source: &str, terms: &[&str], language: &str) -> String {
    let memberships = terms
        .iter()
        .map(|term| format!("{{taxonomy='taxonomy:topic',term='{term}'}}"))
        .collect::<Vec<_>>()
        .join(",");
    source.replace(
        "[content]\n",
        &format!("[content]\nlanguage='{language}'\ntaxonomies=[{memberships}]\n"),
    )
}

fn fixture() -> Fixture {
    let one = classified(PAGE, &["term:alpha"], "en-US");
    let two = classified(
        &PAGE
            .replace("id='content:one'", "id='content:two'")
            .replace("title='One <escaped>'", "title='Second article'")
            .replace(
                "route={path='/guide/',aliases=['/old.html']}",
                "route={path='/second/'}",
            ),
        &["term:beta"],
        "hi",
    );
    let other = classified(OTHER, &["term:alpha"], "en-US");
    let hidden = classified(PROJECT_DOC, &["term:alpha", "term:beta"], "en-US");
    Fixture::new(&[
        ("one/page.md", one.as_bytes()),
        ("one/second.md", two.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("project/guide.md", hidden.as_bytes()),
        ("theme/templates/taxonomy.html", TEMPLATE),
    ])
}

fn configuration(fixture: &Fixture) -> String {
    let mut config = fixture.project_config(true).replace("discoverable=true", "discoverable=false")
        .replacen("public_base_url=", &format!("machine={{json_ld=true,sitemap=true,{}}}\ntaxonomy_pages=[{{taxonomy='taxonomy:topic',route='/topics/',template='fixture/taxonomy.html'}},{{taxonomy='taxonomy:topic',term='term:alpha',route='/topics/chosen-alpha/',template='fixture/taxonomy.html'}},{{taxonomy='taxonomy:topic',term='term:beta',route='/elsewhere-beta/',template='fixture/taxonomy.html'}}]\npublic_base_url=", super::curated_llms("collection:home", &[("Selected articles", &["content:one", "content:two"])])), 1);
    config.push_str("[[taxonomies]]\nid='taxonomy:topic'\nlabel='Topics <public>'\nterms=[{id='term:beta',label='Beta topic'},{id='term:alpha',label='Alpha topic'}]\n");
    config
}

fn scripts(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    html::inspect(bytes, Default::default(), |kind, text| {
        if matches!(kind, TextKind::JsonScript) {
            values.push(serde_json::from_str(text).expect("native JSON script"));
        }
        Ok::<_, Infallible>(())
    })
    .expect("native rendered HTML tokenization");
    values
}

#[test]
fn configured_taxonomy_index_and_terms_share_exact_members_urls_html_and_machine_outputs() {
    let fixture = fixture();
    let config = configuration(&fixture);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("configured taxonomy pages reach the real Theme");
    let tree = rendered.site().output();
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("complete shared model");
    let pages = model["taxonomy_pages"]
        .as_object()
        .expect("derived taxonomy page map");
    assert_eq!(pages.len(), 3);
    for page in pages.values() {
        assert_eq!(page["presentation"]["theme"], "theme:one");
        assert_eq!(page["presentation"]["capabilities"], json!({}));
        assert_eq!(page["presentation"]["theme_resources"], json!([]));
        assert_eq!(page["presentation"]["resources"], json!([]));
        assert!(page["presentation"].get("search").is_none());
    }
    let alpha = json!({"term":{"id":"term:alpha","label":"Alpha topic"},"canonical_url":"https://one.example/mount/topics/chosen-alpha/","pages":[{"id":"content:one","title":"One <escaped>","canonical_url":"https://one.example/mount/guide/","language":"en-US"}]});
    let beta = json!({"term":{"id":"term:beta","label":"Beta topic"},"canonical_url":"https://one.example/mount/elsewhere-beta/","pages":[{"id":"content:two","title":"Second article","canonical_url":"https://one.example/mount/second/","language":"hi"}]});
    assert_eq!(
        pages["topics/index.html"]["groups"],
        json!([alpha.clone(), beta.clone()])
    );
    assert_eq!(
        pages["topics/chosen-alpha/index.html"]["groups"],
        json!([alpha])
    );
    assert_eq!(pages["elsewhere-beta/index.html"]["groups"], json!([beta]));
    assert!(pages["topics/index.html"].get("term").is_none());
    assert_eq!(
        pages["topics/chosen-alpha/index.html"]["term"],
        json!({"id":"term:alpha","label":"Alpha topic"})
    );
    assert_eq!(
        pages["elsewhere-beta/index.html"]["term"],
        json!({"id":"term:beta","label":"Beta topic"})
    );
    assert_eq!(
        model["pages"]
            .as_object()
            .expect("native authored page map")
            .len(),
        3,
        "taxonomy navigation does not create fake content nodes"
    );
    assert!(model["pages"].get("doc:native").is_none());
    let taxonomy_text = serde_json::to_string(pages).expect("derived public model");
    for absent in [
        "doc:native",
        "content:other",
        "Native documentation",
        "source:native",
        "fixture/taxonomy.html",
    ] {
        assert!(
            !taxonomy_text.contains(absent),
            "unselected member or private template leaked: {absent}"
        );
    }
    let graph: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site JSON-LD");
    let graph_nodes = graph["@graph"]
        .as_array()
        .expect("shared linked-data nodes");
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).expect("sitemap");
    let llms = std::str::from_utf8(bytes(tree, "llms.txt")).expect("LLM navigation");
    for (path, route, title) in [
        ("topics/index.html", "topics/", "Topics <public>"),
        (
            "topics/chosen-alpha/index.html",
            "topics/chosen-alpha/",
            "Alpha topic",
        ),
        ("elsewhere-beta/index.html", "elsewhere-beta/", "Beta topic"),
    ] {
        let page = &pages[path];
        let url = format!("https://one.example/mount/{route}");
        assert_eq!(page["output"], path);
        assert_eq!(page["canonical_url"], url);
        assert_eq!(page["title"], title);
        assert_eq!(
            page["taxonomy"],
            json!({
                "id":"taxonomy:topic",
                "label":"Topics <public>",
                "destination": {
                    "url":"https://one.example/mount/topics/",
                    "canonical_url":"https://one.example/mount/topics/",
                    "destination": {
                        "kind":"internal-native",
                        "site":"site:one",
                        "content":null,
                        "presentation":{"theme":"theme:one"},
                        "relationships":[],
                    },
                },
            })
        );
        let script_values = scripts(bytes(tree, path));
        assert_eq!(
            script_values,
            vec![
                page["json_ld"].clone(),
                page["taxonomy"].clone(),
                page["groups"].clone()
            ],
            "Theme consumes the same compiler values as the machine model"
        );
        assert!(page["json_ld"].is_object());
        assert!(
            graph_nodes.contains(&page["json_ld"]),
            "site JSON-LD must retain exactly the same derived page value"
        );
        let html = std::str::from_utf8(bytes(tree, path))
            .expect("actual Theme HTML")
            .replace("&#x2F;", "/");
        assert!(
            html.contains("<body data-theme=\"theme:one\">"),
            "generated taxonomy pages receive the shared Theme presentation envelope"
        );
        assert!(html.contains(&format!("href=\"{url}\"")));
        assert!(sitemap.contains(&format!("<loc>{url}</loc>")));
        assert!(
            !llms.contains(&format!("]({url})")),
            "taxonomy generation must not expand curated discovery"
        );
    }

    let parent = tempdir().expect("fresh CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage path"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("actual CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = Path::new(receipt["directory"].as_str().expect("actual stage"));
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(directory.join(entry.path.as_str())).expect("native staged bytes"),
                tree.file(&entry.path).expect("retained output")
            );
        }
    }
    fs::write(
        fixture
            .directory
            .path()
            .join("theme/templates/taxonomy.html"),
        "not retained",
    )
    .expect("change only disposable live template after capture");
    fs::write(fixture.directory.path().join("one/page.md"), "not retained")
        .expect("change only disposable live content after capture");
    let repeated =
        render(&inputs).expect("retained sources and template repeat without native reread");
    assert_eq!(tree.inventory(), repeated.site().output().inventory());
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                tree.file(&entry.path),
                repeated.site().output().file(&entry.path)
            );
        }
    }
}

#[test]
fn taxonomy_route_collision_refuses_the_complete_output_before_staging() {
    let fixture = fixture();
    let config = configuration(&fixture).replace("route='/topics/'", "route='/guide/'");
    let inputs = acquire(&config);
    assert!(
        render(&inputs).is_err(),
        "taxonomy output cannot overwrite an authored page"
    );
    let parent = tempdir().expect("empty collision stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage path"),
    ]);
    let result = cli(&config, &arguments);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty(), "no partial successful receipt");
    assert!(
        fs::read_dir(parent.path())
            .expect("stage parent")
            .next()
            .is_none()
    );
}
