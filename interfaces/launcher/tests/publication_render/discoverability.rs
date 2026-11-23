//! Discovery selection never substitutes for source admission or page materialization.

use std::{collections::BTreeMap, path::Path};

use gazette_sdk::{process, workspace::Key};
use serde_json::json;

use super::{
    Fixture, OTHER, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, Value, acquire, build, bytes, cli,
    curated_llms, fs, policy, source, tempdir, toml,
};

const TITLE: &str = "Placement-only documentation title";
const BODY: &str = "Placement-only retained documentation body";
const URL: &str = "https://one.example/mount/docs/native/";
const TEMPLATE: &[u8] = br#"<h1>{{ content.title }}</h1>{{ body_html | safe }}{% for item in children %}<b data-child="{{ item.id }}">{{ item.title }}</b>{% endfor %}{% for item in steps %}<b data-step="{{ item.id }}">{{ item.title }}</b>{% endfor %}{% for item in related %}<b data-related="{{ item.id }}">{{ item.title }}</b>{% endfor %}"#;

fn fixture() -> Fixture {
    let doc = PROJECT_DOC
        .replace("collections=['collection:home']\n", "")
        .replace("Native documentation", TITLE)
        .replace("# Retained native documentation", BODY);
    let page = format!(
        "{}\n[Deliberate direct link]({URL})\n",
        PAGE.replace("collections=['collection:home']\n", "")
    );
    let other = format!(
        "{}\n[Deliberate cross-site link]({URL})\n",
        OTHER.replacen("+++\n", "+++\nrelated=['doc:native']\n", 1)
    );
    let journey = "+++\n[content]\nid='journey:explicit'\nsite='site:one'\ntitle='Explicit authored traversal'\nlifecycle='published'\nroute={path='/journey/'}\nstructure={kind='journey',collection='collection:home',members=[{content='doc:native',primary=true},{content='content:one',primary=true}]}\n+++\nAuthored traversal body.\n";
    Fixture::new(&[
        ("project/guide.md", doc.as_bytes()),
        ("one/page.md", page.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("one/journey.md", journey.as_bytes()),
        ("theme/templates/page.html", TEMPLATE),
        ("theme/templates/graph.json", b"{{ __tera_context }}"),
    ])
}

fn configured(fixture: &Fixture, discoverable: bool) -> String {
    let contents: &[&str] = if discoverable {
        &["collection:home", "content:one", "doc:native"]
    } else {
        &["collection:home", "content:one"]
    };
    let one = curated_llms("collection:home", &[("Selected reading", contents)]);
    let other = curated_llms("content:other", &[("Documentation", &["content:other"])]);
    fixture.project_config(true)
        .replace("discoverable=true", &format!("discoverable={discoverable}"))
        .replacen("public_base_url=", &format!("machine={{json_ld=true,search='lexical',sitemap=true,{one},feeds={{identity_base='https://identity.example/',channels=[{{id='feed:all',channel='collection:home',path='/feed.json',format='json',scope={{kind='site'}}}}]}}}}\npublic_base_url="), 1)
        .replace("public_base_url='https://other.example/'", &format!("machine={{json_ld=true,search='lexical',sitemap=true,{other}}}\npublic_base_url='https://other.example/'"))
}

fn options<'a>(
    _page: &'a source::EntryPath,
    _alias: &'a source::EntryPath,
    model: &'a source::EntryPath,
    graph: &'a source::EntryPath,
) -> build::SiteRenderOptions<'a> {
    build::SiteRenderOptions {
        render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
        model_output: Some(model),
        graph: Some(build::GraphRenderOptions {
            output: graph,
            limits: Default::default(),
        }),
        limits: Default::default(),
    }
}

fn render(inputs: &build::AcquiredInputs) -> build::PublicationRender<'_> {
    build::render_publication(
        inputs,
        options(
            &"page.html".parse().expect("page"),
            &"alias.html".parse().expect("alias"),
            &"model.json".parse().expect("model"),
            &"graph.json".parse().expect("graph"),
        ),
        Default::default(),
    )
    .expect("complete retained discovery and pages")
}

fn model(tree: &source::CapturedSource) -> Value {
    serde_json::from_slice(bytes(tree, "model.json")).expect("actual shared site model")
}

pub(super) fn assert_public_json_ld(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                assert!(
                    !key.starts_with("gazette:"),
                    "native field in public JSON-LD: {key}"
                );
                if key == "@type" {
                    let values = child
                        .as_array()
                        .map_or_else(|| vec![child], |values| values.iter().collect());
                    for kind in values {
                        assert!(
                            !kind
                                .as_str()
                                .is_some_and(|kind| kind.starts_with("gazette:")),
                            "native type in public JSON-LD: {kind}"
                        );
                    }
                }
                assert_public_json_ld(child);
            }
            assert!(
                fields
                    .get("@context")
                    .and_then(Value::as_object)
                    .is_none_or(|context| !context.contains_key("gazette"))
            );
        }
        Value::Array(values) => values.iter().for_each(assert_public_json_ld),
        _ => {}
    }
}

fn assert_discovery(tree: &source::CapturedSource, visible: bool, curated_document: bool) {
    let model = model(tree);
    assert_eq!(
        model["pages"]
            .as_object()
            .expect("page map")
            .contains_key("doc:native"),
        visible
    );
    let graph: Value =
        serde_json::from_slice(bytes(tree, "graph.json")).expect("real Tera graph projection");
    assert_eq!(
        graph["references"]["content:other"]["presentation"]["theme"], "theme:one",
        "foreign graph endpoints carry destination presentation without copying their record"
    );
    assert_eq!(
        graph["nodes"]
            .as_object()
            .expect("graph nodes")
            .contains_key("doc:native"),
        visible
    );
    assert_eq!(
        graph["routes"]
            .as_object()
            .expect("graph routes")
            .contains_key("doc:native"),
        visible
    );
    let search: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("actual search records");
    let linked: Value =
        serde_json::from_slice(bytes(tree, "site.jsonld")).expect("actual JSON-LD index");
    assert_public_json_ld(&linked);
    for page in model["pages"].as_object().expect("page map").values() {
        if let Some(linked) = page.get("json_ld") {
            assert_public_json_ld(linked);
        }
    }
    assert_eq!(
        linked["@graph"]
            .as_array()
            .expect("linked records")
            .iter()
            .any(|record| record["identifier"] == "doc:native"),
        visible
    );
    let journey = linked["@graph"]
        .as_array()
        .expect("linked records")
        .iter()
        .find(|record| record["identifier"] == "journey:explicit")
        .expect("listed journey");
    let members = journey["mainEntity"]["itemListElement"]
        .as_array()
        .expect("positioned eligible journey members");
    assert_eq!(members.len(), if visible { 2 } else { 1 });
    assert_eq!(
        members.last().expect("retained second member")["position"],
        2
    );
    if !visible {
        assert!(
            !journey.to_string().contains("doc:native"),
            "no hidden structural endpoint through technical breadcrumbs"
        );
        assert!(!linked.to_string().contains(TITLE));
    }
    assert_eq!(
        search
            .iter()
            .any(|item| item["content"]["id"] == "doc:native"),
        visible
    );
    let feed: Value = serde_json::from_slice(bytes(tree, "feed.json")).expect("actual JSON Feed");
    assert_eq!(
        feed["items"]
            .as_array()
            .expect("feed items")
            .iter()
            .any(|item| item["_gazette"]["content"]["id"] == "doc:native"),
        visible
    );
    for (path, needle) in [
        ("sitemap.xml", format!("<loc>{URL}</loc>")),
        ("llms.txt", format!("]({URL})")),
    ] {
        assert_eq!(
            std::str::from_utf8(bytes(tree, path))
                .expect("discovery text")
                .contains(&needle),
            if path == "llms.txt" {
                curated_document
            } else {
                visible
            },
            "{path}"
        );
    }
    // Authored literal links and journey declarations remain authored meaning.
    // Only derived records/cards are removed; blanket text redaction would be wrong.
    assert!(
        std::str::from_utf8(bytes(tree, "guide/index.html"))
            .expect("direct authored page")
            .contains(URL)
    );
    assert!(
        model["pages"]["content:one"]["body_text"]
            .as_str()
            .expect("retained listing text")
            .contains("Deliberate direct link")
    );
    let journey =
        std::str::from_utf8(bytes(tree, "journey/index.html")).expect("direct authored journey");
    let native_step = journey
        .find("data-step=\"doc:native\"")
        .expect("explicit hidden member remains on its own journey page");
    let next_step = journey
        .find("data-step=\"content:one\"")
        .expect("explicit following member");
    assert!(
        native_step < next_step,
        "authored traversal order remains intact"
    );
    assert!(journey.contains(TITLE));
    let children = model["pages"]["collection:home"]["children"]
        .as_array()
        .expect("automatic direct collection children");
    assert_eq!(children.len(), 1, "pages reside only in their journey");
    assert_eq!(children[0]["id"], "journey:explicit");
    assert_eq!(
        members
            .iter()
            .any(|member| member["item"]["identifier"] == "doc:native"),
        visible,
        "discovery filters the document at its actual journey residence"
    );
    if !visible {
        for record in model["pages"]
            .as_object()
            .expect("automatic pages")
            .values()
        {
            assert!(
                !record.to_string().contains(TITLE),
                "no suppressed title through automatic related/step cards"
            );
            assert!(!record.to_string().contains(BODY));
        }
        assert!(!graph["references"].to_string().contains(URL));
    }
    let html = std::str::from_utf8(bytes(tree, "docs/native/index.html"))
        .expect("rendered unlisted document");
    assert!(html.contains(TITLE));
    assert!(html.contains(BODY));
}

#[test]
fn placement_flag_controls_all_automatic_outputs_but_not_rendered_or_authored_meaning() {
    let fixture = fixture();
    let config = configured(&fixture, false);
    let inputs = acquire(&config);
    let rendered = render(&inputs);
    assert_discovery(rendered.site().output(), false, false);
    assert!(
        rendered
            .site()
            .evidence()
            .pages
            .contains_key(&"doc:native".parse().expect("retained document evidence"))
    );
    let parent = tempdir().expect("disjoint stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--graph-output",
        "graph.json",
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("native stage receipt");
    assert_eq!(
        receipt["inventory"],
        json!(rendered.site().output().inventory())
    );
    let stage = Path::new(receipt["directory"].as_str().expect("exact retained stage"));
    for entry in rendered.site().output().inventory().entries() {
        if let Some(retained) = rendered.site().output().file(&entry.path) {
            assert_eq!(
                fs::read(stage.join(entry.path.as_str())).expect("complete stage file"),
                retained
            );
        }
    }
    fs::write(
        fixture.directory.path().join("project/guide.md"),
        "changed after exact capture",
    )
    .expect("disposable source edit");
    let repeated = render(&inputs);
    assert_eq!(
        repeated.site().output().inventory().identity(),
        rendered.site().output().inventory().identity()
    );
    assert_discovery(repeated.site().output(), false, false);
}

#[test]
fn exact_flag_flip_changes_discovery_and_checker_request_without_project_state_inference() {
    let fixture = fixture();
    let observation = tempdir().expect("private checker observation");
    let result = observation.path().join("received.json");
    let (policy_config, _) = policy::configured(&fixture, &result, 0);
    let base = fixture.config();
    let prefix = policy_config
        .strip_suffix(&base)
        .expect("reuse exact native checker declaration only");
    let hidden = format!("{prefix}{}", configured(&fixture, false));
    let visible = hidden.replace("discoverable=false", "discoverable=true");
    let hidden_inputs = acquire(&hidden);
    let visible_inputs = acquire(&visible);
    let hidden_render = render(&hidden_inputs);
    let visible_render = render(&visible_inputs);
    assert_discovery(hidden_render.site().output(), false, false);
    assert_discovery(visible_render.site().output(), true, false);
    assert_eq!(
        bytes(hidden_render.site().output(), "llms.txt"),
        bytes(visible_render.site().output(), "llms.txt"),
        "discoverability alone never enrolls a page in curated sections"
    );
    let curated = format!("{prefix}{}", configured(&fixture, true));
    let curated_inputs = acquire(&curated);
    let curated_render = render(&curated_inputs);
    assert_discovery(curated_render.site().output(), true, true);
    assert!(
        std::str::from_utf8(bytes(curated_render.site().output(), "llms.txt"))
            .expect("curated Markdown/HTML pairs")
            .contains("](https://one.example/mount/docs/native/index.md)"),
        "explicit enrollment links the materialized Markdown counterpart"
    );
    assert!(
        std::str::from_utf8(bytes(
            curated_render.site().output(),
            "docs/native/index.md"
        ))
        .expect("actual retained Markdown counterpart")
        .contains(BODY)
    );
    assert_ne!(
        bytes(visible_render.site().output(), "llms.txt"),
        bytes(curated_render.site().output(), "llms.txt"),
        "explicit curation changes its own output"
    );
    assert_eq!(
        bytes(hidden_render.site().output(), "docs/native/index.html"),
        bytes(visible_render.site().output(), "docs/native/index.html")
    );
    for path in [
        "model.json",
        "search.json",
        "feed.json",
        "sitemap.xml",
        "graph.json",
        "site.jsonld",
    ] {
        assert_ne!(
            bytes(hidden_render.site().output(), path),
            bytes(visible_render.site().output(), path),
            "actual changed discovery bytes {path}"
        );
    }
    let mut requests = Vec::new();
    for rendered in [&hidden_render, &visible_render] {
        let check = build::check_output_policy(
            rendered,
            process::Options {
                allow_execute: true,
                ..Default::default()
            },
            None,
        )
        .expect("real native output checker");
        let received = fs::read(&result).expect("actual checker input");
        assert_eq!(
            check.invocation().request_sha256,
            source::Sha256Digest::of(&received)
        );
        requests.push(received);
    }
    assert_ne!(requests[0], requests[1]);
    // The local pipeline is not public-policy admission: project state cannot replace the flag.
    let private = visible.replace(
        "lifecycle='active'\nvisibility='public'\navailability='available'",
        "lifecycle='done'\nvisibility='private'\navailability='unavailable'",
    );
    let private_inputs = acquire(&private);
    assert_discovery(render(&private_inputs).site().output(), true, false);
}

#[test]
fn mixed_site_set_keeps_cross_site_authored_links_without_promoting_hidden_references() {
    let fixture = fixture();
    let config = configured(&fixture, false);
    let selected: Vec<Key> = ["site:one", "site:other"]
        .map(|id| id.parse().expect("site"))
        .into();
    let inputs = build::acquire_publications(
        &config,
        &selected,
        toml::Limits::default(),
        Default::default(),
    )
    .expect("complete two-site source set");
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model_path = "model.json".parse().expect("model");
    let graph_path = "graph.json".parse().expect("graph");
    let rendered = build::render_publications(
        &inputs,
        build::PublicationSetRenderOptions {
            derive_selection: false,
            preview_link_bases: None,
            sites: selected
                .iter()
                .cloned()
                .map(|id| (id, options(&page, &alias, &model_path, &graph_path)))
                .collect::<BTreeMap<_, _>>(),
            selection: None,
            preview_urls: None,
            review_responses: None,
        },
        Default::default(),
    )
    .expect("shared complete semantic set");
    assert!(rendered.is_complete());
    assert_eq!(rendered.sites().len(), 2);
    let one = rendered.sites()[&selected[0]]
        .as_ref()
        .expect("first complete site");
    assert_discovery(one.site().output(), false, false);
    let other = rendered.sites()[&selected[1]]
        .as_ref()
        .expect("second complete site");
    let model = model(other.site().output());
    assert!(
        std::str::from_utf8(bytes(other.site().output(), "elsewhere/index.html"))
            .expect("direct cross-site authored page")
            .contains(URL)
    );
    assert!(
        model["pages"]["content:other"]["body_text"]
            .as_str()
            .expect("cross-site listing text")
            .contains("Deliberate cross-site link")
    );
    assert!(
        model["pages"]["content:other"]["related"]
            .as_array()
            .expect("automatic related cards")
            .is_empty()
    );
    let graph: Value =
        serde_json::from_slice(bytes(other.site().output(), "graph.json")).expect("foreign graph");
    assert!(graph["references"].get("doc:native").is_none());
    let request = json!({"format":"gazette-local-publication-set/1","render_year":2040,"workspace":{"kind":"supplied","source":config},"sites":selected.iter().map(|site| json!({"site":site,"model_output":"model.json","graph":{"output":"graph.json"}})).collect::<Vec<_>>()});
    let output = cli(&request.to_string(), &["render-sites", "--local"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("complete local set evidence");
    assert_eq!(
        report["sites"]
            .as_object()
            .expect("same exact selection")
            .len(),
        2
    );
    for site in selected {
        assert_eq!(report["sites"][site.as_str()]["status"], "rendered");
        assert_eq!(
            report["sites"][site.as_str()]["inventory"],
            json!(
                rendered.sites()[&site]
                    .as_ref()
                    .expect("complete SDK site")
                    .site()
                    .output()
                    .inventory()
            )
        );
    }
}

#[test]
fn suppressed_discovery_cannot_turn_missing_selected_source_into_a_successful_subset() {
    let fixture = fixture();
    let valid = configured(&fixture, false);
    let _control = acquire(&valid);
    let config = valid.replace("subdirectory='project'", "subdirectory='missing-project'");
    let refused = build::acquire_publication(
        &config,
        &"site:one".parse().expect("site"),
        toml::Limits::default(),
        source::PreparationLimits {
            sources: 8,
            entries: 128,
            bytes: 1024 * 1024,
            per_source: Default::default(),
        },
    )
    .expect_err("missing explicitly required project source");
    let parent = tempdir().expect("preserved stage parent");
    fs::write(parent.path().join("canary"), b"unchanged").expect("existing independent bytes");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let output = cli(&config, &arguments);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(refused.code()),
        "SDK and CLI preserve the actual source refusal class"
    );
    assert_eq!(
        fs::read(parent.path().join("canary")).expect("preserved file"),
        b"unchanged"
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("no partial stage")
            .count(),
        1
    );
}
