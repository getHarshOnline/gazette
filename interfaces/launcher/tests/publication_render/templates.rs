//! Explicit page presentation selects captured templates without changing content meaning.

use super::*;

fn selected(source: &str, template: &str) -> String {
    source.replacen("+++\n", &format!("+++\ntemplate='{template}'\n"), 1)
}

#[test]
fn theme_kind_defaults_render_until_an_authored_page_overrides_them() {
    let manifest = MANIFEST.replace(
        "graph='graph.html'",
        "graph='graph.html',kinds={article='reading.html',collection='index.html'}",
    );
    for own in [false, true] {
        let page = if own {
            selected(PAGE, "fixture/own.html")
        } else {
            PAGE.into()
        };
        let fixture = Fixture::new(&[
            ("theme/theme.toml", manifest.as_bytes()),
            ("theme/templates/reading.html", b"reading {{ content.id }}"),
            ("theme/templates/index.html", b"index {{ content.id }}"),
            ("theme/templates/own.html", b"own {{ content.id }}"),
            ("one/page.md", page.as_bytes()),
        ]);
        let config = fixture.config();
        let inputs = acquire(&config);
        let rendered = render(&inputs).unwrap();
        assert_eq!(
            bytes(rendered.site().output(), "index.html"),
            b"index collection:home"
        );
        assert_eq!(
            bytes(rendered.site().output(), "guide/index.html"),
            if own {
                b"own content:one".as_slice()
            } else {
                b"reading content:one".as_slice()
            }
        );
    }
}

#[test]
fn publication_relationships_select_home_and_collection_directory_templates() {
    let manifest = MANIFEST.replace(
        "graph='graph.html'",
        "graph='graph.html',home='home.html',collection_directory='directory.html',kinds={index='index.html'}",
    );
    let catalogue = "+++\n[content]\nid='content:catalogue'\nsite='site:one'\ntitle='Collections'\nlifecycle='published'\nroute={path='/collections/'}\nstructure={kind='page',family={kind='index'}}\n+++\n";
    for own in [false, true] {
        let home = if own {
            selected(HOME, "fixture/own.html")
        } else {
            HOME.into()
        };
        let fixture = Fixture::new(&[
            ("theme/theme.toml", manifest.as_bytes()),
            ("theme/templates/home.html", b"home {{ content.id }}"),
            (
                "theme/templates/directory.html",
                b"directory {{ content.id }}",
            ),
            ("theme/templates/index.html", b"index {{ content.id }}"),
            ("theme/templates/own.html", b"own {{ content.id }}"),
            ("one/home.md", home.as_bytes()),
            ("one/catalogue.md", catalogue.as_bytes()),
        ]);
        let config = fixture.config().replacen("public_base_url=", "home='collection:home'\nlistings=[{content='content:catalogue',select={kind='root-collections'},order={first=[],remaining='title-then-id'},pagination={page_size=15,next_pages='page/{number}/'}}]\npublic_base_url=", 1);
        let inputs = acquire(&config);
        let rendered = render(&inputs).unwrap();
        assert_eq!(
            bytes(rendered.site().output(), "index.html"),
            if own {
                b"own collection:home".as_slice()
            } else {
                b"home collection:home".as_slice()
            }
        );
        assert_eq!(
            bytes(rendered.site().output(), "collections/index.html"),
            b"directory content:catalogue"
        );
    }
}

#[test]
fn authored_article_collection_and_default_templates_share_sdk_cli_and_retained_output() {
    let page = selected(PAGE, "fixture/article.html");
    let home = selected(HOME, "fixture/collections/index.html");
    let default = PAGE
        .replace("content:one", "content:default")
        .replace("path='/guide/',aliases=['/old.html']", "path='/default/'");
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/home.md", home.as_bytes()),
        ("one/default.md", default.as_bytes()),
        ("theme/templates/article.html", b"<article data-template=\"article\">{{ content.id }}{{ body_html | safe }}</article>"),
        ("theme/templates/collections/index.html", b"<section data-template=\"collection\">{{ content.id }}{% for child in children %}<b>{{ child.id }}</b>{% endfor %}{{ body_html | safe }}</section>"),
    ]);
    let config = fixture.config().replace(
        "public_base_url=",
        "machine={search='lexical',json_ld=true}\npublic_base_url=",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("one build with explicit per-page selections");
    let tree = rendered.site().output();
    let evidence = serde_json::to_value(rendered.site().evidence()).unwrap();
    for (id, path, template, marker) in [
        (
            "content:one",
            "guide/index.html",
            "fixture/article.html",
            "data-template=\"article\"",
        ),
        (
            "collection:home",
            "index.html",
            "fixture/collections/index.html",
            "data-template=\"collection\"",
        ),
        (
            "content:default",
            "default/index.html",
            "fixture/page.html",
            "<h1>One &lt;escaped&gt;</h1>",
        ),
    ] {
        assert_eq!(evidence["pages"][id]["template"], template);
        assert!(
            std::str::from_utf8(bytes(tree, path))
                .unwrap()
                .contains(marker),
            "{path}"
        );
    }
    let alias = std::str::from_utf8(bytes(tree, "old.html")).unwrap();
    assert!(
        alias.contains("http-equiv=\"refresh\""),
        "independent alias template still used"
    );
    assert!(!alias.contains("data-template="));
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    for id in ["content:one", "content:default", "collection:home"] {
        assert!(model["pages"][id]["content"].get("template").is_none());
    }
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    for record in &search {
        let id = record["content"]["id"].as_str().unwrap();
        assert_eq!(record["content"], model["pages"][id]["content"]);
        assert!(record.get("template").is_none());
    }
    let stage = tempdir().unwrap();
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", stage.path().to_str().unwrap()]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).unwrap()
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().unwrap());
    for path in [
        "guide/index.html",
        "index.html",
        "default/index.html",
        "old.html",
        "model.json",
        "search.json",
    ] {
        assert_eq!(fs::read(directory.join(path)).unwrap(), bytes(tree, path));
    }
    fs::remove_dir_all(fixture.directory.path().join("theme")).unwrap();
    fs::remove_dir_all(fixture.directory.path().join("one")).unwrap();
    let repeated = render(&inputs).expect("exact retained selectors and templates");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
}

#[test]
fn missing_or_escaping_authored_template_refuses_without_a_partial_stage() {
    for template in ["absent.html", "../outside.html", "/absolute.html"] {
        let page = selected(PAGE, template);
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let stage = tempdir().unwrap();
        fs::write(stage.path().join("keep"), b"unrelated").unwrap();
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", stage.path().to_str().unwrap()]);
        let output = cli(&fixture.config(), &arguments);
        assert!(!output.status.success(), "{template} must refuse");
        assert!(output.stdout.is_empty(), "no partial success receipt");
        assert!(!output.stderr.is_empty(), "explicit native error");
        assert_eq!(fs::read_dir(stage.path()).unwrap().count(), 1);
        assert_eq!(fs::read(stage.path().join("keep")).unwrap(), b"unrelated");
    }
}
