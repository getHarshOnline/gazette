//! Existing index identity and native listing slices through real output carriers.

use super::*;
use gazette_sdk::html::{self, TextKind};
use serde_json::json;
use std::convert::Infallible;

const INDEX: &str = "+++\ntemplate='fixture/listing.html'\n[content]\nid='content:blog'\nsite='site:one'\ntitle='Reading Ω <&>'\nlifecycle='published'\nroute={path='/blog/'}\nstructure={kind='page',family={kind='index'}}\n+++\n";
const LISTING_TEMPLATE: &[u8] = b"<article data-owner=\"{{ content.id }}\"><h1>{{ content.title }}</h1><main>{{ body_html | safe }}</main><a data-current href=\"{{ route.canonical_url }}\">current</a><ol data-page=\"{{ listing.page_number }}\" data-count=\"{{ listing.page_count }}\" data-total=\"{{ listing.total_items }}\">{% for item in listing.items %}<li data-id=\"{{ item.id }}\"><a href=\"{{ item.url }}\">{{ item.title }}</a></li>{% endfor %}</ol>{% if listing.previous is defined %}<a data-previous href=\"{{ listing.previous }}\">previous</a>{% endif %}{% if listing.next is defined %}<a data-next href=\"{{ listing.next }}\">next</a>{% endif %}{% if json_ld is defined %}<script type=\"application/ld+json\">{{ json_ld | json_script }}</script>{% endif %}<script type=\"application/json\" data-context>{{ __tera_context | json_script }}</script></article>";
const VIEW_OUTPUTS: [&str; 2] = ["blog/index.html", "blog/page/2/index.html"];
const PUBLIC_OUTPUTS: [&str; 6] = [
    "blog/index.html",
    "blog/page/2/index.html",
    "model.json",
    "search.json",
    "sitemap.xml",
    "site.jsonld",
];

fn article(id: &str, title: &str, metadata: &str) -> String {
    format!(
        "+++\n[content]\nid='content:{id}'\nsite='site:one'\ntitle='{title}'\nlifecycle='published'\nroute={{path='/{id}/'}}\n{metadata}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# Retained {title}\n"
    )
}

fn fixture(index: &str, collision: bool) -> Fixture {
    let newer = article("new", "Newer Ω", "published='2026-10-05'");
    let older = article("older", "Older", "published='2026-10-01'");
    let undated = article("undated", "A missing date", "modified='2099-01-01'");
    let contained = PAGE.replace(
        "lifecycle='draft'",
        "lifecycle='published'\npublished='2030-01-01'",
    );
    let collision_page = OTHER
        .replace("site='site:other'", "site='site:one'")
        .replace("id='content:other'", "id='content:collision'")
        .replace("path='/elsewhere/'", "path='/blog/page/2/'");
    let mut files = vec![
        ("one/blog.md", index.as_bytes()),
        ("one/new.md", newer.as_bytes()),
        ("one/undated.md", undated.as_bytes()),
        ("one/page.md", contained.as_bytes()),
        ("project/older.md", older.as_bytes()),
        ("theme/templates/listing.html", LISTING_TEMPLATE),
    ];
    if collision {
        files.push(("one/collision.md", collision_page.as_bytes()));
    }
    Fixture::new(&files)
}

fn config(fixture: &Fixture) -> String {
    let mut config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one'},{source='source:articles',target='distributed'}]\nlistings=[{content='content:blog',select={kind='standalone-articles'},order={by='published',direction='descending',undated='last-title-then-id'},pagination={page_size=2,next_pages='page/{number}/'}}]\nmachine={search='lexical',sitemap=true,json_ld=true}",
    );
    config.push_str(&format!(
        "[[sources]]\nid='source:articles'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='project'}}\n",
        fixture.directory.path().to_str().expect("explicit native root"),
        fixture.commit,
    ));
    config
}

fn publication(config: &str) -> Value {
    json!({
        "recipe_json":json!({
            "format":"gazette-local-publication-set/1",
            "render_year":2040,
            "workspace":{"kind":"supplied","source":config},
            "sites":[{"site":"site:one","model_output":"model.json"}]
        }).to_string(),
        "local":true,"allow_read":true
    })
}

fn assert_stage(receipt: &Value, rendered: &build::PublicationRender<'_>) {
    let tree = rendered.site().output();
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("native inventory")
    );
    assert_eq!(
        receipt["evidence"],
        serde_json::to_value(rendered.site().evidence()).expect("native evidence")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("fresh stage"));
    for path in PUBLIC_OUTPUTS {
        assert_eq!(
            fs::read(directory.join(path)).expect("actual staged output"),
            bytes(tree, path),
            "same retained output: {path}"
        );
    }
}

#[test]
fn distributed_articles_paginate_existing_index_across_theme_machine_and_cli_outputs() {
    let fixture = fixture(INDEX, false);
    let config = config(&fixture);
    let inputs = acquire(&config);
    let parsed = build::parse_publication(&inputs, Default::default()).expect("native corpus");
    let resolved = parsed.resolve(Default::default()).expect("native graph");
    let authored = resolved.authored();
    let listing = authored
        .project_listing_pages(
            &"site:one".parse().expect("site"),
            &authored.discovery().expect("bound discovery"),
        )
        .expect("complete compiler listing");
    assert_eq!(listing.len(), 2);
    assert_eq!(
        listing[0]
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["content:new", "content:older"]
    );
    assert_eq!(listing[1].items[0].id.as_str(), "content:undated");
    assert!(
        listing[1].items[0].published.is_none(),
        "modified is not published"
    );
    let rendered = render(&inputs).expect("complete native listing rendering");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let pages = model["pages"].as_object().expect("existing content map");
    assert_eq!(pages.len(), 6);
    let continuations = model["listing_pages"]
        .as_object()
        .expect("output-keyed continuations");
    assert_eq!(continuations.len(), 1);
    let first = &pages["content:blog"];
    let second = &continuations[VIEW_OUTPUTS[1]];
    let schema = gazette_sdk::schema::contract(gazette_sdk::schema::Contract::TemplateContext);
    let validator =
        jsonschema::validator_for(schema.as_value()).expect("native Theme context schema");
    for (index, page) in [first, second].into_iter().enumerate() {
        assert_eq!(
            page["content"], first["content"],
            "owner identity never changes"
        );
        assert_eq!(page["content"]["id"], "content:blog");
        assert_eq!(page["content"]["title"], "Reading Ω <&>");
        assert_eq!(page["body_html"], "", "index is declaration-only");
        assert_eq!(
            page["listing"],
            json!(listing[index]),
            "compiler-owned slice"
        );
        assert_eq!(page["route"], json!(listing[index].serving_route()));
        assert_eq!(page["listing"]["page_number"], index + 1);
        assert_eq!(page["listing"]["page_count"], 2);
        assert_eq!(page["listing"]["total_items"], 3);
        let html = std::str::from_utf8(bytes(tree, VIEW_OUTPUTS[index]))
            .expect("selected listing Theme HTML")
            .replace("&#x2F;", "/");
        assert!(html.contains("data-owner=\"content:blog\""));
        assert!(html.contains("<h1>Reading Ω &lt;&amp;&gt;</h1><main></main>"));
        assert!(html.contains(&format!(
            "data-page=\"{}\" data-count=\"2\" data-total=\"3\"",
            index + 1
        )));
        assert!(html.contains(&format!("data-current href=\"{}\"", listing[index].url)));
        for item in &listing[index].items {
            assert!(html.contains(&format!("data-id=\"{}\"", item.id)));
            assert!(html.contains(&format!("href=\"{}\"", item.url)));
        }
        assert!(
            !html.contains("data-id=\"content:one\""),
            "contained article excluded despite newer date"
        );
        assert_eq!(
            page["json_ld"]["mainEntity"],
            listing[index].public_item_list()
        );
        let expected: &[(&str, &str)] = if index == 0 {
            &[
                (
                    "https://one.example/mount/new/",
                    "https://one.example/mount/new/#article",
                ),
                (
                    "https://one.example/mount/older/",
                    "https://one.example/mount/older/#article",
                ),
            ]
        } else {
            &[(
                "https://one.example/mount/undated/",
                "https://one.example/mount/undated/#article",
            )]
        };
        let public_list = &page["json_ld"]["mainEntity"];
        assert_eq!(public_list["@type"], "ItemList");
        let elements = public_list["itemListElement"].as_array().unwrap();
        assert_eq!(elements.len(), expected.len());
        assert_eq!(public_list["numberOfItems"], expected.len());
        for (element, (url, identity)) in elements.iter().zip(expected) {
            assert_eq!(element["@type"], "ListItem");
            assert_eq!(element["item"]["@type"], "Article");
            assert_eq!(element["item"]["url"], *url);
            assert_eq!(element["item"]["@id"], *identity);
        }
        assert_eq!(page["json_ld"]["identifier"], "content:blog");
        assert_eq!(page["json_ld"]["url"], listing[index].url);
        super::discoverability::assert_public_json_ld(&page["json_ld"]);
        let mut scripts = Vec::<Value>::new();
        let mut contexts = Vec::<Value>::new();
        html::inspect(
            bytes(tree, VIEW_OUTPUTS[index]),
            Default::default(),
            |kind, text| {
                if kind == TextKind::JsonScript {
                    let value: Value = serde_json::from_str(text).expect("native JSON script");
                    if let Some(context) = value.as_str() {
                        contexts.push(serde_json::from_str(context).expect("actual Tera context"));
                    } else {
                        scripts.push(value);
                    }
                }
                Ok::<_, Infallible>(())
            },
        )
        .expect("actual HTML tokenizer");
        assert_eq!(contexts.len(), 1, "one actual Tera context");
        let actual = &contexts[0];
        validator
            .validate(actual)
            .expect("native Theme context schema");
        for field in ["content", "route", "listing", "json_ld", "presentation"] {
            assert_eq!(actual[field], page[field], "same shared field: {field}");
        }
        assert_eq!(
            scripts,
            [page["json_ld"].clone()],
            "one shared linked-data value"
        );
    }
    assert_eq!(first["listing"]["next"], second["route"]["url"]);
    assert!(first["listing"].get("previous").is_none());
    assert_eq!(second["listing"]["previous"], first["route"]["url"]);
    assert!(second["listing"].get("next").is_none());
    assert_eq!(second["json_ld"]["@id"], listing[1].url);
    let first_html = std::str::from_utf8(bytes(tree, VIEW_OUTPUTS[0]))
        .expect("first HTML")
        .replace("&#x2F;", "/");
    let second_html = std::str::from_utf8(bytes(tree, VIEW_OUTPUTS[1]))
        .expect("second HTML")
        .replace("&#x2F;", "/");
    assert!(first_html.contains(&format!("data-next href=\"{}\"", listing[1].url)));
    assert!(second_html.contains(&format!("data-previous href=\"{}\"", listing[0].url)));
    assert_eq!(listing[1].url, "https://one.example/mount/blog/page/2/");
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("search records");
    assert_eq!(
        search.len(),
        pages.len(),
        "continuation is not another search identity"
    );
    assert_eq!(
        search
            .iter()
            .filter(|record| record["content"]["id"] == "content:blog")
            .count(),
        1
    );
    for record in &search {
        let id = record["content"]["id"].as_str().expect("existing identity");
        assert_eq!(record["route"], pages[id]["route"]);
    }
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).expect("sitemap");
    assert_eq!(sitemap.matches("<loc>").count(), pages.len() + 1);
    for page in &listing {
        assert!(sitemap.contains(&format!("<loc>{}</loc>", page.url)));
    }
    let linked: Value =
        serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site linked data");
    let graph = linked["@graph"].as_array().expect("native graph");
    assert_eq!(graph.len(), pages.len() + 1);
    for page in [first, second] {
        assert_eq!(
            graph
                .iter()
                .filter(|node| *node == &page["json_ld"])
                .count(),
            1
        );
    }
    let evidence = serde_json::to_value(rendered.site().evidence()).expect("operator evidence");
    assert_eq!(
        evidence["pages"]["content:blog"]["template"],
        "fixture/listing.html"
    );
    assert_eq!(
        evidence["listing_pages"][VIEW_OUTPUTS[1]]["template"],
        "fixture/listing.html"
    );
    assert_eq!(
        evidence["listing_pages"][VIEW_OUTPUTS[1]]["content"],
        "content:blog"
    );
    for path in PUBLIC_OUTPUTS {
        let text = std::str::from_utf8(bytes(tree, path)).expect("public UTF-8");
        for private in [
            fixture.directory.path().to_str().expect("root"),
            "source:articles",
            "distributed/older.md",
            "one/blog.md",
        ] {
            assert!(
                !text.contains(private),
                "private coordinate {private} in {path}"
            );
        }
    }
    let parent = tempdir().expect("CLI stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI receipt");
    assert_stage(&receipt, &rendered);
    fs::write(
        fixture.directory.path().join("project/older.md"),
        b"changed after capture",
    )
    .expect("disposable source mutation");
    fs::write(
        fixture
            .directory
            .path()
            .join("theme/templates/listing.html"),
        b"changed Theme",
    )
    .expect("disposable Theme mutation");
    let repeated = render(&inputs).expect("retained corpus and Theme");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
}

#[test]
fn initialized_mcp_refuses_colliding_continuation_then_stages_valid_listing_in_same_process() {
    let bad = fixture(INDEX, true);
    let good = fixture(INDEX, false);
    let bad_config = config(&bad);
    let good_config = config(&good);
    super::assert_cli_failure_preserves_parent(&bad_config, "listing_route_collision");
    let inputs = acquire(&good_config);
    let rendered = render(&inputs).expect("valid native listing");
    let parent = tempdir().expect("MCP stage parent");
    fs::write(parent.path().join("keep"), b"unrelated").expect("unrelated output");
    let arguments = |config: &str| json!({"publication":publication(config),"create_stage":true,"output_parent":parent.path(),"directory_bytes":1024});
    mcp_carrier::initialized_interact("2025-11-25", |initialized, send| {
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        let catalog = send(json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}));
        assert!(catalog.get("error").is_none(), "{catalog}");
        let tool = catalog["result"]["tools"]
            .as_array()
            .expect("initialized catalog")
            .iter()
            .find(|tool| tool["name"] == "stage_rendered_site")
            .expect("native tool");
        let input_schema = jsonschema::validator_for(&tool["inputSchema"]).expect("input schema");
        let output_schema =
            jsonschema::validator_for(&tool["outputSchema"]).expect("output schema");
        for (id, config, failed) in [
            (2, bad_config.as_str(), true),
            (3, good_config.as_str(), false),
        ] {
            let args = arguments(config);
            input_schema.validate(&args).expect("actual native input");
            let response = send(
                json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"stage_rendered_site","arguments":args}}),
            );
            assert!(
                response.get("error").is_none(),
                "application result, healthy initialized carrier: {response}"
            );
            assert_eq!(response["result"]["isError"], failed, "{response}");
            let value = &response["result"]["structuredContent"];
            output_schema.validate(value).expect("typed native result");
            let text: Value = serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .expect("text result"),
            )
            .expect("JSON result");
            assert_eq!(text, *value);
            if failed {
                assert_eq!(value["code"], "listing_route_collision", "{value}");
                assert_eq!(
                    fs::read_dir(parent.path())
                        .expect("no partial stage")
                        .count(),
                    1
                );
            } else {
                assert_stage(value, &rendered);
                assert_eq!(
                    fs::read_dir(parent.path())
                        .expect("one successful stage")
                        .count(),
                    2
                );
            }
            assert_eq!(
                fs::read(parent.path().join("keep")).expect("unrelated file retained"),
                b"unrelated"
            );
        }
    });
}

#[test]
fn configured_listing_owner_with_authored_body_refuses_render_and_cli_staging() {
    let fixture = fixture(&format!("{INDEX}Not a declaration-only index.\n"), false);
    let config = config(&fixture);
    let inputs = acquire(&config);
    let error = render(&inputs).expect_err("owner body cannot be silently discarded");
    assert_eq!(error.code(), "listing_binding");
    super::assert_cli_failure_preserves_parent(&config, "listing_binding");
}

#[test]
fn opposite_order_page_one_and_continuations_share_native_static_outputs() {
    let fixture = fixture(INDEX, false);
    let config = config(&fixture).replace("pagination={page_size=2", "alternate_order={order={by='published',direction='ascending',undated='last-title-then-id'},path='oldest'},pagination={page_size=2").replace("machine={search='lexical',sitemap=true,json_ld=true}", "machine={search='lexical',sitemap=true,json_ld=true,markdown_alternates={enabled=true,directory_page='index.md',file_page='append-md',media_type='text/markdown',discovery_rel='alternate'}}");
    let config = config.replace("[[sites]]\nid='site:one'", "[[sites]]\ncomposition.invitations.'theme:services'={state='enabled',destination='content:other',articles=false,pages={'content:blog'=true}}\nid='site:one'");
    let inputs = acquire(&config);
    let rendered = render(&inputs).unwrap();
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).unwrap();
    assert_eq!(
        model["pages"]["content:blog"]["listing"]["variant"],
        "primary"
    );
    assert_eq!(model["listing_pages"].as_object().unwrap().len(), 3);
    for (output, direction, variant, number, first) in [
        ("blog/index.html", "descending", "primary", 1, "content:new"),
        (
            "blog/page/2/index.html",
            "descending",
            "primary",
            2,
            "content:undated",
        ),
        (
            "blog/oldest/index.html",
            "ascending",
            "alternate",
            1,
            "content:older",
        ),
        (
            "blog/oldest/page/2/index.html",
            "ascending",
            "alternate",
            2,
            "content:undated",
        ),
    ] {
        let page = if output == "blog/index.html" {
            &model["pages"]["content:blog"]
        } else {
            &model["listing_pages"][output]
        };
        let listing = &page["listing"];
        assert_eq!(listing["direction"], direction);
        assert_eq!(listing["variant"], variant);
        assert_eq!(listing["page_number"], number);
        assert_eq!(listing["items"][0]["id"], first);
        assert_eq!(listing["pages"].as_array().unwrap().len(), 2);
        assert_eq!(
            listing["sorts"][0]["url"],
            "https://one.example/mount/blog/"
        );
        assert_eq!(
            listing["sorts"][1]["url"],
            "https://one.example/mount/blog/oldest/"
        );
        assert_eq!(page["route"]["canonical_url"], listing["url"]);
        assert_eq!(page["json_ld"]["url"], listing["url"]);
        let structured = &page["json_ld"]["mainEntity"];
        assert_eq!(
            structured["itemListOrder"],
            if direction == "descending" {
                "https://schema.org/ItemListOrderDescending"
            } else {
                "https://schema.org/ItemListOrderAscending"
            }
        );
        for (index, item) in listing["items"].as_array().unwrap().iter().enumerate() {
            let linked = &structured["itemListElement"][index];
            assert_eq!(linked["position"], index + 1);
            assert_eq!(linked["item"]["identifier"], item["id"]);
        }
        assert_eq!(page["content"]["id"], "content:blog");
        let html = std::str::from_utf8(bytes(tree, output))
            .unwrap()
            .replace("&#x2F;", "/");
        assert!(html.contains(&format!(
            "data-current href=\"{}\"",
            listing["url"].as_str().unwrap()
        )));
        let mut invitation_present = None;
        html::inspect(bytes(tree, output), Default::default(), |kind, text| {
            if kind == TextKind::JsonScript {
                let value: Value = serde_json::from_str(text).unwrap();
                if let Some(context) = value.as_str() {
                    let context: Value = serde_json::from_str(context).unwrap();
                    invitation_present = Some(context.get("invitations").is_some());
                }
            }
            Ok::<_, Infallible>(())
        })
        .unwrap();
        assert_eq!(
            invitation_present,
            Some(output == "blog/index.html"),
            "{output}"
        );
        let markdown = output
            .strip_suffix("index.html")
            .map(|prefix| format!("{prefix}index.md"))
            .unwrap();
        let md = std::str::from_utf8(bytes(tree, &markdown)).unwrap();
        assert!(md.contains(listing["items"][0]["title"].as_str().unwrap()));
    }
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).unwrap();
    assert!(sitemap.contains("https://one.example/mount/blog/oldest/"));
    assert!(sitemap.contains("https://one.example/mount/blog/oldest/page/2/"));
    let search: Value = serde_json::from_slice(bytes(tree, "search.json")).unwrap();
    assert_eq!(
        search
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v["content"]["id"] == "content:blog")
            .count(),
        1
    );
    let older = &model["pages"]["content:older"]["browsing_locations"];
    assert!(
        older
            .as_array()
            .unwrap()
            .iter()
            .all(|location| !location["url"].as_str().unwrap().contains("oldest")),
        "article browsing stays on the primary chronology"
    );
}
