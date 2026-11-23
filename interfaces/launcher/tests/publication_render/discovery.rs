//! Configured discovery files use the actual retained local publication path.

#[path = "discovery/presentation.rs"]
mod presentation;
#[path = "discovery/sitemap_index.rs"]
mod sitemap_index;

use serde_json::json;

use super::{
    Fixture, PAGE, RENDER_ARGUMENTS, Value, acquire, build, bytes, cli, curated_llms, fs, render,
    source, tempdir, toml,
};

fn configured(fixture: &Fixture, sitemap: bool, llms: bool) -> String {
    let mut config = fixture.config();
    for (site, description, sections) in [
        (
            "one",
            "collection:home",
            vec![
                ("Collections", &["collection:home"][..]),
                ("Articles", &["content:one"][..]),
            ],
        ),
        (
            "other",
            "content:other",
            vec![("Documentation", &["content:other"][..])],
        ),
    ] {
        let curated = llms.then(|| curated_llms(description, &sections));
        config = config.replace(
            &format!("site='site:{site}'\npublic_base_url="),
            &format!(
                "site='site:{site}'\nmachine={{sitemap={sitemap}{}}}\npublic_base_url=",
                curated.map_or_else(String::new, |value| format!(",{value}"))
            ),
        );
    }
    config
}

#[test]
fn configured_outputs_match_actual_pages_and_cli_staging_without_foreign_or_alias_routes() {
    let fixture = Fixture::new(&[]);
    let config = configured(&fixture, true, true);
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("complete local discovery outputs");
    let tree = rendered.site().output();
    let model: Value = serde_json::from_slice(bytes(tree, "model.json")).expect("shared model");
    let sitemap = std::str::from_utf8(bytes(tree, "sitemap.xml")).expect("XML");
    let llms = std::str::from_utf8(bytes(tree, "llms.txt")).expect("Markdown");
    assert_eq!(sitemap.matches("<loc>").count(), 2);
    assert_eq!(
        llms.lines().filter(|line| line.starts_with("- [")).count(),
        2
    );
    assert!(llms.starts_with("# Explicit bindings\n"));
    assert!(llms.contains("The explicit fixture collection and its retained articles."));
    for (id, output) in [
        ("collection:home", "index.html"),
        ("content:one", "guide/index.html"),
    ] {
        let url = model["pages"][id]["route"]["canonical_url"]
            .as_str()
            .expect("compiled URL");
        assert!(sitemap.contains(&format!("<loc>{url}</loc>")));
        assert!(llms.contains(&format!("]({url})")));
        let alternate = if output == "index.html" {
            "index.md"
        } else {
            "guide/index.md"
        };
        assert!(llms.contains(&format!("]({url}index.md)")));
        assert!(!bytes(tree, alternate).is_empty());
        let html = std::str::from_utf8(bytes(tree, output)).expect("actual page HTML");
        assert!(
            html.replace("&#x2F;", "/")
                .contains(&format!("href=\"{url}\""))
        );
    }
    assert_eq!(
        model["pages"]["content:one"]["content"]["lifecycle"],
        "draft"
    );
    assert!(llms.contains("One \\<escaped\\>"));
    assert!(llms.contains("## Collections"));
    assert!(llms.contains("## Articles"));
    for absent in [
        "other.example",
        "old.html",
        "/contact",
        "/journeys/",
        "<lastmod>",
        "source:one",
    ] {
        assert!(
            !sitemap.contains(absent),
            "unexpected sitemap value {absent}"
        );
        assert!(!llms.contains(absent), "unexpected llms value {absent}");
    }
    assert!(!llms.contains(fixture.directory.path().to_str().expect("native root")));

    let parent = tempdir().expect("local stage parent");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments.extend([
        "--output-parent",
        parent.path().to_str().expect("stage parent"),
    ]);
    let result = cli(&config, &arguments);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let receipt: Value = serde_json::from_slice(&result.stdout).expect("stage evidence");
    assert_eq!(
        receipt["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    let directory = std::path::Path::new(receipt["directory"].as_str().expect("retained stage"));
    for name in ["sitemap.xml", "llms.txt"] {
        assert_eq!(
            fs::read(directory.join(name)).expect("staged discovery bytes"),
            bytes(tree, name)
        );
    }
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after capture",
    )
    .expect("disposable source mutation");
    let repeated = render(&inputs).expect("same retained inputs");
    assert_eq!(
        repeated.site().output().inventory().identity(),
        tree.inventory().identity()
    );
    for name in ["sitemap.xml", "llms.txt"] {
        assert_eq!(bytes(repeated.site().output(), name), bytes(tree, name));
    }
}

#[test]
fn curated_discovery_preserves_section_order_repetition_and_intentional_omission() {
    let fixture = Fixture::new(&[]);
    let fields = curated_llms(
        "collection:home",
        &[
            ("Read first", &["content:one"]),
            ("Return here", &["content:one"]),
        ],
    );
    let config = fixture.config().replacen(
        "public_base_url=",
        &format!("machine={{{fields}}}\npublic_base_url="),
        1,
    );
    let inputs = acquire(&config);
    let rendered = render(&inputs).expect("explicit ordered curated discovery");
    let llms = std::str::from_utf8(bytes(rendered.site().output(), "llms.txt")).expect("Markdown");
    assert!(
        llms.find("## Read first").expect("first section")
            < llms.find("## Return here").expect("second section")
    );
    assert_eq!(
        llms.matches("guide/index.md").count(),
        2,
        "deliberate repetition is not silently deduplicated"
    );
    assert!(
        !llms.contains("[Home]"),
        "description source does not automatically become an entry"
    );
    assert!(
        !llms.contains("## Authors"),
        "curation does not synthesize an attribution catalogue"
    );
    assert!(
        !llms.contains("other.example"),
        "unselected foreign page is not an entry"
    );
}

#[test]
fn invalid_curated_reference_refuses_output_instead_of_silently_shrinking_sections() {
    let fixture = Fixture::new(&[]);
    for content in ["content:missing", "content:other"] {
        let fields = curated_llms("collection:home", &[("Required reading", &[content])]);
        let config = fixture.config().replacen(
            "public_base_url=",
            &format!("machine={{{fields}}}\npublic_base_url="),
            1,
        );
        let inputs = acquire(&config);
        let failure =
            render(&inputs).expect_err("invalid owner-selected reference cannot be omitted");
        assert_eq!(failure.code(), "site_llms_reference");
        failed_without_stage(&config, "model.json", "site_llms_reference");
    }
}

#[test]
fn omission_and_each_machine_switch_select_only_the_requested_files() {
    let fixture = Fixture::new(&[]);
    for (config, sitemap, llms) in [
        (fixture.config(), false, false),
        (configured(&fixture, false, false), false, false),
        (configured(&fixture, true, false), true, false),
        (configured(&fixture, false, true), false, true),
        (
            configured(&fixture, false, true).replace("enabled=true", "enabled=false"),
            false,
            false,
        ),
    ] {
        let inputs = acquire(&config);
        let rendered = render(&inputs).expect("independent explicit selection");
        for (path, present) in [
            ("sitemap.xml", sitemap),
            ("llms.txt", llms),
            ("index.md", llms),
            ("guide/index.md", llms),
        ] {
            assert_eq!(
                rendered
                    .site()
                    .output()
                    .file(&path.parse().expect("machine path"))
                    .is_some(),
                present
            );
        }
    }
}

#[test]
fn independently_selected_second_site_uses_its_own_base_and_complete_local_page_set() {
    let fixture = Fixture::new(&[]);
    let config = configured(&fixture, true, true);
    let inputs = build::acquire_publication(
        &config,
        &"site:other".parse().expect("second site"),
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .expect("native second-site capture");
    let rendered = render(&inputs).expect("independently selected site");
    let tree = rendered.site().output();
    for path in ["sitemap.xml", "llms.txt"] {
        let text = std::str::from_utf8(bytes(tree, path)).expect("machine UTF-8");
        assert!(text.contains("https://other.example/elsewhere/"));
        assert!(!text.contains("one.example"));
        assert!(!text.contains("/mount/"));
    }
    let model: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("second-site model");
    assert_eq!(model["pages"].as_object().expect("local pages").len(), 1);
    assert_eq!(
        model["pages"]["content:other"]["content"]["lifecycle"],
        "archived"
    );
    let mut arguments = RENDER_ARGUMENTS;
    arguments[3] = "site:other";
    let output = cli(&config, &arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("CLI evidence");
    assert_eq!(report["status"], "local_rendered");
    assert_eq!(
        report["inventory"],
        serde_json::to_value(tree.inventory()).expect("SDK inventory")
    );
    assert_eq!(
        report["evidence"],
        serde_json::to_value(rendered.site().evidence()).expect("SDK evidence")
    );
}

fn failed_without_stage(config: &str, model_output: &str, code: &str) {
    let parent = tempdir().expect("unchanged output parent");
    fs::write(parent.path().join("keep"), b"unrelated").expect("unrelated fixture file");
    let mut arguments = RENDER_ARGUMENTS.to_vec();
    arguments[5] = model_output;
    arguments.extend(["--output-parent", parent.path().to_str().expect("parent")]);
    let result = cli(config, &arguments);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty(), "no successful partial receipt");
    assert!(
        String::from_utf8_lossy(&result.stderr).contains(code),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("parent contents")
            .count(),
        1
    );
    assert_eq!(
        fs::read(parent.path().join("keep")).expect("preserved bytes"),
        b"unrelated"
    );
}

#[test]
fn machine_paths_collide_with_model_page_and_static_assets_without_staging() {
    let fixture = Fixture::new(&[]);
    for model in ["sitemap.xml", "llms.txt"] {
        failed_without_stage(&configured(&fixture, true, true), model, "source_inventory");
    }
    let page = PAGE.replace("path='/guide/'", "path='/sitemap.xml'");
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    failed_without_stage(
        &configured(&fixture, true, true),
        "model.json",
        "source_inventory",
    );
    let fixture = Fixture::new(&[("one/llms.txt", b"retained static asset")]);
    let config = configured(&fixture, true, true).replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/'}}]",
    );
    failed_without_stage(&config, "model.json", "source_inventory");
}

#[test]
fn machine_serialization_failure_has_no_site_and_does_not_shrink_explicit_set() {
    let route = format!("/{}", "long".repeat(520));
    let page = PAGE.replace("/guide/", &route);
    let fixture = Fixture::new(&[("one/page.md", page.as_bytes())]);
    let config = configured(&fixture, true, true);
    let inputs = acquire(&config);
    let failure = render(&inputs).expect_err("sitemap URL bound rejects complete site");
    assert_eq!(failure.code(), "discovery_limit");
    failed_without_stage(&config, "model.json", "discovery_limit");
    let request = json!({
        "format": "gazette-local-publication-set/1", "workspace": {"kind":"supplied","source":config},
        "sites": (["site:one", "site:other"].map(|site| json!({
            "site": site,   "model_output": "model.json",
        }))),
    });
    let output = cli(&request.to_string(), &["render-sites", "--local"]);
    assert!(!output.status.success(), "explicit set remains incomplete");
    let report: Value =
        serde_json::from_slice(&output.stdout).expect("complete per-site failure report");
    assert_eq!(report["sites"].as_object().expect("requested set").len(), 2);
    assert_eq!(report["sites"]["site:one"]["status"], "failed");
    assert!(report["sites"]["site:one"].get("inventory").is_none());
    assert_eq!(report["sites"]["site:other"]["status"], "rendered");
}

#[test]
fn machine_files_share_graph_collisions_and_complete_inventory_entry_limits() {
    let fixture = Fixture::new(&[("theme/templates/graph.html", b"<h1>Local graph</h1>")]);
    let disabled_inputs = acquire(&fixture.config());
    let baseline = render(&disabled_inputs).expect("baseline without discovery files");
    let inputs = acquire(&configured(&fixture, true, true));
    let model = "model.json".parse().expect("model");
    let collision = "llms.txt".parse().expect("graph output");
    let options = |graph, limits| build::SiteRenderOptions {
        render_year: None,
        model_output: Some(&model),
        graph,
        limits,
    };
    let failure = build::render_publication(
        &inputs,
        options(
            Some(build::GraphRenderOptions {
                output: &collision,
                limits: Default::default(),
            }),
            Default::default(),
        ),
        Default::default(),
    )
    .expect_err("graph view and discovery cannot own one output path");
    assert_eq!(failure.code(), "source_inventory");
    let limits = source::Limits {
        entries: baseline.site().output().inventory().entries().len(),
        ..Default::default()
    };
    build::render_publication(&disabled_inputs, options(None, limits), Default::default())
        .expect("same exact entry budget admits the baseline");
    let failure = build::render_publication(&inputs, options(None, limits), Default::default())
        .expect_err("machine files consume the shared inventory budget");
    assert_eq!(failure.code(), "source_limit");
}
