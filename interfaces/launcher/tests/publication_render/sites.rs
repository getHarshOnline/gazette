//! Explicit local site sets through the real native acquisition, SDK and CLI.

#[path = "sites/limits.rs"]
mod limits;

use gazette_sdk::workspace::Key;
use serde_json::json;

use super::{Fixture, OTHER, Value, acquire, build, bytes, cli, fs, render, source, toml};

const SITES: [&str; 2] = ["site:one", "site:other"];
const PUBLICATION_SITES: [(&str, &str, &str); 7] = [
    ("site:getharsh", "getharsh", "getHarsh"),
    ("site:rawthoughts", "rawthoughts", "RawThoughts"),
    ("site:causality", "causality", "Causality"),
    (
        "site:post-agi-economics",
        "post-agi-economics",
        "Post-AGI Economics",
    ),
    ("site:sleepwalker", "sleepwalker", "Sleepwalker"),
    ("site:reve", "reve", "Rêve"),
    ("site:dao-studio", "dao-studio", "DAO Studio"),
];

#[path = "sites/selection.rs"]
mod selection;

#[path = "sites/configured.rs"]
mod configured;

#[path = "sites/singular_policy.rs"]
mod singular_policy;

#[path = "sites/preview.rs"]
pub(super) mod preview;

#[path = "sites/mcp.rs"]
mod mcp;

fn key(value: &str) -> Key {
    value.parse().expect("fixture site identity")
}

pub(super) fn acquire_set(config: &str, selected: &[&str]) -> build::AcquiredPublicationSet {
    build::acquire_publications(
        config,
        &selected.iter().map(|site| key(site)).collect::<Vec<_>>(),
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits::default(),
    )
    .expect("one explicit native acquisition set")
}

pub(super) fn options<'a>(
    selected: &[&str],
    page: &'a source::EntryPath,
    alias: &'a source::EntryPath,
    model: &'a source::EntryPath,
) -> build::PublicationSetRenderOptions<'a> {
    let mut options = yearless_options(selected, page, alias, model);
    for site in options.sites.values_mut() {
        site.render_year =
            Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"));
    }
    options
}

pub(super) fn yearless_options<'a>(
    selected: &[&str],
    _page: &'a source::EntryPath,
    _alias: &'a source::EntryPath,
    model: &'a source::EntryPath,
) -> build::PublicationSetRenderOptions<'a> {
    build::PublicationSetRenderOptions {
        derive_selection: false,
        preview_link_bases: None,
        selection: None,
        preview_urls: None,
        review_responses: None,
        sites: selected
            .iter()
            .map(|site| {
                (
                    key(site),
                    build::SiteRenderOptions {
                        render_year: None,
                        model_output: Some(model),
                        graph: None,
                        limits: source::Limits::default(),
                    },
                )
            })
            .collect(),
    }
}

pub(super) fn request(config: &str, selected: &[&str]) -> Value {
    json!({
        "format": "gazette-local-publication-set/1",
        "render_year": 2040,
        "workspace": {"kind":"supplied","source":config},
        "sites": selected.iter().map(|site| json!({
            "site": site,


            "model_output": "model.json",
        })).collect::<Vec<_>>(),
    })
}

fn independent_theme_config(fixture: &Fixture, other: &Fixture) -> String {
    let mut config = fixture.config().replacen(
        "[[brands]]",
        "[sites.composition]\ntheme={package='theme:other'}\n[[brands]]",
        1,
    );
    config.push_str(&format!(
        "[[sources]]\nid='source:other-theme'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n[[theme_packages]]\npackage='theme:other'\nsource='source:other-theme'\nmanifest='theme.toml'\n",
        other.directory.path().to_str().expect("independent Theme root"),
        other.commit,
    ));
    config
}

fn same_tree(actual: &source::CapturedSource, expected: &source::CapturedSource) {
    assert_eq!(
        actual.inventory().identity(),
        expected.inventory().identity()
    );
    assert_eq!(
        serde_json::to_value(actual.inventory()).expect("actual inventory"),
        serde_json::to_value(expected.inventory()).expect("expected inventory"),
    );
    for entry in expected.inventory().entries() {
        if matches!(entry.content, source::Content::File { .. }) {
            assert_eq!(
                actual.file(&entry.path).expect("complete set output"),
                expected.file(&entry.path).expect("complete single output"),
                "exact bytes at {}",
                entry.path,
            );
        }
    }
}

fn receipt_with_invocation_order(expected: &Value, selected: &[&str]) -> Value {
    assert_eq!(expected["build_selection"]["origin"], "invocation");
    let original = expected["build_selection"]["sites"]
        .as_array()
        .expect("retained invocation sites");
    assert_eq!(
        original
            .iter()
            .map(|site| site.as_str().expect("site identity"))
            .collect::<std::collections::BTreeSet<_>>(),
        selected
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        "only invocation order changes, never the selected set"
    );
    let mut reordered = expected.clone();
    reordered["build_selection"]["sites"] = json!(selected);
    reordered
}

pub(crate) fn seven_site_fixture() -> (Fixture, String, Vec<&'static str>) {
    let pages = PUBLICATION_SITES
        .iter()
        .enumerate()
        .map(|(index, (site, slug, name))| {
            let (related_site, related_slug, _) =
                PUBLICATION_SITES[(index + 1) % PUBLICATION_SITES.len()];
            (
                format!("one/{slug}/page.md"),
                format!(
                    "+++\nrelated=['content:{related_slug}']\n[content]\nid='content:{slug}'\nsite='{site}'\ntitle='{name}'\ndescription='The synthetic {name} publication acceptance page.'\nlifecycle='published'\nroute={{path='/{slug}/'}}\nstructure={{kind='page',family={{kind='article'}}}}\n+++\n# {name}\n"
                ),
                related_site,
            )
        })
        .collect::<Vec<_>>();
    let overrides = pages
        .iter()
        .map(|(path, body, _)| (path.as_str(), body.as_bytes()))
        .collect::<Vec<_>>();
    let fixture = Fixture::new(&overrides);
    let mut config = String::from(
        "[composition]\ntheme={package='theme:one'}\n\
         [[principals]]\nid='owner:one'\nkind='person'\nname='Fixture owner'\n",
    );
    for (site, _, _) in PUBLICATION_SITES {
        config.push_str(&format!(
            "[[sites]]\nid='{site}'\npublisher='owner:one'\ndeployable=false\n"
        ));
    }
    for (site, slug, name) in PUBLICATION_SITES {
        config.push_str(&format!(
            "[[brands]]\nid='brand:{slug}'\nname='{name}'\n\
             [[brands.publications]]\nsite='{site}'\npublic_base_url='https://{slug}.example/'\n\
             authored=[{{source='source:content-{slug}',target='{slug}'}}]\n"
        ));
    }
    config.push_str(&format!(
        "[[sources]]\nid='source:theme'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n[[theme_packages]]\npackage='theme:one'\nsource='source:theme'\nmanifest='theme.toml'\n",
        fixture.directory.path().to_str().expect("fixture root"),
        fixture.commit,
    ));
    for (_, slug, _) in PUBLICATION_SITES {
        config.push_str(&format!(
            "[[sources]]\nid='source:content-{slug}'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='one/{slug}'}}\n",
            fixture.directory.path().to_str().expect("fixture root"),
            fixture.commit,
        ));
    }
    (
        fixture,
        config,
        PUBLICATION_SITES.iter().map(|(site, _, _)| *site).collect(),
    )
}

#[test]
fn seven_publication_workspace_isolated_outputs_and_cross_site_relations_are_order_independent() {
    let (_fixture, config, selected) = seven_site_fixture();
    let page = "page.html".parse().expect("page template");
    let alias = "alias.html".parse().expect("alias template");
    let model = "model.json".parse().expect("model output");
    let inputs = acquire_set(&config, &selected);
    assert_eq!(
        inputs.sources().len(),
        PUBLICATION_SITES.len() + 1,
        "seven exact content sources and one Theme"
    );
    let rendered = build::render_publications(
        &inputs,
        options(&selected, &page, &alias, &model),
        Default::default(),
    )
    .expect("complete seven-site semantic phase");
    assert!(rendered.is_complete());
    assert_eq!(rendered.sites().len(), PUBLICATION_SITES.len());

    for (index, (site, slug, _)) in PUBLICATION_SITES.iter().enumerate() {
        let output = rendered.sites()[&key(site)]
            .as_ref()
            .expect("selected publication rendered")
            .site()
            .output();
        let value: Value = serde_json::from_slice(bytes(output, "model.json")).expect("site model");
        let pages = value["pages"].as_object().expect("site page map");
        assert_eq!(pages.len(), 1, "no foreign page copied into {site}");
        let page = &pages[&format!("content:{slug}")];
        let (related_site, related_slug, _) =
            PUBLICATION_SITES[(index + 1) % PUBLICATION_SITES.len()];
        assert_eq!(page["related"][0]["id"], format!("content:{related_slug}"));
        assert_eq!(page["related"][0]["site"], related_site);
        assert!(
            output
                .file(&format!("{slug}/index.html").parse().expect("site route"))
                .is_some()
        );
        for (foreign_site, foreign_slug, _) in PUBLICATION_SITES {
            if foreign_site != *site {
                assert!(
                    output
                        .file(
                            &format!("{foreign_slug}/index.html")
                                .parse()
                                .expect("foreign route"),
                        )
                        .is_none(),
                    "{site} must not copy {foreign_site}"
                );
            }
        }
    }

    let expected = serde_json::to_value(rendered.inspect()).expect("seven-site receipt");
    assert_eq!(expected["build_selection"]["sites"], json!(selected));
    let reversed = selected.iter().rev().copied().collect::<Vec<_>>();
    let reversed_expected = receipt_with_invocation_order(&expected, &reversed);
    let reversed_inputs = acquire_set(&config, &reversed);
    let reversed_rendered = build::render_publications(
        &reversed_inputs,
        options(&reversed, &page, &alias, &model),
        Default::default(),
    )
    .expect("request order cannot change the site set");
    assert_eq!(
        serde_json::to_value(reversed_rendered.inspect()).expect("reordered receipt"),
        reversed_expected
    );
    for site in &selected {
        same_tree(
            reversed_rendered.sites()[&key(site)]
                .as_ref()
                .expect("reversed publication")
                .site()
                .output(),
            rendered.sites()[&key(site)]
                .as_ref()
                .expect("original publication")
                .site()
                .output(),
        );
    }

    let response = cli(
        &request(&config, &reversed).to_string(),
        &["render-sites", "--local"],
    );
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI receipt"),
        reversed_expected
    );
    let checked = cli(
        &request(&config, &reversed).to_string(),
        &["check", "build", "--local"],
    );
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert_eq!(checked.stdout, response.stdout);
}

#[test]
fn build_check_reuses_complete_render_failure_without_retaining_an_artifact() {
    let fixture = Fixture::new(&[("theme/templates/page.html", b"{% if broken")]);
    let input = request(&fixture.config(), &SITES).to_string();
    let rendered = cli(&input, &["render-sites", "--local"]);
    let checked = cli(&input, &["check", "build", "--local"]);
    assert!(!rendered.status.success());
    assert!(!checked.status.success());
    assert_eq!(checked.stdout, rendered.stdout);
    assert_eq!(checked.stderr, rendered.stderr);
    let report: Value =
        serde_json::from_slice(&checked.stdout).expect("complete failed set report");
    assert_eq!(report["status"], "local_set_incomplete");
    for site in SITES {
        assert_eq!(report["sites"][site]["status"], "failed");
        assert_eq!(report["sites"][site]["code"], "render_template");
    }
}

#[test]
fn two_site_sdk_and_cli_match_single_outputs_and_survive_native_changes() {
    let other = OTHER.replacen("+++\n", "+++\nrelated=['content:one']\n", 1);
    let fixture = Fixture::new(&[("other/page.md", other.as_bytes())]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &SITES);
    assert_eq!(
        inputs.sites().keys().cloned().collect::<Vec<_>>(),
        SITES.map(key)
    );
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("complete shared semantic phase");
    assert!(rendered.is_complete());
    assert_eq!(rendered.sites().len(), 2);

    for site in SITES {
        let selected = build::acquire_publication(
            &config,
            &key(site),
            toml::Limits::default(),
            source::PreparationLimits {
                sources: 8,
                entries: 128,
                bytes: 1024 * 1024,
                per_source: Default::default(),
            },
        )
        .expect("existing single-site native path");
        let single = render(&selected).expect("existing single-site render path");
        let actual = rendered.sites()[&key(site)]
            .as_ref()
            .expect("selected site succeeded");
        same_tree(actual.site().output(), single.site().output());
        assert_eq!(
            serde_json::to_value(actual.site().evidence()).expect("set evidence"),
            serde_json::to_value(single.site().evidence()).expect("single evidence"),
        );
        let value: Value = serde_json::from_slice(bytes(actual.site().output(), "model.json"))
            .expect("site model");
        let (content, foreign, foreign_site) = if site == "site:one" {
            ("content:one", "content:other", "site:other")
        } else {
            ("content:other", "content:one", "site:one")
        };
        assert_eq!(value["pages"][content]["related"][0]["id"], foreign);
        assert_eq!(value["pages"][content]["related"][0]["site"], foreign_site);
        assert!(
            value["pages"].get(foreign).is_none(),
            "foreign pages are not copied"
        );
    }

    let expected = serde_json::to_value(rendered.inspect()).expect("set receipt");
    assert_eq!(expected["build_selection"]["sites"], json!(SITES));
    let first_input = inputs.sites()[&key("site:one")]
        .as_ref()
        .expect("first acquired site");
    let acquisition =
        serde_json::to_value(first_input.inspect()).expect("shared acquisition evidence");
    assert_eq!(
        expected["compilation"],
        serde_json::to_value(first_input.compilation()).expect("exact compiled configuration"),
    );
    assert_eq!(
        expected["sources"], acquisition["sources"],
        "exact shared pins and captured inventories"
    );
    assert_eq!(
        expected["sources"]
            .as_object()
            .expect("source evidence map")
            .len(),
        3
    );
    for source_key in ["source:one", "source:other", "source:theme"] {
        assert!(
            expected["sources"].get(source_key).is_some(),
            "retained source {source_key}"
        );
    }
    for selected in [SITES, ["site:other", "site:one"]] {
        let ordered_expected = receipt_with_invocation_order(&expected, &selected);
        let response = cli(
            &request(&config, &selected).to_string(),
            &["render-sites", "--local"],
        );
        assert!(
            response.status.success(),
            "{}",
            String::from_utf8_lossy(&response.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Value>(&response.stdout).expect("CLI set receipt"),
            ordered_expected
        );
        let reversed_inputs = acquire_set(&config, &selected);
        let reversed = build::render_publications(
            &reversed_inputs,
            options(&selected, &page, &alias, &model),
            Default::default(),
        )
        .expect("request order is not meaning");
        assert_eq!(
            serde_json::to_value(reversed.inspect()).expect("stable receipt"),
            ordered_expected
        );
        for site in SITES {
            same_tree(
                reversed.sites()[&key(site)]
                    .as_ref()
                    .expect("reordered success")
                    .site()
                    .output(),
                rendered.sites()[&key(site)]
                    .as_ref()
                    .expect("original success")
                    .site()
                    .output(),
            );
        }
    }

    for path in ["one/page.md", "other/page.md", "theme/templates/page.html"] {
        fs::write(
            fixture.directory.path().join(path),
            "changed after all CLI comparisons",
        )
        .expect("disposable native fixture change");
    }
    let retained = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("no live-file reread during rendering");
    assert!(retained.is_complete());
    assert_eq!(
        serde_json::to_value(retained.inspect()).expect("retained receipt"),
        expected
    );
    for site in SITES {
        same_tree(
            retained.sites()[&key(site)]
                .as_ref()
                .expect("retained success")
                .site()
                .output(),
            rendered.sites()[&key(site)]
                .as_ref()
                .expect("original success")
                .site()
                .output(),
        );
    }
}

#[test]
fn distinct_site_theme_and_source_bindings_match_their_independent_single_renders() {
    let other_template =
        "<section data-theme='other'>{{ content.title }}{{ body_html | safe }}</section>";
    let fixture = Fixture::new(&[
        (
            "theme/other/theme.toml",
            b"package='theme:other'\nnamespace='other'\ntemplates='pages'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
        ),
        ("theme/other/pages/page.html", other_template.as_bytes()),
        ("theme/other/pages/alias.html", super::ALIAS.as_bytes()),
    ]);
    let mut config = fixture.config().replacen(
        "[[brands]]",
        "[sites.composition]\ntheme={package='theme:other'}\n[[brands]]",
        1,
    );
    config.push_str(&format!(
        "[[sources]]\nid='source:other-theme'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme/other'}}\n[[theme_packages]]\npackage='theme:other'\nsource='source:other-theme'\nmanifest='theme.toml'\n",
        fixture.directory.path().to_str().expect("native source root"),
        fixture.commit,
    ));
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("same logical page entry");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("one shared graph with distinct resolved presentation bindings");
    assert!(rendered.is_complete());
    for (site, theme, source_key, output) in [
        ("site:one", "theme:one", "source:theme", "guide/index.html"),
        (
            "site:other",
            "theme:other",
            "source:other-theme",
            "elsewhere/index.html",
        ),
    ] {
        let selected = build::acquire_publication(
            &config,
            &key(site),
            toml::Limits::default(),
            build::PublicationSetAcquisitionLimits::default().sources,
        )
        .expect("independent single-site acquisition");
        let single = render(&selected).expect("single-site resolved Theme");
        let actual = rendered.sites()[&key(site)]
            .as_ref()
            .expect("selected site success")
            .site();
        same_tree(actual.output(), single.site().output());
        assert_eq!(
            serde_json::to_value(actual.evidence()).expect("set evidence"),
            serde_json::to_value(single.site().evidence()).expect("single evidence"),
        );
        assert_eq!(actual.evidence().theme.theme.as_str(), theme);
        assert_eq!(
            actual
                .evidence()
                .theme
                .packages
                .last()
                .expect("selected root package contribution")
                .source,
            key(source_key)
        );
        let html = std::str::from_utf8(bytes(actual.output(), output)).expect("rendered HTML");
        if site == "site:other" {
            assert!(html.starts_with("<section data-theme='other'>Other"));
            assert!(html.contains("Other body"));
            assert!(html.ends_with("</section>"));
        } else {
            assert!(html.starts_with("<h1>One &lt;escaped&gt;</h1>"));
            assert!(!html.contains("data-theme='other'"));
        }
    }
    let first = rendered.sites()[&key("site:one")]
        .as_ref()
        .expect("first")
        .site();
    let other = rendered.sites()[&key("site:other")]
        .as_ref()
        .expect("other")
        .site();
    let first_model: Value =
        serde_json::from_slice(bytes(first.output(), "model.json")).expect("first site model");
    let other_model: Value =
        serde_json::from_slice(bytes(other.output(), "model.json")).expect("other site model");
    assert_eq!(
        first_model["pages"]["content:one"]["site"]["presentation"]["theme"],
        "theme:one"
    );
    assert_eq!(
        first_model["pages"]["content:one"]["related"][0]["presentation"]["theme"], "theme:other",
        "cross-site relations carry the destination's resolved Theme identity"
    );
    assert_eq!(
        other_model["pages"]["content:other"]["site"]["presentation"]["theme"],
        "theme:other"
    );
    assert_ne!(
        first.evidence().theme.template_tree,
        other.evidence().theme.template_tree
    );
    let response = cli(
        &request(&config, &SITES).to_string(),
        &["render-sites", "--local"],
    );
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout)
            .expect("distinct-composition CLI receipt"),
        serde_json::to_value(rendered.inspect()).expect("exact SDK receipt"),
    );
}

#[test]
fn dirty_or_missing_presentation_sources_isolate_first_last_and_all_sites() {
    let other_manifest = "package='theme:other'\nnamespace='other'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n";
    for missing in [false, true] {
        for failed in [&SITES[..1], &SITES[1..], SITES.as_slice()] {
            let fixture = Fixture::new(&[]);
            let other = Fixture::new(&[("theme/theme.toml", other_manifest.as_bytes())]);
            let config = independent_theme_config(&fixture, &other);
            let baseline_inputs = acquire_set(&config, &SITES);
            let page = "page.html".parse().expect("page");
            let alias = "alias.html".parse().expect("alias");
            let model = "model.json".parse().expect("model");
            let render_options = || {
                let mut selected = options(&SITES, &page, &alias, &model);
                for site in selected.sites.values_mut() {
                    site.render_year =
                        Some(gazette_sdk::authored::RenderYear::new(2040).expect("fixture year"));
                }
                selected
            };
            let baseline =
                build::render_publications(&baseline_inputs, render_options(), Default::default())
                    .expect("baseline shared phase");
            assert!(baseline.is_complete());
            for site in failed {
                let root = if *site == "site:one" {
                    &fixture
                } else {
                    &other
                };
                if missing {
                    fs::rename(
                        root.directory.path().join("theme"),
                        root.directory.path().join("parked-theme"),
                    )
                    .expect("only the disposable selected Theme subtree becomes unavailable");
                } else {
                    fs::write(
                        root.directory.path().join("theme/templates/page.html"),
                        "dirty Theme canary",
                    )
                    .expect("only the disposable selected Theme subtree becomes dirty");
                }
            }
            let inputs = acquire_set(&config, &SITES);
            assert_eq!(
                inputs.sites().keys().cloned().collect::<Vec<_>>(),
                SITES.map(key)
            );
            assert_eq!(inputs.source_failures().len(), failed.len());
            let rendered =
                build::render_publications(&inputs, render_options(), Default::default())
                    .expect("presentation-only acquisition failures retain the exact result set");
            assert!(!rendered.is_complete());
            let report =
                serde_json::to_value(rendered.inspect()).expect("partial acquisition receipt");
            assert_eq!(
                report["source_failures"]
                    .as_object()
                    .expect("source failures")
                    .len(),
                failed.len()
            );
            for site in SITES {
                let source_key = if site == "site:one" {
                    "source:theme"
                } else {
                    "source:other-theme"
                };
                if failed.contains(&site) {
                    let source_failure = &inputs.source_failures()[&key(source_key)];
                    let acquisition = inputs.sites()[&key(site)]
                        .as_ref()
                        .expect_err("dependent site acquisition fails");
                    assert!(
                        std::sync::Arc::ptr_eq(acquisition, source_failure),
                        "the exact source failure is shared"
                    );
                    assert!(
                        !inputs.sources().contains_key(&key(source_key)),
                        "failed bytes are never admitted"
                    );
                    let failure = rendered.sites()[&key(site)]
                        .as_ref()
                        .expect_err("no failed-site tree");
                    assert!(matches!(
                        failure,
                        build::PublicationSiteError::Acquisition(_)
                    ));
                    assert_eq!(failure.code(), source_failure.code());
                    assert_eq!(report["sites"][site]["status"], "failed");
                    assert!(report["sites"][site].get("inventory").is_none());
                    assert_eq!(
                        report["source_failures"][source_key]["code"],
                        source_failure.code()
                    );
                    assert_eq!(
                        report["source_failures"][source_key]["message"],
                        source_failure.to_string()
                    );
                } else {
                    assert!(
                        inputs.sites()[&key(site)].is_ok(),
                        "unaffected acquisition remains usable"
                    );
                    let actual = rendered.sites()[&key(site)]
                        .as_ref()
                        .expect("unaffected site renders after any prior failure");
                    same_tree(
                        actual.site().output(),
                        baseline.sites()[&key(site)]
                            .as_ref()
                            .expect("baseline")
                            .site()
                            .output(),
                    );
                    let single_inputs = build::acquire_publication(
                        &config,
                        &key(site),
                        toml::Limits::default(),
                        build::PublicationSetAcquisitionLimits::default().sources,
                    )
                    .expect("unaffected native single-site acquisition");
                    same_tree(
                        actual.site().output(),
                        render(&single_inputs)
                            .expect("single-site sibling")
                            .site()
                            .output(),
                    );
                    for failed_site in failed {
                        let protected = if *failed_site == "site:one" {
                            &fixture
                        } else {
                            &other
                        };
                        let children = || {
                            fs::read_dir(protected.directory.path())
                                .expect("protected root")
                                .map(|entry| entry.expect("root entry").file_name())
                                .collect::<std::collections::BTreeSet<_>>()
                        };
                        let before = children();
                        let staged = build::stage_publication(
                            actual,
                            protected.directory.path(),
                            source::Limits::default(),
                            16 << 10,
                        );
                        assert!(
                            matches!(staged, Err(source::StageError::Preflight(_))),
                            "failed selected source root remains protected from a healthy sibling's stage"
                        );
                        assert_eq!(children(), before, "rejected stage leaves no native output");
                    }
                }
            }
            let response = cli(
                &request(&config, &SITES).to_string(),
                &["render-sites", "--local", "--render-year", "2040"],
            );
            assert!(!response.status.success());
            assert_eq!(
                serde_json::from_slice::<Value>(&response.stdout)
                    .expect("CLI keeps failed membership"),
                report
            );
            for site in failed {
                let root = if *site == "site:one" {
                    &fixture
                } else {
                    &other
                };
                if missing {
                    assert!(
                        !root.directory.path().join("theme").exists(),
                        "no repair or replacement effect"
                    );
                    assert!(root.directory.path().join("parked-theme").is_dir());
                } else {
                    assert_eq!(
                        fs::read(root.directory.path().join("theme/templates/page.html"))
                            .expect("unchanged dirty source"),
                        b"dirty Theme canary"
                    );
                }
            }
        }
    }
}

#[test]
fn one_shared_failed_presentation_source_fans_out_without_parsing_all_failed_sites() {
    let fixture = Fixture::new(&[("one/page.md", b"+++\n[invalid content syntax\n+++\n")]);
    let config = fixture.config();
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "dirty shared Theme",
    )
    .expect("disposable shared presentation failure");
    let inputs = acquire_set(&config, &SITES);
    assert_eq!(
        inputs.source_failures().keys().cloned().collect::<Vec<_>>(),
        vec![key("source:theme")]
    );
    assert_eq!(
        inputs.sources().len(),
        2,
        "both content sources were captured without syntax promotion"
    );
    let shared = &inputs.source_failures()[&key("source:theme")];
    for site in SITES {
        let failed = inputs.sites()[&key(site)]
            .as_ref()
            .expect_err("both sites depend on the failed source");
        assert!(std::sync::Arc::ptr_eq(failed, shared));
    }
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("all-failed acquisition does not attempt the intentionally invalid corpus");
    assert!(!rendered.is_complete());
    assert_eq!(
        rendered.sites().keys().cloned().collect::<Vec<_>>(),
        SITES.map(key)
    );
    for result in rendered.sites().values() {
        assert!(matches!(
            result,
            Err(build::PublicationSiteError::Acquisition(_))
        ));
    }
    let report = serde_json::to_value(rendered.inspect()).expect("all-failed receipt");
    assert_eq!(
        report["compilation"],
        serde_json::to_value(inputs.compilation()).expect("shared configuration survives")
    );
    assert_eq!(
        report["source_failures"]
            .as_object()
            .expect("failure map")
            .len(),
        1
    );
    let response = cli(
        &request(&config, &SITES).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("exact all-failed CLI receipt"),
        report
    );
}

#[test]
fn a_failed_source_required_by_both_content_and_presentation_remains_global() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config().replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:theme',target='one'}]",
    );
    build::acquire_publications(
        &config,
        &SITES.map(key),
        toml::Limits::default(),
        Default::default(),
    )
    .expect("the selected source can be acquired before the deliberate native failure");
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "dirty content-required source",
    )
    .expect("disposable shared-content source mutation");
    let error = build::acquire_publications(
        &config,
        &SITES.map(key),
        toml::Limits::default(),
        Default::default(),
    )
    .expect_err("content ownership prevents downgrading this to a per-site presentation failure");
    assert!(
        matches!(&error, build::PublicationSetError::Acquisition(build::CompositionError::Source { binding, .. }) if binding == &key("source:theme"))
    );
    let response = cli(
        &request(&config, &SITES).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert!(
        response.stdout.is_empty(),
        "no partial acquisition set exists for shared content failure"
    );
    assert!(String::from_utf8_lossy(&response.stderr).contains(error.code()));
}

#[test]
fn all_failed_presentation_sources_survive_even_when_each_site_reports_only_the_first() {
    let fixture = Fixture::new(&[]);
    let capabilities = Fixture::new(&[(
        "theme/plugin.toml",
        b"package='shared-plugin'\n[extensions.'shared-plugin.shared/v1']\nschema='shared.schema.json'\n[providers.shared]\nimplements=['shared-plugin.shared/v1']\n",
    ), ("theme/shared.schema.json", b"{}")]);
    let mut config = fixture.config().replacen(
        "[[principals]]",
        "[composition.plugins]\n'shared-plugin'={state='enabled'}\n[composition.capabilities]\n'shared-plugin.shared/v1'={state='enabled',provider={kind='plugin',package='shared-plugin',provider='shared'},requirement='required'}\n[[principals]]",
        1,
    );
    config.push_str(&format!(
        "[[sources]]\nid='source:capabilities'\nlocation={{kind='repository',root={:?},commit='{}',subdirectory='theme'}}\n[[plugin_packages]]\npackage='shared-plugin'\nsource='source:capabilities'\nmanifest='plugin.toml'\n",
        capabilities.directory.path().to_str().expect("capability source"), capabilities.commit,
    ));
    for selected in [&fixture, &capabilities] {
        fs::write(
            selected.directory.path().join("theme/templates/page.html"),
            "failed selected source",
        )
        .expect("independent disposable presentation failure");
    }
    let inputs = acquire_set(&config, &SITES);
    assert_eq!(
        inputs.source_failures().keys().cloned().collect::<Vec<_>>(),
        vec![key("source:capabilities"), key("source:theme")]
    );
    let first = &inputs.source_failures()[&key("source:capabilities")];
    for site in SITES {
        assert!(std::sync::Arc::ptr_eq(
            inputs.sites()[&key(site)]
                .as_ref()
                .expect_err("first failure per site"),
            first
        ));
    }
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("all failures remain observable without rendering");
    assert!(!rendered.is_complete());
    let report = serde_json::to_value(rendered.inspect()).expect("complete failure ledger");
    assert_eq!(
        report["source_failures"]
            .as_object()
            .expect("every source failure")
            .len(),
        2
    );
    for (source_key, error) in inputs.source_failures() {
        assert_eq!(
            report["source_failures"][source_key.as_str()]["code"],
            error.code()
        );
        assert_eq!(
            report["source_failures"][source_key.as_str()]["message"],
            error.to_string()
        );
    }
    let response = cli(
        &request(&config, &SITES).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI source failures"),
        report
    );
}

#[test]
fn presentation_only_native_byte_limit_does_not_discard_an_independent_sibling() {
    let large_template = "large native presentation input ".repeat(128);
    let fixture = Fixture::new(&[("theme/templates/page.html", large_template.as_bytes())]);
    let other = Fixture::new(&[(
        "theme/theme.toml",
        b"package='theme:other'\nnamespace='other'\ntemplates='templates'\nbrowser_theme_colors={light='#ffffff',dark='#000000'}\nrender={page='page.html',alias='alias.html'}\n",
    )]);
    let config = independent_theme_config(&fixture, &other);
    let mut limits = build::PublicationSetAcquisitionLimits::default();
    limits.sources.per_source.content.file_bytes = 1024;
    let inputs =
        build::acquire_publications(&config, &SITES.map(key), toml::Limits::default(), limits)
            .expect("the oversized source is presentation-only");
    assert_eq!(
        inputs.source_failures().keys().cloned().collect::<Vec<_>>(),
        vec![key("source:theme")]
    );
    assert!(inputs.sites()[&key("site:one")].is_err());
    assert!(inputs.sites()[&key("site:other")].is_ok());
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("bounded source failure does not become a shared semantic failure");
    assert!(!rendered.is_complete());
    assert!(matches!(
        rendered.sites()[&key("site:one")],
        Err(build::PublicationSiteError::Acquisition(_))
    ));
    let single_inputs = build::acquire_publication(
        &config,
        &key("site:other"),
        toml::Limits::default(),
        limits.sources,
    )
    .expect("same native allowance admits the independent sibling");
    same_tree(
        rendered.sites()[&key("site:other")]
            .as_ref()
            .expect("sibling tree")
            .site()
            .output(),
        render(&single_inputs)
            .expect("native single-site sibling")
            .site()
            .output(),
    );
}

#[test]
fn explicit_subset_keeps_foreign_relationships_without_rendering_foreign_site() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &["site:one"]);
    assert_eq!(
        inputs.sites().keys().cloned().collect::<Vec<_>>(),
        vec![key("site:one")]
    );
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let rendered = build::render_publications(
        &inputs,
        options(&["site:one"], &page, &alias, &model),
        Default::default(),
    )
    .expect("explicit subset");
    assert!(rendered.is_complete());
    assert_eq!(
        rendered.sites().keys().cloned().collect::<Vec<_>>(),
        vec![key("site:one")]
    );
    let tree = rendered.sites()[&key("site:one")]
        .as_ref()
        .expect("selected site")
        .site()
        .output();
    let value: Value =
        serde_json::from_slice(bytes(tree, "model.json")).expect("shared graph projection");
    assert_eq!(
        value["pages"]["content:one"]["related"][0]["id"],
        "content:other"
    );
    assert!(
        tree.file(&"elsewhere/index.html".parse().expect("foreign output"))
            .is_none()
    );
    let single_inputs = acquire(&config);
    same_tree(
        tree,
        render(&single_inputs)
            .expect("single render")
            .site()
            .output(),
    );
    let response = cli(
        &request(&config, &["site:one"]).to_string(),
        &["render-sites", "--local"],
    );
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("subset receipt"),
        serde_json::to_value(rendered.inspect()).expect("SDK subset receipt"),
    );
}

#[test]
fn each_site_can_render_its_shared_graph_with_exact_paired_cli_options() {
    let template = "{{ site }}|{{ presentation.theme }}{% for id, node in references %}|{{ id }}:{{ node.site }}{% endfor %}";
    let fixture = Fixture::new(&[("theme/templates/graph.html", template.as_bytes())]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let graph_output = "graph-view.html".parse().expect("graph output");
    let mut selected = options(&SITES, &page, &alias, &model);
    for option in selected.sites.values_mut() {
        option.graph = Some(build::GraphRenderOptions {
            output: &graph_output,
            limits: Default::default(),
        });
    }
    let rendered = build::render_publications(&inputs, selected, Default::default())
        .expect("shared semantic graph with separate site projections");
    assert!(rendered.is_complete());
    for site in SITES {
        let rendered_site = rendered.sites()[&key(site)]
            .as_ref()
            .expect("site graph success")
            .site();
        let json: Value = serde_json::from_slice(bytes(rendered_site.output(), "model.json"))
            .expect("machine graph projection");
        assert_eq!(json["graph"]["site"], site);
        assert_eq!(json["graph"]["presentation"]["theme"], "theme:one");
        assert_eq!(json["graph"]["presentation"]["capabilities"], json!({}));
        assert_eq!(json["graph"]["presentation"]["theme_resources"], json!([]));
        assert_eq!(json["graph"]["presentation"]["resources"], json!([]));
        let evidence = rendered_site
            .evidence()
            .graph
            .as_ref()
            .expect("explicit graph evidence");
        assert_eq!(
            evidence.model_sha256,
            source::Sha256Digest::of(
                &serde_json::to_vec(&json["graph"]).expect("same graph Value")
            ),
        );
        let output = std::str::from_utf8(bytes(rendered_site.output(), "graph-view.html"))
            .expect("graph HTML");
        assert!(output.starts_with(site));
        assert!(output.contains("|theme:one"));
        if site == "site:one" {
            assert!(output.contains("|content:other:site:other"));
            assert_eq!(
                json["graph"]["references"]["content:other"]["site"],
                "site:other"
            );
            assert_eq!(
                json["graph"]["references"]["content:other"]["title"],
                "Other"
            );
        }
    }
    let mut json = request(&config, &SITES);
    for site in json["sites"].as_array_mut().expect("site requests") {
        site["graph"] = json!({"output": "graph-view.html"});
    }
    let response = cli(&json.to_string(), &["render-sites", "--local"]);
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("graph set receipt"),
        serde_json::to_value(rendered.inspect()).expect("same SDK graph evidence"),
    );
}
