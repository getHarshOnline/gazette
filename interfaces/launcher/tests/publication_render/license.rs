//! Content-licence facts flow through one real retained publication, not software metadata.

use super::{
    Fixture, HOME, PAGE, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs, render, source, tempdir,
};
use gazette_sdk::html::{self, TextKind};
use serde_json::{Value, json};
use std::{convert::Infallible, path::Path};

const LICENSE: &str = "https://licenses.example/Content%2Fterms?edition=1&lang=en#grant";
const TEMPLATE: &[u8] = br#"<!doctype html><html><head><title>Licence fixture</title><script type="application/ld+json">{{ json_ld | json_script }}</script></head><body><script type="application/json">{{ content | json_script }}</script>{% if content.license is defined %}<a rel="license" href="{{ content.license }}">Content licence</a>{% endif %}{{ body_html | safe }}</body></html>"#;
const GRAPH: &[u8] = br#"<!doctype html><html><head><title>Graph</title></head><body><script type="application/json">{{ nodes | json_script }}</script></body></html>"#;

fn scripts(bytes: &[u8]) -> Vec<Value> {
    let mut values = Vec::new();
    html::inspect(bytes, Default::default(), |kind, text| {
        if matches!(kind, TextKind::JsonScript) {
            values.push(serde_json::from_str(text).expect("native JSON script"));
        }
        Ok::<_, Infallible>(())
    })
    .expect("native HTML tokenization");
    values
}

#[test]
fn content_license_reaches_real_cli_theme_and_machine_outputs_without_repository_fallback() {
    let page = PAGE.replace("[content]\n", &format!("[content]\nlicense='{LICENSE}'\n"));
    let unlicensed = PAGE
        .replace("id='content:one'", "id='content:unlicensed'")
        .replace(
            "route={path='/guide/',aliases=['/old.html']}",
            "route={path='/unlicensed/'}",
        );
    let home = HOME.replace(
        "[content]\n",
        "[content]\nlicense='https://licenses.example/collection-only'\n",
    );
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("one/unlicensed.md", unlicensed.as_bytes()),
        ("one/home.md", home.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
        ("theme/templates/graph.html", GRAPH),
        (
            "LICENSE",
            b"Software licence only; not publication content permission.",
        ),
        (
            "Cargo.toml",
            b"[package]\nname='software-only'\nversion='0.0.0'\nlicense='MIT'\n",
        ),
    ]);
    let config = fixture.config().replace(
        "public_base_url=",
        "machine={json_ld=true,search='lexical'}\npublic_base_url=",
    );
    let inputs = acquire(&config);
    let graph_output = "graph.html".parse().expect("graph output");
    let model_output = "model.json".parse().expect("model");
    let options = || build::SiteRenderOptions {
        render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
        model_output: Some(&model_output),
        graph: Some(build::GraphRenderOptions {
            output: &graph_output,
            limits: Default::default(),
        }),
        limits: source::Limits::default(),
    };
    let rendered = build::render_publication(&inputs, options(), Default::default())
        .expect("actual connected publication");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("site model");
    let page = &model["pages"]["content:one"];
    assert_eq!(page["content"]["license"], LICENSE);
    assert_eq!(page["json_ld"]["license"], json!({"@id":LICENSE}));
    assert_eq!(page["content"]["lifecycle"], "draft");
    assert_eq!(model["graph"]["nodes"]["content:one"], page["content"]);
    assert_eq!(
        scripts(bytes(tree, "guide/index.html")),
        vec![page["json_ld"].clone(), page["content"].clone()]
    );
    assert!(
        std::str::from_utf8(bytes(tree, "guide/index.html"))
            .expect("actual HTML")
            .contains("rel=\"license\"")
    );
    assert_eq!(
        scripts(bytes(tree, "graph.html")),
        vec![model["graph"]["nodes"].clone()]
    );
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let record = search
        .iter()
        .find(|v| v["content"]["id"] == "content:one")
        .expect("exact search record");
    assert_eq!(record["content"], page["content"]);
    let linked: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site JSON-LD");
    assert!(
        linked["@graph"]
            .as_array()
            .expect("graph")
            .contains(&page["json_ld"])
    );
    let absent = &model["pages"]["content:unlicensed"];
    assert!(absent["content"].get("license").is_none());
    assert!(absent["json_ld"].get("license").is_none());
    assert!(
        model["graph"]["nodes"]["content:unlicensed"]
            .get("license")
            .is_none()
    );
    assert!(
        search
            .iter()
            .find(|v| v["content"]["id"] == "content:unlicensed")
            .expect("unlicensed search record")["content"]
            .get("license")
            .is_none()
    );
    assert!(
        !std::str::from_utf8(bytes(tree, "unlicensed/index.html"))
            .expect("unlicensed HTML")
            .contains("rel=\"license\"")
    );
    assert_eq!(
        model["pages"]["collection:home"]["content"]["license"],
        "https://licenses.example/collection-only"
    );

    let parent = tempdir().expect("fresh stage parent");
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
    assert_eq!(receipt["inventory"], json!(tree.inventory()));
    let stage = Path::new(receipt["directory"].as_str().expect("stage"));
    for entry in tree.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                fs::read(stage.join(entry.path.as_str())).expect("staged bytes"),
                tree.file(&entry.path).expect("retained output")
            );
        }
    }
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after capture",
    )
    .expect("only disposable source mutation");
    let repeated = build::render_publication(&inputs, options(), Default::default())
        .expect("retained rebuild");
    assert_eq!(tree.inventory(), repeated.site().output().inventory());
}

#[test]
fn malformed_content_license_is_refused_before_any_output_stage() {
    for value in [
        "'javascript:alert(1)'",
        "'MIT'",
        "42",
        "'https://user:password@licenses.example/terms'",
    ] {
        let page = PAGE.replace("[content]\n", &format!("[content]\nlicense={value}\n"));
        let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
        let config = fixture.config();
        assert!(
            render(&acquire(&config)).is_err(),
            "invalid content licence {value}"
        );
        let parent = tempdir().expect("fresh refusal stage");
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
        let output = cli(&config, &arguments);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty(), "no partial success receipt");
        assert!(
            fs::read_dir(parent.path())
                .expect("stage parent")
                .next()
                .is_none()
        );
    }
}
