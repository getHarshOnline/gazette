//! One resolved container deck follows authored order across static HTML and native Markdown.

use super::{Fixture, HOME, MANIFEST, PAGE, Value, acquire, build, bytes, render, sites};

const FAQ: &str = "\n::::faq\n:::question {question='Where next?'}\nRead the guide.\n:::\n::::\n";
const PAGE_TEMPLATE: &str = r#"<main data-placed="{{ container_contents_placed | default(value=false) }}">{{ body_html | safe }}</main>{% if not container_contents_placed | default(value=false) %}<nav data-default>{% for child in children %}<a href="{{ child.resolved_route.url }}">{{ child.title }}</a>{% endfor %}</nav>{% endif %}"#;
const SLOT_TEMPLATE: &str = r#"<nav data-slot>{% for entry in component.container_contents.entries %}<a href="{{ entry.url }}">{{ entry.title }}</a>{% endfor %}</nav><script type="application/json">{{ component | json_script }}</script>"#;

fn fixture(body: &str, semantic: bool, leaf: bool) -> Fixture {
    fixture_with(body, semantic, leaf, &[])
}

fn fixture_with(body: &str, semantic: bool, leaf: bool, extra: &[(&str, &[u8])]) -> Fixture {
    let manifest = format!(
        "{MANIFEST}\n[components.place]\ntemplate='place.html'\n{}\n[components.again]\ntemplate='place.html'\nsemantic={{kind='container-contents'}}\n[components.wrap]\ntemplate='wrap.html'\n[components.faq]\ntemplate='faq.html'\nsemantic={{kind='faq'}}\n[components.question]\ntemplate='question.html'\nsemantic={{kind='faq-item',question_argument='question'}}\narguments={{type='object',required=['question'],additionalProperties=false,properties={{question={{type='string',minLength=1}}}}}}\n",
        if semantic {
            "semantic={kind='container-contents'}"
        } else {
            ""
        }
    ) + "
[components.catalogue]
template='catalogue.html'
semantic={kind='catalogue'}
[components.card]
template='card.html'
semantic={kind='catalogue-entry'}
";
    let home = format!("{HOME}{}", if leaf { "" } else { body });
    let page = format!(
        "{}{}",
        PAGE.replace("One <escaped>", "Guide alpha")
            .replace("lifecycle='draft'", "lifecycle='published'"),
        if leaf { body } else { "" }
    );
    let mut files: Vec<(&str, &[u8])> = vec![
        ("one/home.md", home.as_bytes()),
        ("one/page.md", page.as_bytes()),
        ("theme/theme.toml", manifest.as_bytes()),
        ("theme/templates/page.html", PAGE_TEMPLATE.as_bytes()),
        (
            "theme/templates/place.html",
            if semantic {
                SLOT_TEMPLATE.as_bytes()
            } else {
                b""
            },
        ),
        (
            "theme/templates/wrap.html",
            b"{{ component.body.html | safe }}",
        ),
        (
            "theme/templates/faq.html",
            b"<section data-faq>{{ component.body.html | safe }}</section>",
        ),
        (
            "theme/templates/question.html",
            b"<h2>{{ component.arguments.question }}</h2>{{ component.body.html | safe }}",
        ),
        (
            "theme/templates/catalogue.html",
            b"<section><h2>Catalogue</h2>{{ component.body.html | safe }}</section>",
        ),
        (
            "theme/templates/card.html",
            b"<article>{{ component.body.html | safe }}</article>",
        ),
    ];
    files.extend_from_slice(extra);
    Fixture::new(&files)
}

fn config(fixture: &Fixture) -> String {
    fixture.config().replace("public_base_url=", "machine={json_ld=true,search='lexical',markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\npublic_base_url=")
}

#[test]
fn catalogue_uses_the_visible_linked_headings_in_order() {
    let body = r#"
::::catalogue
:::card
### [Alpha](https://example.test/alpha/)

Read the [supporting note](https://example.test/note/).
:::

:::card
### [Beta](https://example.test/beta/)
:::
::::
"#;
    let fixture = fixture(body, false, false);
    let inputs = acquire(&config(&fixture));
    let rendered = render(&inputs).expect("authored catalogue");
    let output = rendered.site().output();
    let html = String::from_utf8_lossy(bytes(output, "index.html"));
    assert!(html.contains("<h2>Catalogue</h2>"));
    let model: Value = serde_json::from_slice(bytes(output, "model.json")).unwrap();
    let parts = model["pages"]["collection:home"]["json_ld"]["hasPart"]
        .as_array()
        .expect("page parts");
    let list = parts
        .iter()
        .find(|node| node["@type"] == "ItemList")
        .expect("catalogue");
    assert_eq!(
        list["itemListElement"],
        serde_json::json!([
            {"@type":"ListItem","position":1,"name":"Alpha","item":{"url":"https://example.test/alpha/"}},
            {"@type":"ListItem","position":2,"name":"Beta","item":{"url":"https://example.test/beta/"}}
        ])
    );
}

#[test]
fn slot_precedes_faq_once_without_changing_authored_search_or_faq_claims() {
    let body = format!("\n:::place\n:::\n{FAQ}");
    let placed = fixture(&body, true, false);
    let baseline = fixture(&body, false, false);
    let inputs = acquire(&config(&placed));
    let baseline_inputs = acquire(&config(&baseline));
    let rendered = render(&inputs).expect("placed collection");
    let baseline_rendered =
        render(&baseline_inputs).expect("same authored source without semantic placement");
    let tree = rendered.site().output();
    let baseline_tree = baseline_rendered.site().output();
    let html = String::from_utf8_lossy(bytes(tree, "index.html"));
    assert_eq!(html.matches("<nav data-slot>").count(), 1);
    assert!(!html.contains("data-default"));
    assert!(html.find("data-slot").unwrap() < html.find("data-faq").unwrap());
    assert!(!html.contains("reading_markdown"));
    let markdown = String::from_utf8_lossy(bytes(tree, "index.md"));
    assert_eq!(markdown.matches("## Collection contents").count(), 1);
    assert!(markdown.find("Guide alpha").unwrap() < markdown.find("Where next?").unwrap());
    assert!(!markdown.contains("<nav"));
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    let baseline_model: Value = serde_json::from_slice(bytes(baseline_tree, "model.json")).unwrap();
    let page = &model["pages"]["collection:home"];
    let baseline_page = &baseline_model["pages"]["collection:home"];
    assert!(html.contains("data-placed=\"true\""));
    assert!(page.get("container_contents_placed").is_none());
    assert!(baseline_page.get("container_contents_placed").is_none());
    assert_eq!(page["body_text"], baseline_page["body_text"]);
    assert_eq!(page["json_ld"], baseline_page["json_ld"]);
    assert_eq!(page["components"], baseline_page["components"]);
    let search: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    let baseline_search: Value =
        serde_json::from_slice(bytes(baseline_tree, "search.json")).unwrap();
    assert_eq!(search, baseline_search);
    assert!(!search.to_string().contains("container_contents"));
    let cli = super::cli(&config(&placed), &super::RENDER_ARGUMENTS);
    assert!(
        cli.status.success(),
        "{}",
        String::from_utf8_lossy(&cli.stderr)
    );
    let receipt: Value = serde_json::from_slice(&cli.stdout).unwrap();
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).unwrap()
    );
}

#[test]
fn absent_slot_retains_default_append_and_resolved_links() {
    let fixture = fixture(FAQ, true, false);
    let inputs = acquire(&config(&fixture));
    let rendered = render(&inputs).unwrap();
    let tree = rendered.site().output();
    let html = String::from_utf8_lossy(bytes(tree, "index.html"));
    assert_eq!(html.matches("<nav data-default>").count(), 1);
    assert!(!html.contains("data-slot"));
    let markdown = String::from_utf8_lossy(bytes(tree, "index.md"));
    assert!(
        markdown.find("Where next?").unwrap() < markdown.find("## Collection contents").unwrap()
    );
    assert_eq!(markdown.matches("## Collection contents").count(), 1);
    assert!(markdown.contains("https://one.example/mount/guide/"));
}

#[test]
fn invalid_slot_positions_and_arguments_fail_without_partial_output() {
    for (body, leaf) in [
        ("\n:::place\n:::\n\n:::again\n:::\n", false),
        ("\n:::place {extra=true}\n:::\n", false),
        ("\n:::place\nAuthored body.\n:::\n", false),
        ("\n::::wrap\n:::place\n:::\n::::\n", false),
        ("\n> :::place\n> :::\n", false),
        ("\n- :::place\n  :::\n", false),
        ("\n:::place\n:::\n", true),
    ] {
        let fixture = fixture(body, true, leaf);
        let inputs = acquire(&config(&fixture));
        assert!(render(&inputs).is_err(), "admitted {body:?}, leaf={leaf}");
    }
}

#[test]
fn slot_uses_the_selected_public_or_preview_destinations() {
    let fixture = fixture("\n:::place\n:::\n", true, false);
    let config = config(&fixture);
    let inputs = sites::acquire_set(&config, &["site:one"]);
    let page = "page.html".parse().unwrap();
    let alias = "alias.html".parse().unwrap();
    let model = "model.json".parse().unwrap();
    let site: gazette_sdk::workspace::Key = "site:one".parse().unwrap();
    for preview in [false, true] {
        let mut options = sites::options(&["site:one"], &page, &alias, &model);
        if preview {
            options.preview_urls = Some(
                [(
                    site.clone(),
                    "http://127.0.0.1:8001/mount/".parse().unwrap(),
                )]
                .into_iter()
                .collect(),
            );
        }
        let rendered = build::render_publications(&inputs, options, Default::default()).unwrap();
        let tree = rendered.sites()[&site].as_ref().unwrap().site().output();
        let expected = if preview {
            "http://127.0.0.1:8001/mount/guide/"
        } else {
            "https://one.example/mount/guide/"
        };
        let html = String::from_utf8_lossy(bytes(tree, "index.html")).replace("&#x2F;", "/");
        assert!(html.contains(&format!("href=\"{expected}\"")), "{html}");
        let markdown = String::from_utf8_lossy(bytes(tree, "index.md"));
        assert!(markdown.contains(expected), "{markdown}");
    }
}

#[test]
fn direct_journey_steps_do_not_widen_discovery_markdown_or_search() {
    let journey = "+++\n[content]\nid='journey:explicit'\nsite='site:one'\ntitle='Explicit traversal'\nlifecycle='published'\nroute={path='/journey/'}\nstructure={kind='journey',collection='collection:home',members=[{content='doc:native',primary=true},{content='content:one',primary=true}]}\n+++\nBefore.\n\n:::place\n:::\n\nAfter.\n";
    let doc = super::PROJECT_DOC.replace("collections=['collection:home']\n", "");
    let page = PAGE
        .replace("collections=['collection:home']\n", "")
        .replace("One <escaped>", "Guide alpha")
        .replace("lifecycle='draft'", "lifecycle='published'");
    let fixture = fixture_with(
        "",
        true,
        false,
        &[
            ("project/guide.md", doc.as_bytes()),
            ("one/page.md", page.as_bytes()),
            ("one/journey.md", journey.as_bytes()),
        ],
    );
    for discoverable in [false, true] {
        let config = fixture.project_config(true).replace("discoverable=true", &format!("discoverable={discoverable}")).replace("public_base_url=", "machine={search='lexical',markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\npublic_base_url=");
        let inputs = acquire(&config);
        let rendered = render(&inputs).unwrap();
        let tree = rendered.site().output();
        let html = String::from_utf8_lossy(bytes(tree, "journey/index.html"));
        assert!(html.contains("Native documentation"));
        assert!(html.contains("Guide alpha"));
        let markdown = String::from_utf8_lossy(bytes(tree, "journey/index.md"));
        assert_eq!(markdown.contains("Journey sequence"), discoverable);
        assert_eq!(markdown.contains("Native documentation"), discoverable);
        assert_eq!(markdown.contains("Guide alpha"), discoverable);
        if discoverable {
            assert!(
                markdown.find("Native documentation").unwrap()
                    < markdown.find("Guide alpha").unwrap()
            );
            assert!(markdown.find("Guide alpha").unwrap() < markdown.find("After.").unwrap());
            assert_eq!(markdown.matches("Journey sequence").count(), 1);
        }
        let search = String::from_utf8_lossy(bytes(tree, "search.json"));
        assert_eq!(search.contains("Native documentation"), discoverable);
        assert!(!search.contains("container_contents"));
    }
}

#[test]
fn documentation_groups_keep_the_existing_direct_child_order() {
    let home = format!("{}\n:::place\n:::\n", HOME.replace("kind='collection'", "kind='collection',purpose='documentation',navigation=[{group='Read first',pages=[{content='content:one'}]}]"));
    let fixture = fixture_with("", true, false, &[("one/home.md", home.as_bytes())]);
    let inputs = acquire(&config(&fixture));
    let rendered = render(&inputs).unwrap();
    let html = String::from_utf8_lossy(bytes(rendered.site().output(), "index.html"));
    let start = html.find("<script type=\"application/json\">").unwrap()
        + "<script type=\"application/json\">".len();
    let end = start + html[start..].find("</script>").unwrap();
    let component: Value = serde_json::from_str(&html[start..end]).unwrap();
    let view = &component["container_contents"];
    assert_eq!(view["documentation"], true);
    assert_eq!(view["entries"][0]["id"], "content:one");
    assert_eq!(
        view["outline"],
        serde_json::json!([
            {"kind":"group-open","title":"Read first"},
            {"kind":"entry","content":"content:one"},
            {"kind":"group-close"},
        ])
    );
    assert!(component.get("reading_markdown").is_none());
}

#[test]
fn readable_contents_follow_authored_multi_child_order_with_or_without_groups_and_slot() {
    let one = PAGE.replace("One <escaped>", "Zulu guide");
    let two = one
        .replace("content:one", "content:two")
        .replace("Zulu guide", "Alpha guide")
        .replace("/guide/", "/alpha/")
        .replace("/old.html", "/alpha-old.html");
    let three = one
        .replace("content:one", "content:three")
        .replace("Zulu guide", "Middle guide")
        .replace("/guide/", "/middle/")
        .replace("/old.html", "/middle-old.html");
    for grouped in [false, true] {
        let navigation = if grouped {
            "[{group='Read first',pages=[{content='content:one'},{group='Choose next',pages=[{content='content:three'}]}]},{group='Then continue',pages=[{content='content:two'}]}]"
        } else {
            "[{content='content:one'},{content='content:three'},{content='content:two'}]"
        };
        for placed in [false, true] {
            let home = format!(
                "{}\n{}{FAQ}",
                HOME.replace(
                    "kind='collection'",
                    &format!("kind='collection',purpose='documentation',navigation={navigation}")
                ),
                if placed { ":::place\n:::\n" } else { "" }
            );
            let fixture = fixture_with(
                "",
                true,
                false,
                &[
                    ("one/home.md", home.as_bytes()),
                    ("one/page.md", one.as_bytes()),
                    ("one/alpha.md", two.as_bytes()),
                    ("one/middle.md", three.as_bytes()),
                ],
            );
            let inputs = acquire(&config(&fixture));
            let rendered = render(&inputs).expect("resolved authored documentation order");
            let tree = rendered.site().output();
            let markdown = String::from_utf8_lossy(bytes(tree, "index.md"));
            let expected = [
                "[Zulu guide](https://one.example/mount/guide/)",
                "[Middle guide](https://one.example/mount/middle/)",
                "[Alpha guide](https://one.example/mount/alpha/)",
            ];
            let positions = expected.map(|entry| {
                assert_eq!(markdown.matches(entry).count(), 1, "{markdown}");
                markdown.find(entry).unwrap()
            });
            assert!(
                positions.windows(2).all(|pair| pair[0] < pair[1]),
                "{markdown}"
            );
            assert_eq!(markdown.matches("## Collection contents").count(), 1);
            assert_eq!(positions[2] < markdown.find("Where next?").unwrap(), placed);
            for label in ["Read first", "Choose next", "Then continue"] {
                assert_eq!(
                    markdown.matches(label).count(),
                    usize::from(grouped),
                    "{markdown}"
                );
            }
            if grouped {
                assert!(markdown.find("Read first").unwrap() < positions[0]);
                assert!(positions[0] < markdown.find("Choose next").unwrap());
                assert!(markdown.find("Choose next").unwrap() < positions[1]);
                assert!(positions[1] < markdown.find("Then continue").unwrap());
                assert!(markdown.find("Then continue").unwrap() < positions[2]);
                assert!(
                    markdown
                        .lines()
                        .any(|line| line.starts_with("    - [Middle guide]")),
                    "nested group stays nested: {markdown}"
                );
            }
            if placed {
                let html = String::from_utf8_lossy(bytes(tree, "index.html"));
                let start = html.find("<script type=\"application/json\">").unwrap()
                    + "<script type=\"application/json\">".len();
                let end = start + html[start..].find("</script>").unwrap();
                let component: Value = serde_json::from_str(&html[start..end]).unwrap();
                let ids: Vec<_> = component["container_contents"]["outline"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|event| event.get("content").and_then(Value::as_str))
                    .collect();
                assert_eq!(ids, ["content:one", "content:three", "content:two"]);
            }
        }
    }
}

#[test]
fn readable_documentation_groups_omit_hidden_only_labels_for_slot_and_default() {
    for placed in [false, true] {
        let home = format!("{}\n{}", HOME.replace("kind='collection'", "kind='collection',purpose='documentation',navigation=[{group='Hidden group',pages=[{content='doc:native'}]},{group='Visible group',pages=[{content='content:one'}]}]"), if placed { ":::place\n:::\n" } else { "" });
        let fixture = fixture_with(
            "",
            true,
            false,
            &[
                ("one/home.md", home.as_bytes()),
                ("project/guide.md", super::PROJECT_DOC.as_bytes()),
            ],
        );
        let config = fixture.project_config(true).replace("discoverable=true", "discoverable=false").replace("public_base_url=", "machine={markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}\npublic_base_url=");
        let inputs = acquire(&config);
        let rendered = render(&inputs).unwrap();
        let markdown = String::from_utf8_lossy(bytes(rendered.site().output(), "index.md"));
        assert!(!markdown.contains("Hidden group"));
        assert!(!markdown.contains("Native documentation"));
        assert_eq!(markdown.matches("Visible group").count(), 1);
        assert_eq!(markdown.matches("Guide alpha").count(), 1);
        assert!(markdown.find("Visible group").unwrap() < markdown.find("Guide alpha").unwrap());
    }
}
