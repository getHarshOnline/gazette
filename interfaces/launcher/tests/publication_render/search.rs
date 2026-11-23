//! Complete retained search documents through the native SDK and CLI pipeline.

#[path = "search/browser.rs"]
mod browser;

#[path = "search/configured.rs"]
mod configured;

use super::{
    Fixture, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, Value, acquire, build, bytes, cli, fs, render,
    source, tempdir, toml,
};

fn enable(config: &str) -> String {
    config.replace(
        "public_base_url=",
        "machine={search='lexical'}\npublic_base_url=",
    )
}

const SEARCH_TEMPLATE: &str = "<h1>{{ content.title }}</h1><a href=\"{{ route.canonical_url }}\">canonical</a>{% if presentation.search %}<a data-search-output=\"{{ presentation.search.output }}\" href=\"{{ presentation.search.url }}\">search corpus</a>{% else %}<span>search unavailable</span>{% endif %}";

fn assert_transport(tree: &source::CapturedSource, expected: Option<&str>) {
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("actual model");
    for page in model["pages"].as_object().expect("page map").values() {
        let html = std::str::from_utf8(bytes(
            tree,
            page["route"]["output"].as_str().expect("page output"),
        ))
        .expect("actual Theme HTML")
        .replace("&#x2F;", "/");
        if let Some(url) = expected {
            assert_eq!(
                page["presentation"]["search"],
                serde_json::json!({"profile":"lexical","fields":["title","body","summary","projects","keywords"],"route":"/search.json","output":"search.json","url":url})
            );
            assert!(
                html.contains(&format!(
                    "data-search-output=\"search.json\" href=\"{url}\""
                )),
                "Theme consumes exactly the shared transport"
            );
        } else {
            assert!(
                page["presentation"].get("search").is_none(),
                "disabled search is omitted, not a guessed URL or null binding"
            );
            assert!(html.contains("search unavailable"));
            assert!(!html.contains("data-search-output="));
        }
    }
    let index = tree.file(&"search.json".parse().expect("search output"));
    assert_eq!(index.is_some(), expected.is_some());
    if let Some(bytes) = index {
        let records: Vec<Value> = serde_json::from_slice(bytes).expect("complete emitted corpus");
        assert!(!records.is_empty());
        for record in records {
            assert!(
                record.get("presentation").is_none(),
                "no recursive presentation metadata in corpus"
            );
            assert_eq!(
                record["route"],
                model["pages"][record["content"]["id"].as_str().expect("identity")]["route"]
            );
        }
    }
}

#[test]
fn search_transport_uses_each_mounted_site_and_matches_sdk_cli_and_retained_template() {
    let fixture = Fixture::new(&[("theme/templates/page.html", SEARCH_TEMPLATE.as_bytes())]);
    let config = enable(&fixture.config()).replace(
        "https://other.example/",
        "https://other.example/second%20mount/",
    );
    let selected = ["site:one", "site:other"];
    let inputs = super::sites::acquire_set(&config, &selected);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let options = || {
        let mut options = super::sites::options(&selected, &page, &alias, &model);
        for site in options.sites.values_mut() {
            site.render_year =
                Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"));
        }
        options
    };
    let rendered =
        build::render_publications(&inputs, options(), Default::default()).expect("shared set");
    assert!(rendered.is_complete());
    for (site, url) in [
        ("site:one", "https://one.example/mount/search.json"),
        (
            "site:other",
            "https://other.example/second%20mount/search.json",
        ),
    ] {
        let tree = rendered.sites()[&site.parse::<gazette_sdk::workspace::Key>().expect("site")]
            .as_ref()
            .expect("rendered site")
            .site()
            .output();
        assert_transport(tree, Some(url));
        let parent = tempdir().expect("independent stage");
        let mut arguments = RENDER_ARGUMENTS.to_vec();
        arguments[3] = site;
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
        let receipt: Value = serde_json::from_slice(&result.stdout).expect("CLI native receipt");
        assert_eq!(
            receipt["inventory"],
            serde_json::to_value(tree.inventory()).expect("SDK inventory")
        );
        let directory = std::path::Path::new(
            receipt["directory"]
                .as_str()
                .expect("actual retained directory"),
        );
        assert_eq!(
            fs::read(directory.join("search.json")).expect("emitted search artifact"),
            bytes(tree, "search.json")
        );
        assert_eq!(
            fs::read(directory.join("model.json")).expect("emitted model"),
            bytes(tree, "model.json")
        );
    }
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        b"changed after capture",
    )
    .expect("disposable source change");
    let repeated = build::render_publications(&inputs, options(), Default::default())
        .expect("retained render");
    for site in selected {
        let key: gazette_sdk::workspace::Key = site.parse().expect("site");
        assert_eq!(
            repeated.sites()[&key]
                .as_ref()
                .expect("repeat")
                .site()
                .output()
                .inventory()
                .identity(),
            rendered.sites()[&key]
                .as_ref()
                .expect("original")
                .site()
                .output()
                .inventory()
                .identity()
        );
    }
}

#[test]
fn disabled_search_omits_transport_in_model_and_actual_theme() {
    let fixture = Fixture::new(&[("theme/templates/page.html", SEARCH_TEMPLATE.as_bytes())]);
    let inputs = acquire(&fixture.config());
    let rendered = render(&inputs).expect("disabled search renders");
    assert_transport(rendered.site().output(), None);
    let parent = tempdir().expect("independent stage");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("stage")]);
    let result = cli(&fixture.config(), &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("receipt");
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("directory"));
    assert!(!directory.join("search.json").exists());
    assert_eq!(
        fs::read(directory.join("model.json")).expect("actual model"),
        bytes(rendered.site().output(), "model.json")
    );
}

#[test]
fn search_transport_follows_held_preview_urls_without_changing_public_configuration() {
    use gazette_sdk::preview::{self, PreviewReservation};

    let fixture = Fixture::new(&[("theme/templates/page.html", SEARCH_TEMPLATE.as_bytes())]);
    let config = enable(&fixture.config());
    let selected = ["site:one", "site:other"];
    let inputs = super::sites::acquire_set(&config, &selected);
    let reservation = preview::reserve(
        &inputs,
        selected
            .iter()
            .map(|site| {
                (
                    site.parse().expect("site"),
                    "127.0.0.1:0".parse().expect("ephemeral loopback"),
                )
            })
            .collect(),
        preview::Options {
            allow_listeners: true,
            ..Default::default()
        },
    )
    .expect("held exact native listeners");
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let local = preview::render(
        &inputs,
        &reservation,
        super::sites::options(&selected, &page, &alias, &model),
        Default::default(),
    )
    .expect("preview rendering from held URLs");
    assert!(local.is_complete());
    for site in selected {
        let key: gazette_sdk::workspace::Key = site.parse().expect("site");
        let url = format!("{}search.json", reservation.urls()[&key].as_str());
        assert!(url.starts_with("http://127.0.0.1:"));
        assert_transport(
            local.sites()[&key]
                .as_ref()
                .expect("local site")
                .site()
                .output(),
            Some(&url),
        );
    }
    drop(local);
    drop(reservation);
    let production = build::render_publications(
        &inputs,
        super::sites::options(&selected, &page, &alias, &model),
        Default::default(),
    )
    .expect("same captured public inputs");
    assert_transport(
        production.sites()[&"site:one"
            .parse::<gazette_sdk::workspace::Key>()
            .expect("site")]
            .as_ref()
            .expect("public site")
            .site()
            .output(),
        Some("https://one.example/mount/search.json"),
    );
}

fn documents(tree: &source::CapturedSource) -> Vec<Value> {
    serde_json::from_slice(bytes(tree, "search.json")).expect("complete search document array")
}

fn assert_shared_metadata(tree: &source::CapturedSource) {
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("same page model");
    let documents = documents(tree);
    assert_eq!(
        documents.len(),
        model["pages"].as_object().expect("page map").len()
    );
    let mut previous: Option<&str> = None;
    for document in &documents {
        let id = document["content"]["id"]
            .as_str()
            .expect("exact content identity");
        let page = model["pages"][id]
            .as_object()
            .expect("corresponding rendered page");
        let mut expected = page.clone();
        let text = expected.remove("body_text").expect("shared retained text");
        expected.remove("body_html").expect("rendered body");
        expected.remove("opening");
        expected
            .remove("presentation")
            .expect("page-only presentation binding");
        expected.insert("body".into(), text);
        let mut projects = Vec::new();
        for link in expected["projects"]
            .as_array()
            .expect("native project relations")
        {
            projects.push(link["project"]["id"].clone());
            if let Some(name) = link["project"]["name"]
                .as_str()
                .filter(|name| !name.is_empty())
            {
                projects.push(Value::String(name.to_owned()));
            }
        }
        let ranking = serde_json::json!({
            "title": expected["content"]["title"],
            "body": expected["body"],
            "summary": page.get("summary").and_then(|summary| summary["text"].as_str()).unwrap_or(""),
            "projects": projects,
            "keywords": expected["content"].get("keywords").cloned().unwrap_or_else(|| serde_json::json!([])),
        });
        let selected = &page["presentation"]["search"];
        assert_eq!(selected["profile"], "lexical");
        let selected_fields = selected["fields"]
            .as_array()
            .expect("shared native selected fields")
            .iter()
            .map(|field| field.as_str().expect("field string").to_owned())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            ranking
                .as_object()
                .expect("expected ranking values")
                .keys()
                .cloned()
                .collect::<std::collections::BTreeSet<_>>(),
            selected_fields
        );
        assert_eq!(
            document["indexed"], ranking,
            "selected native ranking values for {id}"
        );
        let mut metadata = document.as_object().expect("native search record").clone();
        metadata.remove("indexed").expect("separate ranking values");
        assert_eq!(
            Value::Object(metadata),
            Value::Object(expected),
            "all metadata fields for {id}"
        );
        for excluded in [
            "body_html",
            "body_text",
            "opening",
            "presentation",
            "source",
        ] {
            assert!(
                document.get(excluded).is_none(),
                "excluded search field {excluded}"
            );
        }
        let url = document["route"]["canonical_url"]
            .as_str()
            .expect("resolved canonical URL");
        if let Some(prior) = previous {
            assert!(
                prior < url,
                "complete canonical order without duplicate documents"
            );
        }
        previous = Some(url);
        let output = document["route"]["output"]
            .as_str()
            .expect("same primary output");
        let html = std::str::from_utf8(bytes(tree, output)).expect("actual primary HTML");
        assert!(
            html.replace("&#x2F;", "/")
                .contains(&format!("href=\"{url}\""))
        );
    }
}

fn assert_provider_hits(path: &std::path::Path, records: &[Value], queries: &[(&str, &str, &str)]) {
    let script = r#"import {readFileSync} from 'node:fs';
import {createIndex} from './sdk/js/search/index.mjs';
const records=JSON.parse(readFileSync(process.argv[1],'utf8'));
const queries=JSON.parse(process.argv[2]);
const index=createIndex(records,{profile:'lexical',maxDocuments:100,maxBytes:1048576,maxQueryBytes:1024,maxResults:100});
console.log(JSON.stringify({size:index.size,queries:queries.map(([query,field])=>index.search(query,{limit:100,fields:{[field]:1}}))}));"#;
    let output = std::process::Command::new(crate::native::node())
        .args(["--input-type=module", "-e", script])
        .arg(path)
        .arg(serde_json::to_string(queries).expect("exact provider queries"))
        .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .output()
        .expect("actual selected Node search provider");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let proof: Value = serde_json::from_slice(&output.stdout).expect("actual provider response");
    assert_eq!(proof["size"], records.len());
    for (position, (query, _, id)) in queries.iter().enumerate() {
        let hits = proof["queries"][position]
            .as_array()
            .expect("provider hits");
        assert_eq!(hits.len(), 1, "exact owning document for {query}");
        let record = records
            .iter()
            .find(|record| record["content"]["id"] == *id)
            .expect("emitted document");
        assert_eq!(
            &hits[0]["record"], record,
            "provider retains every original metadata and ranking value for {id}"
        );
        assert!(
            hits[0]["score"]
                .as_f64()
                .is_some_and(|score| score.is_finite() && score > 0.0)
        );
    }
}

#[test]
fn search_documents_preserve_language_roles_project_relations_and_exact_staged_metadata() {
    let doc = PROJECT_DOC
        .replace("kind='about'}]", "kind='about'},{project='project:native',kind='mentions'}]")
        .replace("[content]\n", "[content]\nlanguage='fr-ca'\nattributions=[{principal='person:author',role='author'},{principal='organization:rights',role='rights-holder'}]\n");
    let doc = format!(
        "{doc}\nA **searchable** sentence with `inline_code`.\n\n```rust\nlet retained_token = 1;\n```\n"
    );
    let fixture = Fixture::new(&[("project/guide.md", doc.as_bytes())]);
    let config = format!(
        "{}[[principals]]\nid='person:author'\nkind='person'\n[[principals]]\nid='organization:rights'\nkind='organization'\n",
        enable(&fixture.project_config(true)),
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete site with search");
    let tree = rendered.site().output();
    assert_shared_metadata(tree);
    let records = documents(tree);
    let native = records
        .iter()
        .find(|doc| doc["content"]["id"] == "doc:native")
        .expect("native document");
    assert_eq!(native["content"]["language"], "fr-CA");
    assert_eq!(
        native["provenance"],
        serde_json::json!({"site":"site:one","project":"project:native"})
    );
    assert_eq!(native["attributions"][0]["role"], "author");
    assert_eq!(native["attributions"][1]["role"], "rights-holder");
    for (project, relation) in [("project:subject", "about"), ("project:native", "mentions")] {
        let link = native["projects"]
            .as_array()
            .expect("typed project links")
            .iter()
            .find(|link| link["project"]["id"] == project)
            .expect("exact project link");
        assert_eq!(link["relationship"]["relation"], relation);
    }
    assert_eq!(native["collections"][0]["id"], "collection:home");
    assert_eq!(native["related"][0]["id"], "content:other");
    let body = native["body"].as_str().expect("plain retained body");
    assert!(body.contains("A searchable sentence with inline_code."));
    assert!(
        body.contains("let retained_token = 1;"),
        "code text is not silently discarded"
    );
    assert!(!body.contains("<strong>"));
    assert!(!body.contains("```"));
    assert!(
        records
            .iter()
            .all(|doc| doc["content"]["site"] == "site:one")
    );
    assert!(
        records
            .iter()
            .any(|doc| doc["content"]["lifecycle"] == "draft")
    );
    assert!(
        records
            .iter()
            .all(|doc| doc["route"]["canonical_url"] != "https://one.example/mount/old.html")
    );
    let text = std::str::from_utf8(bytes(tree, "search.json")).expect("JSON text");
    assert!(!text.contains(fixture.directory.path().to_str().expect("native root")));
    assert!(!text.contains("source:native"));

    let parent = tempdir().expect("disjoint local stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("native receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let stage = std::path::Path::new(receipt["directory"].as_str().expect("retained local stage"));
    assert_eq!(
        fs::read(stage.join("search.json")).expect("staged search"),
        bytes(tree, "search.json")
    );
    assert_eq!(
        fs::read(stage.join("model.json")).expect("staged model"),
        bytes(tree, "model.json")
    );
    assert_provider_hits(
        &stage.join("search.json"),
        &records,
        &[
            ("Native documentation", "title", "doc:native"),
            ("retained_token", "body", "doc:native"),
        ],
    );
    fs::write(
        fixture.directory.path().join("project/guide.md"),
        "changed after capture",
    )
    .expect("disposable edit");
    let repeated = render(&inputs).expect("same retained source packet");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    assert_eq!(
        bytes(repeated.site().output(), "search.json"),
        bytes(tree, "search.json")
    );
}

#[test]
fn search_switch_is_independent_and_selected_site_owns_its_document_set() {
    let fixture = Fixture::new(&[]);
    for (config, search, sitemap, llms) in [
        (fixture.config(), false, false, false),
        (enable(&fixture.config()), true, false, false),
        (
            fixture.config().replacen(
                "public_base_url=",
                &format!(
                    "machine={{sitemap=true,{}}}\npublic_base_url=",
                    super::curated_llms(
                        "collection:home",
                        &[("Start here", &["collection:home", "content:one"])]
                    )
                ),
                1,
            ),
            false,
            true,
            true,
        ),
    ] {
        let inputs = acquire(&config);
        let result = render(&inputs).expect("independent switches");
        for (path, expected) in [
            ("search.json", search),
            ("sitemap.xml", sitemap),
            ("llms.txt", llms),
        ] {
            assert_eq!(
                result
                    .site()
                    .output()
                    .file(&path.parse().expect("output"))
                    .is_some(),
                expected
            );
        }
    }
    let config = enable(&fixture.config());
    let inputs = build::acquire_publication(
        &config,
        &"site:other".parse().expect("site"),
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("independent site capture");
    let result = render(&inputs).expect("second site search");
    assert_shared_metadata(result.site().output());
    let records = documents(result.site().output());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["content"]["id"], "content:other");
    assert_eq!(
        records[0]["route"]["canonical_url"],
        "https://other.example/elsewhere/"
    );
    assert_eq!(records[0]["content"]["lifecycle"], "archived");
    let directory = tempdir().expect("actual emitted corpus for the native provider");
    let path = directory.path().join("search.json");
    fs::write(&path, bytes(result.site().output(), "search.json"))
        .expect("exact emitted corpus bytes");
    assert_provider_hits(&path, &records, &[("Other", "title", "content:other")]);
}

fn collision_without_stage(config: &str, model: &str) {
    let parent = tempdir().expect("stage parent");
    fs::write(parent.path().join("keep"), b"unrelated").expect("unrelated fixture");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[5] = model;
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let result = cli(config, &arguments);
    assert!(!result.status.success());
    assert!(
        result.stdout.is_empty(),
        "no successful partial stage receipt"
    );
    assert!(
        String::from_utf8_lossy(&result.stderr).contains("source_inventory"),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("unchanged parent")
            .count(),
        1
    );
    assert_eq!(
        fs::read(parent.path().join("keep")).expect("preserved bytes"),
        b"unrelated"
    );
}

#[test]
fn search_collisions_with_model_page_and_static_asset_directory_fail_before_staging() {
    let fixture = Fixture::new(&[]);
    collision_without_stage(&enable(&fixture.config()), "search.json");
    let page = PAGE.replace("path='/guide/'", "path='/search.json'");
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    collision_without_stage(&enable(&fixture.config()), "model.json");
    let fixture = Fixture::new(&[("one/retained.txt", b"retained asset")]);
    let config = enable(&fixture.config()).replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/search.json/'}}]",
    );
    collision_without_stage(&config, "model.json");
}

#[test]
fn search_consumes_complete_inventory_budget_instead_of_returning_a_truncated_index() {
    let fixture = Fixture::new(&[]);
    let baseline_inputs = acquire(&fixture.config());
    let baseline = render(&baseline_inputs).expect("baseline site");
    let search_inputs = acquire(&enable(&fixture.config()));
    let limits = source::Limits {
        entries: baseline.site().output().inventory().entries().len(),
        ..Default::default()
    };
    for (inputs, enabled) in [(&baseline_inputs, false), (&search_inputs, true)] {
        let result = build::render_publication(
            inputs,
            build::SiteRenderOptions {
                render_year: None,
                model_output: Some(&"model.json".parse().expect("model")),
                graph: None,
                limits,
            },
            Default::default(),
        );
        if enabled {
            assert_eq!(
                result
                    .expect_err("no complete output under entry budget")
                    .code(),
                "source_limit"
            );
        } else {
            result.expect("baseline fits the exact same entry ceiling");
        }
    }
}
