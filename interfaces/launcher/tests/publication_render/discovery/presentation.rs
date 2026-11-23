//! Actual machine outputs, not configured promises, reach every rendered surface.

use super::*;

const DISCOVERY_TEMPLATE: &[u8] = br#"{% if presentation.discovery is defined %}{% for entry in presentation.discovery %}<a data-discovery="{{ entry.kind }}" href="{{ entry.url }}" type="{{ entry.media_type }}">{{ entry.kind }}</a>{% endfor %}{% endif %}"#;

fn expected(base: &str, sitemap: bool, llms: bool) -> Value {
    let mut records = Vec::new();
    for (enabled, kind, path, media_type) in [
        (sitemap, "sitemap", "sitemap.xml", "application/xml"),
        (llms, "llms", "llms.txt", "text/plain"),
    ] {
        if enabled {
            records
                .push(json!({"kind":kind,"url":format!("{base}{path}"),"media_type":media_type}));
        }
    }
    json!(records)
}

fn assert_html(tree: &source::CapturedSource, output: &str, records: &Value) {
    let html = std::str::from_utf8(bytes(tree, output))
        .expect("actual Theme output")
        .replace("&#x2F;", "/");
    let records = records.as_array().expect("typed discovery list");
    assert_eq!(
        html.matches("data-discovery=").count(),
        records.len(),
        "{output}"
    );
    for record in records {
        let kind = record["kind"].as_str().expect("kind");
        let url = record["url"].as_str().expect("resolved URL");
        let media_type = record["media_type"].as_str().expect("media type");
        assert!(
            html.contains(&format!(
                "data-discovery=\"{kind}\" href=\"{url}\" type=\"{media_type}\""
            )),
            "{output}: {html}"
        );
    }
}

#[test]
fn independent_output_switches_expose_only_emitted_mounted_destinations() {
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", DISCOVERY_TEMPLATE),
        ("theme/templates/alias.html", DISCOVERY_TEMPLATE),
    ]);
    let schema = gazette_sdk::schema::contract(gazette_sdk::schema::Contract::TemplateContext);
    let presentation_schema = json!({
        "$ref": "#/$defs/Presentation",
        "$defs": schema.as_value()["$defs"],
    });
    let validator =
        jsonschema::validator_for(&presentation_schema).expect("native presentation schema");
    for base in [
        "https://one.example/mount/",
        "http://127.0.0.1:4821/preview/mount/",
    ] {
        for (sitemap, llms) in [(false, false), (true, false), (false, true), (true, true)] {
            let config =
                configured(&fixture, sitemap, llms).replace("https://one.example/mount/", base);
            let inputs = acquire(&config);
            let rendered = render(&inputs).expect("complete native discovery render");
            let tree = rendered.site().output();
            let model: Value =
                serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
            let records = expected(base, sitemap, llms);
            for page in model["pages"].as_object().expect("page map").values() {
                assert!(
                    validator.is_valid(&page["presentation"]),
                    "native presentation must satisfy its published schema: {:?}",
                    validator
                        .iter_errors(&page["presentation"])
                        .map(|error| error.to_string())
                        .collect::<Vec<_>>()
                );
                if sitemap || llms {
                    assert_eq!(page["presentation"]["discovery"], records);
                    let mut invalid = page["presentation"].clone();
                    invalid["discovery"][0]["kind"] = json!("unemitted-arbitrary-output");
                    assert!(
                        !validator.is_valid(&invalid),
                        "discovery kinds are a bounded native contract"
                    );
                } else {
                    assert!(
                        page["presentation"].get("discovery").is_none(),
                        "empty discovery is omitted"
                    );
                }
            }
            for (path, emitted) in [("sitemap.xml", sitemap), ("llms.txt", llms)] {
                assert_eq!(
                    tree.file(&path.parse().expect("machine path")).is_some(),
                    emitted
                );
            }
            for path in ["index.html", "guide/index.html", "old.html"] {
                assert_html(tree, path, &records);
            }
        }
    }
}

#[test]
fn partition_root_discovery_reaches_graph_mirror_taxonomy_and_continuation_pages() {
    let article = PAGE.replace(
        "[content]\n",
        "[content]\ntaxonomies=[{taxonomy='taxonomy:topic',term='term:one'}]\n",
    );
    let other = super::super::OTHER.replace(
        "route={path='/elsewhere/'}",
        "route={path='/elsewhere/',mirrors=[{site='site:one',path='/copy/'}]}",
    );
    let second = "+++\n[content]\nid='collection:second'\nsite='site:one'\ntitle='Second collection'\nlifecycle='published'\nroute={path='/second/'}\nstructure={kind='collection'}\n+++\nSecond collection body\n";
    let index = "+++\n[content]\nid='content:directory'\nsite='site:one'\ntitle='Collection directory'\nlifecycle='published'\nroute={path='/directory/'}\nstructure={kind='page',family={kind='index'}}\n+++\n";
    let fixture = Fixture::new(&[
        ("one/page.md", article.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("one/second.md", second.as_bytes()),
        ("one/directory.md", index.as_bytes()),
        ("theme/templates/page.html", DISCOVERY_TEMPLATE),
        ("theme/templates/alias.html", DISCOVERY_TEMPLATE),
        ("theme/templates/graph.html", DISCOVERY_TEMPLATE),
        ("theme/templates/taxonomy.html", DISCOVERY_TEMPLATE),
    ]);
    let mut config = configured(&fixture, true, true)
        .replacen("machine={sitemap=true", "machine={sitemap=true,sitemap_urls_per_file=1,graph={enabled=true,path='/connections/'}", 1)
        .replacen("authored=[{source='source:one',target='one'}]", "authored=[{source='source:one',target='one'}]\nlistings=[{content='content:directory',select={kind='root-collections'},order={first=['collection:home'],remaining='title-then-id'},pagination={page_size=1,next_pages='page/{number}/'}}]\ntaxonomy_pages=[{taxonomy='taxonomy:topic',route='/topics/',template='fixture/taxonomy.html'}]", 1);
    config.push_str("[[taxonomies]]\nid='taxonomy:topic'\nlabel='Topics'\nterms=[{id='term:one',label='First topic'}]\n");
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("all generated surfaces and discovery complete together");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let records = expected("https://one.example/mount/", true, true);
    for surface in [
        &model["pages"]["content:one"],
        &model["pages"]["content:directory"],
        &model["listing_pages"]["directory/page/2/index.html"],
        &model["taxonomy_pages"]["topics/index.html"],
        &model["mirrors"]["copy/index.html"],
        &model["graph"],
    ] {
        assert_eq!(surface["presentation"]["discovery"], records);
    }
    for path in [
        "guide/index.html",
        "old.html",
        "directory/index.html",
        "directory/page/2/index.html",
        "topics/index.html",
        "copy/index.html",
        "connections/index.html",
    ] {
        assert_html(tree, path, &records);
    }
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).expect("sitemap index");
    assert!(sitemap.contains("<sitemapindex"));
    assert!(sitemap.contains("sitemap-00001.xml"));
    let mut children = String::new();
    for entry in tree.inventory().entries() {
        if entry.path.as_str().starts_with("sitemap-") {
            children.push_str(
                std::str::from_utf8(bytes(tree, entry.path.as_str())).expect("partition XML"),
            );
        }
    }
    for included in ["/mount/topics/", "/mount/directory/page/2/"] {
        assert!(
            children.contains(included),
            "moving emission earlier must preserve generated canonical pages"
        );
    }
    for excluded in ["/old.html", "/copy/"] {
        assert!(
            !children.contains(excluded),
            "discovery presentation must not change canonical sitemap membership"
        );
    }
    assert_eq!(
        records.as_array().expect("records").len(),
        2,
        "partition children are not additional footer destinations"
    );
}
