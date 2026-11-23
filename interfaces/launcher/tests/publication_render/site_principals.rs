//! Configured authors follow publication eligibility; site rights never become page copyright.

use std::{convert::Infallible, path::Path};

use gazette_sdk::html::{self, TextKind};
use serde_json::{Value, json};

use super::{Fixture, PAGE, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs, source, tempdir};

const TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Site selections</title><script type=\"application/ld+json\">{{ json_ld | json_script }}</script></head><body><p>publisher={{ site.publisher }}</p><p>rights-holder={{ site.rights_holder }}</p><p>default-author={{ site.default_author }}</p><script type=\"application/json\">{{ site | json_script }}</script>{{ body_html | safe }}</body></html>";

fn scripts(bytes: &[u8]) -> Vec<Value> {
    let mut scripts = Vec::new();
    html::inspect(bytes, Default::default(), |kind, value| {
        if kind == TextKind::JsonScript {
            scripts.push(serde_json::from_str(value).expect("whole native JSON script"));
        }
        Ok::<_, Infallible>(())
    })
    .expect("complete native HTML tokenization");
    scripts
}

fn configuration(fixture: &Fixture) -> String {
    format!(
        "{}\n[[principals]]\nid='organization:rights'\nkind='organization'\nname='Independent site rights holder'\n[[principals]]\nid='person:default'\nkind='person'\nname='Selected default person'\n",
        fixture
            .config()
            .replace(
                "id='site:one'\n",
                "id='site:one'\nrights_holder='organization:rights'\ndefault_author='person:default'\nroles=[{principal='organization:rights',role='site-operator'}]\n",
            )
            .replace(
                "public_base_url=",
                "machine={json_ld=true,search='lexical'}\npublic_base_url=",
            )
    )
}

fn initialized_stage(config: &str, parent: &Path) -> Value {
    let arguments = json!({
        "publication":{
            "recipe_json":json!({
                "format":"gazette-local-publication-set/1",
                "render_year":2040,
                "workspace":{"kind":"supplied","source":config},
                "sites":[{"site":"site:one","model_output":"model.json","graph":{"output":"graph.json"}}]
            }).to_string(),
            "local":true,"allow_read":true
        },
        "create_stage":true,"output_parent":parent,"directory_bytes":1024
    });
    crate::mcp_carrier::initialized_interact("2026-07-28", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2026-07-28");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        let descriptor = catalog["result"]["tools"]
            .as_array()
            .expect("initialized native catalog")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("native rendering and staging tool");
        jsonschema::validator_for(&descriptor["inputSchema"])
            .expect("discovered input schema")
            .validate(&arguments)
            .expect("exact initialized request shape");
        let response = send(json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"stage_rendered_site","arguments":arguments}
        }));
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        let report = response["result"]["structuredContent"].clone();
        jsonschema::validator_for(&descriptor["outputSchema"])
            .expect("discovered native output schema")
            .validate(&report)
            .expect("actual rendering receipt conforms");
        let text: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("readable receipt"),
        )
        .expect("complete readable receipt JSON");
        assert_eq!(text, report);
        report
    })
}

fn exercise(explicit_author: bool) {
    let page = if explicit_author {
        PAGE.replace(
            "[content]\n",
            "[content]\nattributions=[{principal='owner:one',role='author'}]\n",
        )
    } else {
        PAGE.to_owned()
    };
    let fixture = Fixture::new(&[
        ("one/page.md", page.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
    ]);
    let before = source::inspect(fixture.directory.path(), None, Default::default())
        .expect("original native input repository")
        .identity();
    let config = configuration(&fixture);
    let inputs = acquire(&config);
    let rendered = build::render_publication(
        &inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).unwrap()),
            model_output: Some(&"model.json".parse().expect("model output")),
            graph: Some(build::GraphRenderOptions {
                output: &"graph.json".parse().expect("graph output"),
                limits: Default::default(),
            }),
            limits: Default::default(),
        },
        Default::default(),
    )
    .expect("actual retained publication with independent site selections");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("emitted model");
    let graph: Value =
        serde_json::from_slice(bytes(tree, "graph.json")).expect("Tera graph context");
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let linked: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site JSON-LD");
    let parsed = build::parse_publication(&inputs, Default::default()).expect("retained corpus");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same native semantic owner");
    let authored = resolved.authored();
    let discovery = authored.discovery().expect("same discovery policy");
    for (id, path, author) in [
        ("collection:home", "index.html", Some("person:default")),
        (
            "content:one",
            "guide/index.html",
            explicit_author.then_some("owner:one"),
        ),
    ] {
        let page = &model["pages"][id];
        let mut projection = authored
            .project_discovery_page(
                &id.parse().expect("same native content identity"),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect("same retained direct page");
        projection.data.site.metadata.as_mut().unwrap().render_year =
            Some(gazette_sdk::authored::RenderYear::new(2040).unwrap());
        assert_eq!(page["site"]["metadata"]["render_year"], 2040);
        assert_eq!(
            page["site"]["metadata"]["rights_holder"]["id"],
            "organization:rights"
        );
        assert_eq!(
            page["site"]["metadata"]["rights_holder"]["name"],
            "Independent site rights holder"
        );
        let direct_ld = serde_json::to_value(
            projection
                .discovery_json_ld(&discovery)
                .expect("source-bound direct JSON-LD")
                .public(),
        )
        .expect("whole direct-page JSON-LD");
        let listing_ld = serde_json::to_value(
            projection
                .listing_json_ld(&discovery)
                .expect("eligible aggregate JSON-LD")
                .public(),
        )
        .expect("whole listing JSON-LD");
        assert_eq!(page["json_ld"], listing_ld);
        super::json_ld::assert_public(&direct_ld);
        super::json_ld::assert_public(&listing_ld);
        let native_ld = json!(projection.json_ld());
        assert_eq!(page["site"], json!(projection.data.site));
        for (field, principal) in [
            ("publisher", "owner:one"),
            ("rights_holder", "organization:rights"),
            ("default_author", "person:default"),
        ] {
            assert_eq!(page["site"][field], principal);
            assert_eq!(model["graph"][field], principal);
            assert_eq!(graph[field], principal);
        }
        assert_eq!(
            page["site"]["roles"][0]["principal"]["id"],
            "organization:rights"
        );
        let record = search
            .iter()
            .find(|record| record["content"]["id"] == id)
            .expect("same search identity");
        assert_eq!(record["site"], page["site"]);
        assert_eq!(record["attributions"], page["attributions"]);
        let script = scripts(bytes(tree, path));
        assert_eq!(script, [direct_ld.clone(), page["site"].clone()]);
        let html = std::str::from_utf8(bytes(tree, path)).expect("actual HTML bytes");
        for text in [
            "publisher=owner:one",
            "rights-holder=organization:rights",
            "default-author=person:default",
        ] {
            assert!(html.contains(text), "visible configured selection {text}");
        }
        let node = linked["@graph"]
            .as_array()
            .expect("aggregate linked data")
            .iter()
            .find(|node| node["identifier"] == id)
            .expect("same linked identity");
        assert_eq!(node, &page["json_ld"]);
        assert_eq!(node, &listing_ld);
        assert_eq!(node["publisher"]["identifier"], "owner:one");
        assert_eq!(
            native_ld["gazette:siteRoles"]["@list"][0]["gazette:principal"]["identifier"],
            "organization:rights"
        );
        assert!(node.get("copyrightHolder").is_none());
        assert!(direct_ld.get("copyrightHolder").is_none());
        if let Some(author) = author {
            assert_eq!(described(node, &node["author"][0])["identifier"], author);
            assert_eq!(
                described(&direct_ld, &direct_ld["author"][0])["identifier"],
                author
            );
            assert_eq!(page["attributions"][0]["principal"]["id"], author);
            assert_eq!(page["attributions"].as_array().expect("authors").len(), 1);
        } else {
            assert!(node.get("author").is_none());
            assert!(direct_ld.get("author").is_none());
            assert_eq!(native_ld["gazette:attributions"], json!({"@list":[]}));
            assert_eq!(page["attributions"], json!([]));
        }
    }
    assert_eq!(
        model["graph"]["principals"]["person:default"]["kind"],
        "person"
    );
    assert_eq!(
        model["graph"]["principals"]["organization:rights"]["kind"],
        "organization"
    );
    assert_eq!(graph["presentation"]["publication"]["render_year"], 2040);
    assert_eq!(
        graph["presentation"]["publication"]["rights_holder"]["id"],
        "organization:rights"
    );
    let cli_parent = tempdir().expect("private CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--graph-output",
        "graph.json",
        "--output-parent",
        cli_parent.path().to_str().expect("native parent"),
        "--directory-bytes",
        "1024",
    ]);
    let result = cli(&config, &arguments);
    assert!(result.status.success(), "{result:?}");
    let cli_report: Value = serde_json::from_slice(&result.stdout).expect("actual CLI receipt");
    let mcp_parent = tempdir().expect("private MCP stage parent");
    let mcp_report = initialized_stage(&config, mcp_parent.path());
    for report in [&cli_report, &mcp_report] {
        assert_eq!(report["inventory"], json!(tree.inventory()));
        let directory = Path::new(report["directory"].as_str().expect("actual retained stage"));
        for entry in tree.inventory().entries() {
            if let Some(expected) = tree.file(&entry.path) {
                assert_eq!(
                    fs::read(directory.join(entry.path.as_str())).expect("actual staged bytes"),
                    expected,
                    "exact SDK/CLI/initialized MCP output {}",
                    entry.path
                );
            }
        }
    }
    assert_eq!(mcp_report["inputs"], cli_report["inputs"]);
    assert_eq!(mcp_report["evidence"], cli_report["evidence"]);
    assert_eq!(
        source::inspect(fixture.directory.path(), None, Default::default())
            .expect("unchanged native input repository")
            .identity(),
        before
    );
}

#[test]
fn configured_default_author_applies_to_published_collection_but_not_draft_page() {
    exercise(false);
}

#[test]
fn explicit_page_author_survives_distinct_site_default_across_real_interfaces() {
    exercise(true);
}

/// A document describes each principal once; a later mention carries only its `@id`,
/// so resolve it against the publisher described earlier in the same document.
fn described<'a>(document: &'a Value, mention: &'a Value) -> &'a Value {
    if mention.get("identifier").is_some() {
        return mention;
    }
    let publisher = &document["publisher"];
    assert_eq!(
        publisher["@id"], mention["@id"],
        "mention resolves in this document"
    );
    publisher
}
