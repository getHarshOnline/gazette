//! One explicit candidate reaches every selected site through the native SDK and CLI.

use gazette_sdk::{authored, publication::content::Disposition};

use super::{
    Fixture, OTHER, SITES, Value, acquire_set, build, bytes, cli, fs, json, key, options, request,
    same_tree,
};

fn selection() -> authored::Selection {
    authored::Selection {
        content: [
            (key("collection:home"), Disposition::Include),
            (key("content:one"), Disposition::Exclude),
            (key("content:other"), Disposition::Include),
        ]
        .into_iter()
        .collect(),
        assets: Default::default(),
    }
}

fn selected_request(config: &str, selection: &authored::Selection) -> Value {
    let mut input = request(config, &SITES);
    input["selection"] = serde_json::to_value(selection).expect("exact complete intent");
    input
}

#[test]
fn one_candidate_drives_two_sites_human_machine_outputs_and_skips_excluded_programs() {
    let invalid_program = super::super::PAGE.replacen(
        "[content]\n",
        "[[resources]]\ninputs=['bad.ts']\nbuild={kind='browser-module',entry='bad.ts'}\noutput='/bad.js'\n[content]\n",
        1,
    );
    let kept = "+++\ncollections=['collection:home']\nrelated=['content:other']\n[content]\nid='content:kept'\nsite='site:one'\ntitle='Kept archive'\nlifecycle='archived'\nroute={path='/kept/'}\nstructure={kind='page',family={kind='article'}}\n+++\n# Retained archive\n[Public attachment](public.txt)\n";
    let other = OTHER.replacen("+++\n", "+++\nrelated=['content:kept']\n", 1);
    let template = "<h1>{{ content.title }}</h1>{{ body_html | safe }}<script type='application/ld+json'>{{ json_ld | json_script }}</script>";
    let fixture = Fixture::new(&[
        ("one/page.md", invalid_program.as_bytes()),
        ("one/kept.md", kept.as_bytes()),
        ("other/page.md", other.as_bytes()),
        ("one/bad.ts", b"export const = invalid program"),
        ("one/private.txt", b"EXCLUDED PRIVATE ASSET CANARY"),
        ("one/public.txt", b"retained public asset"),
        ("theme/templates/page.html", template.as_bytes()),
        ("theme/templates/graph.html", b"{{ __tera_context }}"),
    ]);
    let config = fixture.config()
        .replace("authored=[{source='source:one',target='one'}]", "authored=[{source='source:one',target='one',static_assets={route_prefix='/assets/'}}]")
        .replacen("public_base_url=", &format!("machine={{search='lexical',sitemap=true,{},json_ld=true}}\npublic_base_url=", super::super::curated_llms("collection:home", &[("Retained reading", &["collection:home", "content:kept"])])), 1)
        .replace("public_base_url='https://other.example/'", &format!("machine={{search='lexical',sitemap=true,{},json_ld=true}}\npublic_base_url='https://other.example/'", super::super::curated_llms("content:other", &[("Documentation", &["content:other"])])))
        .replacen("json_ld=true}", "json_ld=true,feeds={identity_base='https://identity.example/',channels=[{id='feed:json',channel='collection:home',path='/feed.json',format='json',scope={kind='site'}}]}}", 1);
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("valid logical fixture path");
    let alias = "alias.html".parse().expect("valid logical fixture path");
    let model = "model.json".parse().expect("valid logical fixture path");
    let graph_output = "graph-view.html"
        .parse()
        .expect("valid logical fixture path");
    let unfiltered = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("unfiltered shared phase");
    assert_eq!(
        unfiltered.sites()[&key("site:one")]
            .as_ref()
            .expect_err("real invalid TypeScript reaches its native compiler")
            .code(),
        "resource_compile"
    );
    let mut intent = selection();
    intent
        .content
        .insert(key("content:kept"), Disposition::Include);
    for (path, disposition) in [
        ("one/bad.ts", Disposition::Exclude),
        ("one/private.txt", Disposition::Exclude),
        ("one/public.txt", Disposition::Include),
    ] {
        intent.assets.insert(
            path.parse().expect("valid logical fixture path"),
            disposition,
        );
    }
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(intent.clone());
    for site in selected.sites.values_mut() {
        site.graph = Some(build::GraphRenderOptions {
            output: &graph_output,
            limits: Default::default(),
        });
    }
    let rendered = build::render_publications(&inputs, selected, Default::default())
        .expect("complete closed candidate");
    assert!(rendered.is_complete());
    assert_eq!(
        rendered.sites().keys().cloned().collect::<Vec<_>>(),
        SITES.map(key)
    );
    let set_intent = rendered.selection().expect("retained selection");
    assert_eq!(
        serde_json::to_value(set_intent).expect("borrowed set intent"),
        serde_json::to_value(&intent).expect("complete selection intent")
    );
    for (site, count, local, foreign) in [
        ("site:one", 2, "content:kept", "content:other"),
        ("site:other", 1, "content:other", "content:kept"),
    ] {
        let outcome = rendered.sites()[&key(site)]
            .as_ref()
            .expect("selected site output");
        assert!(std::ptr::eq(
            set_intent,
            outcome.selection().expect("same shared intent allocation")
        ));
        let tree = outcome.site().output();
        let model: Value =
            serde_json::from_slice(bytes(tree, "model.json")).expect("site machine model");
        let search: Value =
            serde_json::from_slice(bytes(tree, "search.json")).expect("site search documents");
        assert_eq!(
            model["pages"].as_object().expect("site page map").len(),
            count
        );
        assert_eq!(
            search.as_array().expect("search document array").len(),
            count
        );
        assert_eq!(model["pages"][local]["related"][0]["id"], foreign);
        assert!(model["pages"].get(foreign).is_none());
        for document in search.as_array().expect("search document array") {
            let id = document["content"]["id"]
                .as_str()
                .expect("exact content identity");
            assert_eq!(document["content"], model["pages"][id]["content"]);
            assert_eq!(document["body"], model["pages"][id]["body_text"]);
            let output = document["route"]["output"]
                .as_str()
                .expect("compiled output path");
            let html = std::str::from_utf8(bytes(tree, output)).expect("actual UTF-8 page");
            assert!(
                html.contains(
                    model["pages"][id]["body_html"]
                        .as_str()
                        .expect("shared rendered body")
                )
            );
            assert!(html.contains("application/ld+json"));
        }
        assert_eq!(
            std::str::from_utf8(bytes(tree, "sitemap.xml"))
                .expect("generated sitemap XML")
                .matches("<loc>")
                .count(),
            count
        );
        assert_eq!(
            std::str::from_utf8(bytes(tree, "llms.txt"))
                .expect("generated discovery text")
                .lines()
                .filter(|line| line.starts_with("- ["))
                .count(),
            count
        );
        for entry in tree.inventory().entries() {
            if let Some(bytes) = tree.file(&entry.path) {
                let text = String::from_utf8_lossy(bytes);
                for absent in [
                    "content:one",
                    "EXCLUDED PRIVATE ASSET CANARY",
                    "bad.js",
                    "private.txt",
                ] {
                    assert!(!text.contains(absent), "{} leaked {absent}", entry.path);
                }
            }
        }
        assert!(
            tree.file(
                &"guide/index.html"
                    .parse()
                    .expect("valid logical fixture path")
            )
            .is_none()
        );
        assert!(
            tree.file(&"bad.js".parse().expect("valid logical fixture path"))
                .is_none()
        );
        if site == "site:one" {
            assert_eq!(bytes(tree, "assets/public.txt"), b"retained public asset");
            let feed: Value =
                serde_json::from_slice(bytes(tree, "feed.json")).expect("selected feed");
            assert_eq!(feed["items"].as_array().expect("feed item array").len(), 1);
            assert_eq!(
                feed["items"][0]["_gazette"]["content"],
                model["pages"]["content:kept"]["content"]
            );
        }
    }
    let expected = serde_json::to_value(rendered.inspect()).expect("complete private set receipt");
    assert_eq!(
        expected["selection"],
        serde_json::to_value(&intent).expect("complete selection intent")
    );
    let mut input = selected_request(&config, &intent);
    for site in input["sites"]
        .as_array_mut()
        .expect("explicit site requests")
    {
        site["graph"] = json!({"output":"graph-view.html"});
    }
    let response = cli(&input.to_string(), &["render-sites", "--local"]);
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI private set receipt"),
        expected
    );
    fs::write(
        fixture.directory.path().join("one/kept.md"),
        "changed after acquisition",
    )
    .expect("disposable native fixture mutation");
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(intent);
    for site in selected.sites.values_mut() {
        site.graph = Some(build::GraphRenderOptions {
            output: &graph_output,
            limits: Default::default(),
        });
    }
    let repeated = build::render_publications(&inputs, selected, Default::default())
        .expect("candidate uses retained bytes only");
    for site in SITES {
        same_tree(
            repeated.sites()[&key(site)]
                .as_ref()
                .expect("retained successful site")
                .site()
                .output(),
            rendered.sites()[&key(site)]
                .as_ref()
                .expect("retained successful site")
                .site()
                .output(),
        );
    }
}

#[test]
fn cross_site_excluded_relations_and_known_links_fail_before_any_site_result() {
    for (other, code) in [
        (
            OTHER.replacen("+++\n", "+++\nrelated=['content:one']\n", 1),
            "selection_content",
        ),
        (
            format!("{OTHER}\n[Excluded canonical](https://one.example/mount/guide/?q=1#part)\n"),
            "selection_excluded_link",
        ),
        (
            format!("{OTHER}\n[Excluded alias](https://one.example/mount/old.html#part)\n"),
            "selection_excluded_link",
        ),
    ] {
        let fixture = Fixture::new(&[("other/page.md", other.as_bytes())]);
        let config = fixture.config();
        let inputs = acquire_set(&config, &SITES);
        let page = "missing.html".parse().expect("valid logical fixture path");
        let alias = "alias.html".parse().expect("valid logical fixture path");
        let model = "model.json".parse().expect("valid logical fixture path");
        let mut selected = options(&SITES, &page, &alias, &model);
        selected.selection = Some(selection());
        let error = build::render_publications(&inputs, selected, Default::default())
            .expect_err("cross-site candidate closure precedes missing templates");
        assert!(matches!(error, build::PublicationSetError::Candidate(_)));
        assert_eq!(error.code(), code);
        let mut input = selected_request(&config, &selection());
        for site in input["sites"]
            .as_array_mut()
            .expect("explicit site requests")
        {
            site["model_output"] = if site["site"] == "site:one" {
                json!("guide/index.html")
            } else {
                json!("elsewhere/index.html")
            };
        }
        let response = cli(&input.to_string(), &["render-sites", "--local"]);
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        assert!(String::from_utf8_lossy(&response.stderr).contains(code));
    }
}

#[test]
fn complete_candidate_decisions_are_required_before_rendering_not_inferred_per_site() {
    let fixture = Fixture::new(&[("one/private.txt", b"explicit independent asset")]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &SITES);
    let page = "missing.html".parse().expect("valid logical fixture path");
    let alias = "alias.html".parse().expect("valid logical fixture path");
    let model = "model.json".parse().expect("valid logical fixture path");
    let mut complete = selection();
    complete.assets.insert(
        "one/private.txt"
            .parse()
            .expect("valid logical fixture path"),
        Disposition::Exclude,
    );
    let mut missing_content = complete.clone();
    missing_content.content.remove(&key("content:other"));
    let mut unknown_content = complete.clone();
    unknown_content
        .content
        .insert(key("content:unknown"), Disposition::Include);
    let mut draft = complete.clone();
    draft
        .content
        .insert(key("content:one"), Disposition::Include);
    let mut missing_asset = complete.clone();
    missing_asset.assets.clear();
    let mut unknown_asset = complete.clone();
    unknown_asset.assets.insert(
        "one/unknown.txt"
            .parse()
            .expect("valid logical fixture path"),
        Disposition::Exclude,
    );
    for (intent, code) in [
        (missing_content, "selection_content"),
        (unknown_content, "selection_content"),
        (draft, "selection_draft"),
        (missing_asset, "selection_asset"),
        (unknown_asset, "selection_asset"),
    ] {
        let mut selected = options(&SITES, &page, &alias, &model);
        selected.selection = Some(intent.clone());
        let error = build::render_publications(&inputs, selected, Default::default())
            .expect_err("complete candidate required before presentation");
        assert!(matches!(error, build::PublicationSetError::Candidate(_)));
        assert_eq!(error.code(), code);
        let response = cli(
            &selected_request(&config, &intent).to_string(),
            &["render-sites", "--local"],
        );
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        assert!(String::from_utf8_lossy(&response.stderr).contains(code));
    }
}

#[test]
fn empty_selected_site_retains_failed_membership_and_the_complete_shared_intent() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &SITES);
    let page = "page.html".parse().expect("valid logical fixture path");
    let alias = "alias.html".parse().expect("valid logical fixture path");
    let model = "model.json".parse().expect("valid logical fixture path");
    let mut intent = selection();
    intent
        .content
        .insert(key("content:other"), Disposition::Exclude);
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(intent.clone());
    let rendered = build::render_publications(&inputs, selected, Default::default())
        .expect("closed candidate keeps exact site outcomes");
    assert!(!rendered.is_complete());
    assert_eq!(
        rendered.sites().keys().cloned().collect::<Vec<_>>(),
        SITES.map(key)
    );
    let first = rendered.sites()[&key("site:one")]
        .as_ref()
        .expect("selected home remains");
    assert!(std::ptr::eq(
        rendered.selection().expect("set selection retained"),
        first.selection().expect("same site selection retained")
    ));
    assert_eq!(
        rendered.sites()[&key("site:other")]
            .as_ref()
            .expect_err("requested site cannot disappear")
            .code(),
        "site_empty"
    );
    let report = serde_json::to_value(rendered.inspect()).expect("complete private set receipt");
    assert_eq!(
        report["selection"],
        serde_json::to_value(&intent).expect("complete selection intent")
    );
    assert_eq!(report["sites"]["site:other"]["status"], "failed");
    assert!(report["sites"]["site:other"].get("inventory").is_none());
    let response = cli(
        &selected_request(&config, &intent).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI private set receipt"),
        report
    );
}

#[test]
fn all_failed_presentation_still_checks_candidate_against_retained_content() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    fs::write(
        fixture.directory.path().join("theme/templates/page.html"),
        "dirty shared presentation",
    )
    .expect("disposable native fixture mutation");
    let inputs = acquire_set(&config, &SITES);
    assert!(inputs.sites().values().all(Result::is_err));
    assert_eq!(inputs.sources().len(), 2);
    let page = "page.html".parse().expect("valid logical fixture path");
    let alias = "alias.html".parse().expect("valid logical fixture path");
    let model = "model.json".parse().expect("valid logical fixture path");
    let mut invalid = selection();
    invalid
        .content
        .insert(key("content:one"), Disposition::Include);
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(invalid.clone());
    let error = build::render_publications(&inputs, selected, Default::default())
        .expect_err("all failed Theme sources cannot bypass candidate validation");
    assert_eq!(error.code(), "selection_draft");
    assert!(matches!(error, build::PublicationSetError::Candidate(_)));
    let response = cli(
        &selected_request(&config, &invalid).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert!(response.stdout.is_empty());
    assert!(String::from_utf8_lossy(&response.stderr).contains("selection_draft"));
    let intent = selection();
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(intent.clone());
    let rendered = build::render_publications(&inputs, selected, Default::default())
        .expect("valid candidate retains all presentation failures");
    assert!(!rendered.is_complete());
    assert_eq!(
        rendered.sites().keys().cloned().collect::<Vec<_>>(),
        SITES.map(key)
    );
    assert!(rendered.sites().values().all(Result::is_err));
    let report = serde_json::to_value(rendered.inspect()).expect("complete private set receipt");
    assert_eq!(
        report["selection"],
        serde_json::to_value(&intent).expect("complete selection intent")
    );
    let response = cli(
        &selected_request(&config, &intent).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&response.stdout).expect("CLI private set receipt"),
        report
    );
    fs::write(
        fixture.directory.path().join("one/page.md"),
        "changed after retained capture",
    )
    .expect("disposable native fixture mutation");
    let mut selected = options(&SITES, &page, &alias, &model);
    selected.selection = Some(intent);
    let repeated = build::render_publications(&inputs, selected, Default::default())
        .expect("all-failed candidate uses captured required content without reread");
    assert_eq!(
        serde_json::to_value(repeated.inspect()).expect("repeated private receipt"),
        report
    );
}

#[test]
fn malformed_selection_maps_and_duplicate_keys_fail_before_workspace_or_native_reads() {
    let invalid_workspace = "private-canary invalid TOML";
    for selection in [
        Value::Null,
        json!([]),
        json!({"content": {}}),
        json!({"assets": {}}),
        json!({"content": [], "assets": {}}),
        json!({"content": {"not-a-key": "include"}, "assets": {}}),
        json!({"content": {"content:one": "approve"}, "assets": {}}),
        json!({"content": {"content:one": true}, "assets": {}}),
        json!({"content": {"content:one": null}, "assets": {}}),
        json!({"content": {"content:one": {"include": null}}, "assets": {}}),
        json!({"content": {}, "assets": {"../outside": "include"}}),
        json!({"content": {}, "assets": {}, "unknown": "private-canary"}),
    ] {
        let mut input = request(invalid_workspace, &SITES);
        input["selection"] = selection;
        let response = cli(&input.to_string(), &["render-sites", "--local"]);
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        let message = String::from_utf8_lossy(&response.stderr);
        assert!(message.contains("publication_set_input"), "{message}");
        assert!(!message.contains("private-canary"));
    }
    for raw in [
        r#"{"content":{"content:one":"include","content:one":"exclude"},"assets":{}}"#,
        r#"{"content":{"content:one":"include","content:\u006fne":"exclude"},"assets":{}}"#,
        r#"{"content":{},"assets":{"one/a.txt":"include","one/a.txt":"exclude"}}"#,
    ] {
        let mut input = request(invalid_workspace, &SITES);
        input["selection"] = json!("REPLACE_SELECTION");
        let encoded = input.to_string().replace("\"REPLACE_SELECTION\"", raw);
        let response = cli(&encoded, &["render-sites", "--local"]);
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        let message = String::from_utf8_lossy(&response.stderr);
        assert!(message.contains("publication_set_input"), "{message}");
        assert!(!message.contains("private-canary"));
    }
}
