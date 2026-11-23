//! One compiler-owned JSON-LD value across the actual Theme and site outputs.

use std::convert::Infallible;

use gazette_sdk::html::{self, TextKind};
use serde_json::Value;

#[path = "json_ld/lineage.rs"]
mod lineage;

use super::{
    Fixture, PAGE, PROJECT_DOC, RENDER_ARGUMENTS, acquire, build, bytes, cli, fs, render, source,
    tempdir, toml,
};

const TEMPLATE: &[u8] = b"<!doctype html><html><head><title>Local fixture</title>{% if json_ld is defined %}<script type=\"application/ld+json\">{{ json_ld | json_script }}</script>{% endif %}</head><body><h1>{{ content.title }}</h1>{{ body_html | safe }}<p>after-json-ld</p></body></html>";

#[test]
fn project_names_reach_html_machine_and_registry_views_from_one_exact_catalog() {
    let name = "Project Ω <&>";
    let repository = "https://code.example.test/Group/Library";
    let license = "MIT OR Apache-2.0";
    let acquisition_origin = "https://acquisition.example.test/not-publication";
    let template = String::from_utf8(TEMPLATE.to_vec()).expect("template")
        .replace("<h1>", "{% for link in projects %}{% if link.project.name is defined %}<b>{{ link.project.name }}</b>{% endif %}{% if link.project.repository is defined %}<code>{{ link.project.repository }}</code>{% endif %}{% if link.project.license is defined %}<i>{{ link.project.license }}</i>{% endif %}{% endfor %}<h1>");
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", template.as_bytes()),
        ("project/guide.md", PROJECT_DOC.as_bytes()),
    ]);
    let native =
        git2::Repository::open(fixture.directory.path()).expect("native acquisition repository");
    native
        .remote("origin", acquisition_origin)
        .expect("disposable acquisition origin");
    assert_eq!(native.head().expect("HEAD").target(), Some(fixture.commit));
    let config = fixture
        .project_config(true)
        .replace(
            "id='project:subject'",
            &format!("id='project:subject'\nname={name:?}\nrepository={repository:?}\nlicense={license:?}"),
        )
        .replace(
            "public_base_url=",
            "machine={json_ld=true,search='lexical'}\npublic_base_url=",
        );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("named project publication");
    let tree = rendered.site().output();
    let model = assert_connected(&inputs, tree);
    let project = &model["pages"]["doc:native"]["projects"][0]["project"];
    assert_eq!(project["id"], "project:subject");
    assert_eq!(project["name"], name);
    assert_eq!(project["repository"], repository);
    assert_eq!(project["license"], license);
    assert_eq!(
        model["pages"]["doc:native"]["json_ld"]["about"][0]["name"],
        name
    );
    let public_project = &model["pages"]["doc:native"]["json_ld"]["about"][0];
    assert_eq!(public_project["@type"], "Thing");
    assert_eq!(public_project["identifier"], project["id"]);
    assert!(public_project.get("codeRepository").is_none());
    assert!(public_project.get("license").is_none());
    assert!(
        std::str::from_utf8(bytes(tree, "docs/native/index.html"))
            .expect("HTML")
            .contains("<b>Project Ω &lt;&amp;&gt;</b>")
    );
    // Native Tera HTML autoescaping also encodes forward slashes.
    assert!(
        std::str::from_utf8(bytes(tree, "docs/native/index.html"))
            .expect("HTML")
            .contains(&format!(
                "<code>{}</code>",
                repository.replace('/', "&#x2F;")
            ))
    );
    assert!(
        std::str::from_utf8(bytes(tree, "docs/native/index.html"))
            .expect("HTML")
            .contains("<i>MIT OR Apache-2.0</i>")
    );
    let search: Vec<Value> = serde_json::from_slice(bytes(tree, "search.json")).expect("search");
    let doc = search
        .iter()
        .find(|record| record["content"]["id"] == "doc:native")
        .expect("same document");
    assert_eq!(doc["projects"][0]["project"], *project);
    let parsed = build::parse_publication(&inputs, Default::default()).expect("retained parser");
    let resolved = parsed.resolve(Default::default()).expect("semantic owner");
    let graph = serde_json::to_value(
        resolved
            .authored()
            .project_site_graph(&"site:one".parse().expect("site"), Default::default())
            .expect("graph"),
    )
    .expect("graph JSON");
    assert_eq!(graph["projects"]["project:subject"], *project);
    assert!(
        graph["projects"]["project:native"].get("name").is_none(),
        "no repository/title fallback"
    );
    assert!(
        graph["projects"]["project:native"]
            .get("repository")
            .is_none(),
        "acquisition Git origin is not publication metadata"
    );
    assert!(
        graph["projects"]["project:native"].get("license").is_none(),
        "an absent project licence has no source or content fallback"
    );
    assert!(
        !serde_json::to_string(&model)
            .expect("model JSON")
            .contains(acquisition_origin)
    );
    let registry = resolved
        .registry(Default::default())
        .expect("same registry");
    let registry_model = serde_json::to_value(registry.model()).expect("registry JSON");
    assert_eq!(registry_model["projects"]["project:subject"], *project);
    assert!(
        registry_model["projects"]["project:native"]
            .get("repository")
            .is_none()
    );
    assert!(
        registry_model["projects"]["project:native"]
            .get("license")
            .is_none()
    );
    let output = cli(&config, &RENDER_ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("native report");
    assert_eq!(
        report["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
}

fn enable(config: &str) -> String {
    config.replace(
        "public_base_url=",
        "machine={json_ld=true}\npublic_base_url=",
    )
}

fn script_values(bytes: &[u8]) -> Vec<Value> {
    let mut result = Vec::new();
    html::inspect(bytes, html::Limits::default(), |kind, value| {
        match kind {
            TextKind::JsonScript => result.push(
                serde_json::from_str(value)
                    .expect("one complete JSON value inside its HTML script boundary"),
            ),
            TextKind::Script | TextKind::ModuleScript | TextKind::DataScript => {
                panic!("JSON-LD data created unexpected script text")
            }
            TextKind::Style => panic!("JSON-LD data created CSS text"),
            _ => (),
        }
        Ok::<_, Infallible>(())
    })
    .expect("native HTML tokenizer consumes complete output");
    result
}

pub(super) fn assert_public(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    key != "gazette" && !key.starts_with("gazette:"),
                    "private vocabulary in public linked data: {key}"
                );
                if key == "@type" {
                    assert!(
                        !value
                            .as_str()
                            .is_some_and(|kind| kind.starts_with("gazette:"))
                    );
                }
                assert_public(value);
            }
        }
        Value::Array(values) => values.iter().for_each(assert_public),
        _ => {}
    }
}

fn assert_connected(inputs: &build::AcquiredInputs, tree: &source::CapturedSource) -> Value {
    let parsed = build::parse_publication(inputs, Default::default()).expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same native semantic owner");
    let authored = resolved.authored();
    let discovery = authored.discovery().expect("exact configured discovery");
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("site model");
    let graph: Value = serde_json::from_slice(bytes(tree, "site.jsonld")).expect("site JSON-LD");
    assert_eq!(graph.as_object().expect("graph envelope").len(), 1);
    let nodes = graph["@graph"]
        .as_array()
        .expect("complete canonical graph array");
    let mut pages: Vec<_> = model["pages"]
        .as_object()
        .expect("page model map")
        .values()
        .collect();
    pages.sort_by(|a, b| {
        a["route"]["canonical_url"]
            .as_str()
            .expect("URL")
            .cmp(b["route"]["canonical_url"].as_str().expect("URL"))
    });
    assert_eq!(
        nodes.len(),
        pages.len(),
        "no omitted, aliased or foreign top-level pages"
    );
    for (node, page) in nodes.iter().zip(pages) {
        let id = page["content"]["id"]
            .as_str()
            .expect("content identity")
            .parse()
            .expect("native key");
        let projection = authored
            .project_discovery_page(
                &id,
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect("same direct page and generated navigation");
        let expected_listing = serde_json::to_value(
            projection
                .listing_json_ld(&discovery)
                .expect("eligible listing")
                .public(),
        )
        .expect("native aggregate JSON-LD");
        assert_eq!(
            node, &expected_listing,
            "aggregate uses the public listing projection"
        );
        assert_public(node);
        assert_eq!(
            node, &page["json_ld"],
            "site transport must retain exact compiler value"
        );
        let path = page["route"]["output"]
            .as_str()
            .expect("compiled primary output");
        assert_eq!(
            script_values(bytes(tree, path)),
            [serde_json::to_value(
                projection
                    .discovery_json_ld(&discovery)
                    .expect("source-bound direct role")
                    .public()
            )
            .expect("native direct JSON-LD")],
            "HTML retains the native direct-page JSON-LD role at {path}"
        );
        assert!(
            std::str::from_utf8(bytes(tree, path))
                .expect("HTML")
                .contains("after-json-ld")
        );
    }
    model
}

#[test]
fn json_ld_crosses_native_cli_sdk_html_and_site_transport_with_retained_parity() {
    let title = "</ScRiPt ><script>injected()</script><!--<script>--> Ω & \" quoted";
    let doc = PROJECT_DOC
        .replace(
            "title='Native documentation'",
            &format!(
                "title={}",
                serde_json::to_string(title).expect("TOML string")
            ),
        )
        .replace(
            "kind='about'}]",
            "kind='about'},{project='project:native',kind='mentions'}]",
        )
        .replace("[content]\n", "[content]\nlanguage='fr-ca'\n");
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("project/guide.md", doc.as_bytes()),
    ]);
    let config = enable(&fixture.project_config(true));
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("connected local JSON-LD output");
    let tree = rendered.site().output();
    let model = assert_connected(&inputs, tree);
    assert_eq!(model["pages"].as_object().expect("selected pages").len(), 3);
    assert!(model["pages"].get("content:other").is_none());
    assert_eq!(
        model["pages"]["content:one"]["content"]["lifecycle"],
        "draft"
    );
    assert_eq!(model["pages"]["doc:native"]["content"]["title"], title);
    assert_eq!(model["pages"]["doc:native"]["content"]["language"], "fr-CA");
    assert!(
        script_values(bytes(tree, "old.html")).is_empty(),
        "aliases are not independent JSON-LD documents"
    );
    assert!(
        !std::str::from_utf8(bytes(tree, "site.jsonld"))
            .expect("JSON-LD text")
            .contains(fixture.directory.path().to_str().expect("native root"))
    );

    let parent = tempdir().expect("local stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: Value = serde_json::from_slice(&output.stdout).expect("CLI stage receipt");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for path in ["site.jsonld", "model.json", "docs/native/index.html"] {
        assert_eq!(
            fs::read(directory.join(path)).expect("native read-back"),
            bytes(tree, path)
        );
    }
    fs::write(
        fixture.directory.path().join("project/guide.md"),
        "changed native doc",
    )
    .expect("disposable source edit");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "changed template",
    )
    .expect("disposable Theme edit");
    let repeated = render(&inputs).expect("retained inputs only");
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
fn json_ld_selection_is_independent_and_second_site_does_not_absorb_foreign_pages() {
    let fixture = Fixture::new(&[("theme/templates/page.html", TEMPLATE)]);
    for config in [
        fixture.config(),
        fixture.config().replacen(
            "public_base_url=",
            &format!(
                "machine={{json_ld=false,search='lexical',sitemap=true,{}}}\npublic_base_url=",
                super::curated_llms(
                    "collection:home",
                    &[("Start here", &["collection:home", "content:one"])]
                )
            ),
            1,
        ),
    ] {
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("JSON-LD omitted independently");
        let tree = rendered.site().output();
        assert!(tree.file(&"site.jsonld".parse().expect("output")).is_none());
        let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("model");
        for page in model["pages"].as_object().expect("pages").values() {
            assert!(page.get("json_ld").is_none());
            assert!(
                script_values(bytes(
                    tree,
                    page["route"]["output"].as_str().expect("output")
                ))
                .is_empty()
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
    let rendered = render(&inputs).expect("other site local output");
    let model = assert_connected(&inputs, rendered.site().output());
    assert_eq!(model["pages"].as_object().expect("local page set").len(), 1);
    assert_eq!(
        model["pages"]["content:other"]["content"]["lifecycle"],
        "archived"
    );
    for path in ["sitemap.xml", "llms.txt", "search.json"] {
        assert!(
            rendered
                .site()
                .output()
                .file(&path.parse().expect("unselected output"))
                .is_none()
        );
    }
}

fn collision(config: &str, model: &str) {
    let parent = tempdir().expect("stage parent");
    fs::write(parent.path().join("keep"), b"unrelated").expect("sentinel");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[5] = model;
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let output = cli(config, &arguments);
    assert!(!output.status.success());
    assert!(
        output.stdout.is_empty(),
        "no successful partial stage receipt"
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("source_inventory"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("unchanged parent")
            .count(),
        1
    );
    assert_eq!(
        fs::read(parent.path().join("keep")).expect("sentinel"),
        b"unrelated"
    );
}

#[test]
fn site_json_ld_collisions_fail_the_actual_cli_before_any_stage() {
    let fixture = Fixture::new(&[("theme/templates/page.html", TEMPLATE)]);
    collision(&enable(&fixture.config()), "site.jsonld");
    let page = PAGE.replace("path='/guide/'", "path='/site.jsonld'");
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("one/page.md", page.as_bytes()),
    ]);
    collision(&enable(&fixture.config()), "model.json");
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("one/asset.txt", b"retained asset"),
    ]);
    let config = enable(&fixture.config()).replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/site.jsonld/'}}]",
    );
    collision(&config, "model.json");
}

#[test]
fn json_ld_transport_uses_shared_inventory_limits_without_a_partial_successful_tree() {
    let fixture = Fixture::new(&[("theme/templates/page.html", TEMPLATE)]);
    let inputs = acquire(&enable(&fixture.config()));
    let baseline = render(&inputs).expect("complete baseline");
    let count = baseline.site().output().inventory().entries().len();
    for (entries, passes) in [(count, true), (count - 1, false), (count, true)] {
        let result = build::render_publication(
            &inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits: source::Limits {
                    entries,
                    ..Default::default()
                },
            },
            Default::default(),
        );
        if passes {
            assert_eq!(
                result
                    .expect("exact bound or recovery")
                    .site()
                    .output()
                    .inventory()
                    .identity(),
                baseline.site().output().inventory().identity()
            );
        } else {
            assert_eq!(
                result.expect_err("one below complete inventory").code(),
                "source_limit"
            );
        }
    }
}

#[test]
fn json_ld_is_charged_to_the_exact_complete_page_context_and_failure_does_not_poison_reuse() {
    fn render_at(
        inputs: &build::AcquiredInputs,
        context_bytes: usize,
    ) -> Result<build::PublicationRender<'_>, build::PublicationRenderError> {
        let mut limits = build::PublicationRenderLimits::default();
        limits.output.render.context_bytes = context_bytes;
        build::render_publication(
            inputs,
            build::SiteRenderOptions {
                render_year: Some(
                    gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"),
                ),
                graph: None,
                model_output: Some(&"model.json".parse().expect("model")),
                limits: source::Limits::default(),
            },
            limits,
        )
    }
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("theme/templates/context.json", b"{{ __tera_context }}"),
    ]);
    let inputs = acquire(&enable(&fixture.config()));
    let baseline = render(&inputs).expect("complete JSON-LD context baseline");
    let model = assert_connected(&inputs, baseline.site().output());
    let context_manifest = super::MANIFEST
        .replace("page='page.html'", "page='context.json'")
        .replace("alias='alias.html'", "alias='context.json'");
    let observed_fixture = Fixture::new(&[
        ("theme/theme.toml", context_manifest.as_bytes()),
        ("theme/templates/context.json", b"{{ __tera_context }}"),
    ]);
    let observed_inputs = acquire(&enable(&observed_fixture.config()));
    let observed = build::render_publication(
        &observed_inputs,
        build::SiteRenderOptions {
            render_year: Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year")),
            graph: None,
            model_output: Some(&"model.json".parse().expect("model")),
            limits: source::Limits::default(),
        },
        Default::default(),
    )
    .expect("Tera exposes its actual emitted direct context");
    let page_required = model["pages"]
        .as_object()
        .expect("all fixture pages discoverable")
        .values()
        .map(|page| {
            let path = page["route"]["output"]
                .as_str()
                .expect("exact output route");
            let context: Value = serde_json::from_slice(bytes(observed.site().output(), path))
                .expect("native Tera context");
            let compact = serde_json::to_vec(&context).expect("exact compact context");
            let id: gazette_sdk::workspace::Key = page["content"]["id"]
                .as_str()
                .expect("content identity")
                .parse()
                .expect("native key");
            assert_eq!(
                baseline.site().evidence().pages[&id].model_sha256,
                source::Sha256Digest::of(&compact),
                "probe and actual HTML receive the exact same direct model"
            );
            compact.len()
        })
        .max()
        .expect("nonempty selected site");
    let alias_required = model["pages"]
        .as_object()
        .expect("all fixture pages discoverable")
        .values()
        .flat_map(|page| {
            page["route"]["aliases"]
                .as_array()
                .expect("resolved route aliases")
        })
        .map(|alias| {
            let path = alias["output"].as_str().expect("exact alias output");
            let context: Value = serde_json::from_slice(bytes(observed.site().output(), path))
                .expect("native Tera alias context");
            serde_json::to_vec(&context)
                .expect("exact compact alias context")
                .len()
        })
        .max()
        .unwrap_or(0);
    let required = page_required.max(alias_required);
    for (ceiling, passes) in [(required, true), (required - 1, false), (required, true)] {
        let result = render_at(&inputs, ceiling);
        if passes {
            let result = result.expect("exact complete context ceiling or recovery");
            assert_eq!(
                result.site().output().inventory().identity(),
                baseline.site().output().inventory().identity()
            );
        } else {
            assert_eq!(
                result
                    .expect_err("context is exactly one byte larger than the ceiling")
                    .code(),
                "render_limit"
            );
        }
    }
    let disabled = acquire(&fixture.config());
    render_at(&disabled, required - 1)
        .expect("same content without JSON-LD fits the smaller ceiling");
}

#[test]
fn simultaneous_search_and_json_ld_share_metadata_without_nesting_json_ld_in_search() {
    let fixture = Fixture::new(&[
        ("theme/templates/page.html", TEMPLATE),
        ("project/guide.md", PROJECT_DOC.as_bytes()),
    ]);
    let config = enable(&fixture.project_config(true)).replace(
        "machine={json_ld=true}",
        "machine={json_ld=true,search='lexical'}",
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("both projections in one actual local render");
    let tree = rendered.site().output();
    let model = assert_connected(&inputs, tree);
    let parsed = build::parse_publication(&inputs, Default::default()).expect("retained syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("same native semantic owner");
    let discovery = resolved
        .authored()
        .discovery()
        .expect("configured eligibility");
    let search_selection = inputs
        .compilation()
        .workspace
        .publication_for(inputs.site())
        .and_then(|publication| publication.machine.search.as_ref())
        .expect("exact configured search projection");
    let records: Vec<Value> =
        serde_json::from_slice(bytes(tree, "search.json")).expect("actual search documents");
    assert_eq!(
        records.len(),
        model["pages"].as_object().expect("complete page set").len()
    );
    for record in &records {
        let id = record["content"]["id"]
            .as_str()
            .expect("retained content identity");
        let mut projection = resolved
            .authored()
            .project_discovery_page(
                &id.parse().expect("native content key"),
                &gazette_sdk::raster::inspect,
                &gazette_adapter_markdown::Comrak,
                &discovery,
            )
            .expect("same projected content");
        projection
            .data
            .site
            .metadata
            .as_mut()
            .expect("configured publication metadata")
            .render_year =
            Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"));
        let expected = serde_json::to_value(
            projection
                .listing(&discovery)
                .expect("eligible listing")
                .search_with(search_selection)
                .expect("eligible configured ranking and metadata"),
        )
        .expect("native search role");
        assert_eq!(record, &expected, "complete shared PageData at {id}");
        assert!(
            record.get("json_ld").is_none(),
            "search is not a copy of another projection"
        );
    }
    let output = cli(&config, &RENDER_ARGUMENTS);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("actual CLI evidence");
    assert_eq!(
        report["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
}
