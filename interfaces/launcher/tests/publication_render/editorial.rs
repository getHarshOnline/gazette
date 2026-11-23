//! Explicit editorial facts cross real local outputs without summary or clock inference.

use std::convert::Infallible;

use gazette_sdk::html::{self, TextKind};
use serde_json::{Value, json};

use super::{
    Fixture, HOME, OTHER, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs,
    native_configuration, page_at, render, source, tempdir,
};

const TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Editorial fixture</title><script type=\"application/ld+json\">{{ json_ld | json_script }}</script></head><body>{% if content.description is defined %}<p>{{ content.description }}</p>{% endif %}{{ body_html | safe }}</body></html>";
const GRAPH_TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Graph fixture</title></head><body><script type=\"application/json\">{{ nodes | json_script }}</script></body></html>";

fn enable(config: &str) -> String {
    config.replace(
        "public_base_url=",
        "machine={json_ld=true,search='lexical'}\npublic_base_url=",
    )
}

fn decoded(bytes: &[u8]) -> (Vec<Value>, Vec<String>) {
    let mut scripts = Vec::new();
    let mut text = Vec::new();
    html::inspect(bytes, html::Limits::default(), |kind, value| {
        match kind {
            TextKind::JsonScript => scripts.push(serde_json::from_str(value).expect("JSON script")),
            TextKind::Text => text.push(value.to_owned()),
            TextKind::Script | TextKind::ModuleScript | TextKind::DataScript => {
                panic!("editorial text created unexpected script")
            }
            _ => (),
        }
        Ok::<_, Infallible>(())
    })
    .expect("complete native HTML tokenization");
    (scripts, text)
}

#[test]
fn explicit_editorial_facts_are_shared_by_cli_sdk_search_graph_and_json_ld() {
    let description =
        "Plain *not emphasis* <em>not HTML</em> & </script><script>not executable</script>";
    let page = PAGE.replace(
        "description='The retained local article.'\n",
        &format!(
            "description={}\npublished='2035-06-07'\nmodified='2020-01-02T03:04:05.120000000+00:00'\n",
            serde_json::to_string(description).expect("TOML string")
        ),
    );
    let other = OTHER.replace(
        "description='The independently selected documentation page.'\n",
        "description='Foreign editorial fact'\npublished='2019-01-01'\n",
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
        ("theme/templates/graph.html", GRAPH_TEMPLATE),
    ]);
    let config = enable(&fixture.config());
    let inputs = acquire(&config);
    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            model_output: Some(&"model.json".parse().expect("model output")),
            graph: Some(build::GraphRenderOptions {
                output: &"graph.html".parse().expect("graph output"),
                limits: Default::default(),
            }),
            limits: source::Limits::default(),
        },
        build::PublicationRenderLimits::default(),
    )
    .expect("explicit facts do not impose chronology or lifecycle rules");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    let page = &model["pages"]["content:one"];
    let content = &page["content"];
    assert_eq!(content["description"], description);
    assert_eq!(content["published"], "2035-06-07");
    assert_eq!(content["modified"], "2020-01-02T03:04:05.120Z");
    assert_eq!(
        content["lifecycle"], "draft",
        "dates are not publication approval"
    );
    assert_eq!(model["graph"]["nodes"]["content:one"], *content);
    assert!(
        model["graph"]["references"]["content:other"]
            .get("description")
            .is_none()
    );
    assert_eq!(
        decoded(bytes(tree, "graph.html")).0,
        [model["graph"]["nodes"].clone()]
    );

    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|record| record["content"]["id"] == "content:one")
        .expect("exact local search document");
    assert_eq!(record["content"], *content);
    assert_eq!(record["body"], page["body_text"]);
    assert!(
        !record["body"]
            .as_str()
            .expect("body text")
            .contains(description)
    );

    let ld = &page["json_ld"];
    assert_eq!(ld["description"], description);
    assert_eq!(
        ld["datePublished"],
        json!({"@value":"2035-06-07","@type":"http://www.w3.org/2001/XMLSchema#date"})
    );
    assert_eq!(
        ld["dateModified"],
        json!({"@value":"2020-01-02T03:04:05.120Z","@type":"http://www.w3.org/2001/XMLSchema#dateTime"})
    );
    assert!(
        !serde_json::to_string(ld)
            .expect("LD text")
            .contains("Foreign editorial fact")
    );
    let site_ld: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site LD");
    let site_page = site_ld["@graph"]
        .as_array()
        .expect("LD pages")
        .iter()
        .find(|node| node["identifier"] == "content:one")
        .expect("local LD page");
    assert_eq!(site_page, ld);
    let (scripts, text) = decoded(bytes(tree, "guide/index.html"));
    assert_eq!(scripts, std::slice::from_ref(ld));
    assert!(text.iter().any(|value| value == description));

    let parent = tempdir().expect("stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--graph-output",
        "graph.html",
        "--output-parent",
        parent.path().to_str().expect("parent"),
    ]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("inventory")
    );
    let stage = std::path::Path::new(receipt["directory"].as_str().expect("stage"));
    for path in [
        "model.json",
        "site.jsonld",
        "search.json",
        "guide/index.html",
        "graph.html",
    ] {
        assert_eq!(
            fs::read(stage.join(path)).expect("staged bytes"),
            bytes(tree, path)
        );
    }
}

#[test]
fn editorial_absence_stays_absent_in_plain_native_docs_and_independent_date_fields() {
    let dated = PROJECT_DOC.replace("[content]\n", "[content]\npublished='2024-02-29'\n");
    let home = HOME.replace(
        "description='The explicit fixture collection and its retained articles.'\n",
        "",
    );
    let modified = PAGE
        .replace("description='The retained local article.'\n", "")
        .replace(
            "[content]\n",
            "[content]\nmodified='2021-02-03T04:05:06+05:30'\n",
        );
    let fixture = Fixture::new(&[
        ("one/home.md", home.as_bytes()),
        ("one/page.md", modified.as_bytes()),
        ("project/framed.md", dated.as_bytes()),
        (
            "project/2025-09-01.md",
            b"# A native title\n\nA body is not a description.\n",
        ),
        ("theme/templates/page.html", TEMPLATE),
    ]);
    let inputs = acquire(&enable(&native_configuration(&fixture)));
    let rendered = render(&inputs).expect("native and explicit editorial values coexist");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
    let native = page_at(&model, "https://one.example/mount/docs/library/2025-09-01/");
    for page in [native, &model["pages"]["collection:home"]] {
        for field in ["description", "published", "modified"] {
            assert!(page["content"].get(field).is_none(), "no inferred {field}");
        }
        for field in ["description", "datePublished", "dateModified"] {
            assert!(
                page["json_ld"].get(field).is_none(),
                "no inferred LD {field}"
            );
        }
    }
    let framed = &model["pages"]["doc:native"];
    assert_eq!(framed["content"]["published"], "2024-02-29");
    assert!(framed["content"].get("modified").is_none());
    assert!(framed["json_ld"].get("dateModified").is_none());
    let modified = &model["pages"]["content:one"];
    assert_eq!(modified["content"]["modified"], "2021-02-03T04:05:06+05:30");
    assert!(modified["content"].get("published").is_none());
    assert!(modified["json_ld"].get("datePublished").is_none());
    fs::write(
        fixture.directory.path().join("project/2025-09-01.md"),
        "changed source",
    )
    .expect("disposable edit");
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed metadata",
    )
    .expect("disposable edit");
    let repeated = render(&inputs).expect("retained facts do not reread native state");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        bytes(repeated.site().output(), "site.jsonld"),
        bytes(tree, "site.jsonld")
    );
}

#[test]
fn invalid_editorial_metadata_fails_before_staging_and_preserves_prior_output() {
    let good = Fixture::new(&[]);
    let parent = tempdir().expect("stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&good.config(), &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("prior receipt");
    let prior = std::path::Path::new(receipt["directory"].as_str().expect("prior stage"));
    let original = fs::read(prior.join("model.json")).expect("prior model");
    for declaration in [
        "published='2023-02-29'",
        "published='0000-01-01'",
        "modified='2024-01-01T00:00:00+14:01'",
        "modified='2024-01-01T00:00:00-14:01'",
        "published='2024-01-01T12:00:00'",
        "modified='2024-01-01T12:00:00-00:00'",
        "modified='2024-01-01T12:00:60Z'",
        "published=2024-01-01",
        "description=42",
    ] {
        let page = if declaration.starts_with("description=") {
            PAGE.replace(
                "description='The retained local article.'\n",
                &format!("{declaration}\n"),
            )
        } else {
            PAGE.replace("[content]\n", &format!("[content]\n{declaration}\n"))
        };
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let config = fixture.config();
        let inputs = acquire(&config);
        assert_eq!(
            render(&inputs)
                .expect_err("invalid explicit metadata")
                .code(),
            "publication_authored"
        );
        let output = cli(&config, &arguments);
        assert!(!output.status.success(), "accepted {declaration}");
        assert!(
            output.stdout.is_empty(),
            "no successful receipt for {declaration}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("publication_authored"),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_dir(parent.path()).expect("stage entries").count(),
            1
        );
        assert_eq!(
            fs::read(prior.join("model.json")).expect("unchanged model"),
            original
        );
    }
}
