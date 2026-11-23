//! Complete site-set selection, source, output and transport limits.

use super::*;

#[test]
fn invalid_site_sets_are_rejected_before_compilation_or_native_acquisition() {
    for selected in [vec![], vec![key("site:one"), key("site:one")]] {
        let error = build::acquire_publications(
            "not valid TOML",
            &selected,
            toml::Limits::default(),
            Default::default(),
        )
        .expect_err("selection must fail before the deliberately invalid configuration");
        assert!(matches!(error, build::PublicationSetError::Selection(_)));
        assert_eq!(error.code(), "publication_set_selection");
    }
    let error = build::acquire_publications(
        "not valid TOML",
        &SITES.map(key),
        toml::Limits::default(),
        build::PublicationSetAcquisitionLimits {
            sites: 1,
            ..Default::default()
        },
    )
    .expect_err("site limit applies before compilation");
    assert!(matches!(error, build::PublicationSetError::Selection(_)));

    let fixture = Fixture::new(&[]);
    let missing_root = fixture.directory.path().join("nonexistent-repository");
    let unavailable = fixture.config().replace(
        fixture.directory.path().to_str().expect("fixture root"),
        missing_root.to_str().expect("unavailable root"),
    );
    let error = build::acquire_publications(
        &unavailable,
        &[key("site:unknown")],
        toml::Limits::default(),
        Default::default(),
    )
    .expect_err("unknown site is rejected before opening unavailable source roots");
    assert!(matches!(
        error,
        build::PublicationSetError::Selection("effective build selection names an undeclared site")
    ));
    assert_eq!(error.code(), "publication_set_selection");
    let response = cli(
        &request(&unavailable, &["site:unknown"]).to_string(),
        &["render-sites", "--local"],
    );
    assert!(!response.status.success());
    assert!(
        response.stdout.is_empty(),
        "no set result exists after selection failure"
    );
    let diagnostic = String::from_utf8_lossy(&response.stderr);
    assert!(diagnostic.contains("publication_set_selection"));
    assert!(diagnostic.contains("effective build selection names an undeclared site"));
}

#[test]
fn rendering_requires_options_for_exactly_the_acquired_set() {
    let fixture = Fixture::new(&[]);
    let inputs = acquire_set(&fixture.config(), &SITES);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    for selected in [
        vec![],
        vec!["site:one"],
        vec!["site:one", "site:other", "site:unknown"],
    ] {
        let error = build::render_publications(
            &inputs,
            options(&selected, &page, &alias, &model),
            Default::default(),
        )
        .expect_err("missing or additional options cannot shrink or expand membership");
        assert!(matches!(error, build::PublicationSetError::Selection(_)));
        assert_eq!(error.code(), "publication_set_selection");
    }
}

#[test]
fn aggregate_source_limit_counts_the_shared_union_not_one_budget_per_site() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let mut limits = build::PublicationSetAcquisitionLimits::default();
    limits.sources.sources = 3;
    let inputs =
        build::acquire_publications(&config, &SITES.map(key), toml::Limits::default(), limits)
            .expect("one shared Theme plus two authored sources fits exactly three source keys");
    let first = inputs.sites()[&key("site:one")]
        .as_ref()
        .expect("first acquired site");
    let second = inputs.sites()[&key("site:other")]
        .as_ref()
        .expect("second acquired site");
    assert_eq!(first.sources().len(), 3);
    assert_eq!(second.sources().len(), 3);
    assert!(
        std::ptr::eq(first.sources(), second.sources()),
        "site views share captured ownership"
    );
    limits.sources.sources = 2;
    let error =
        build::acquire_publications(&config, &SITES.map(key), toml::Limits::default(), limits)
            .expect_err("the third source cannot escape the union budget");
    assert!(matches!(error, build::PublicationSetError::Acquisition(_)));
}

#[test]
fn aggregate_output_limits_keep_successful_bytes_and_failed_membership_in_key_order() {
    let fixture = Fixture::new(&[]);
    let config = fixture.config();
    let inputs = acquire_set(&config, &["site:other", "site:one"]);
    let page = "page.html".parse().expect("page");
    let alias = "alias.html".parse().expect("alias");
    let model = "model.json".parse().expect("model");
    let baseline = build::render_publications(
        &inputs,
        options(&SITES, &page, &alias, &model),
        Default::default(),
    )
    .expect("baseline shared phase");
    assert!(baseline.is_complete());
    let bytes: u64 = baseline
        .sites()
        .values()
        .map(|result| {
            result
                .as_ref()
                .expect("baseline success")
                .site()
                .output()
                .inventory()
                .total_bytes()
        })
        .sum();
    let entries: usize = baseline
        .sites()
        .values()
        .map(|result| {
            result
                .as_ref()
                .expect("baseline success")
                .site()
                .output()
                .inventory()
                .entries()
                .len()
        })
        .sum();
    let exact = build::PublicationSetRenderLimits {
        bytes,
        entries,
        ..Default::default()
    };
    let complete =
        build::render_publications(&inputs, options(&SITES, &page, &alias, &model), exact)
            .expect("exact aggregate ceilings");
    assert!(complete.is_complete());
    assert_eq!(
        serde_json::to_value(complete.inspect()).expect("exact receipt"),
        serde_json::to_value(baseline.inspect()).expect("baseline receipt"),
    );
    for (limits, flag, value) in [
        (
            build::PublicationSetRenderLimits {
                bytes: bytes - 1,
                ..exact
            },
            "--max-total-output-bytes",
            (bytes - 1).to_string(),
        ),
        (
            build::PublicationSetRenderLimits {
                entries: entries - 1,
                ..exact
            },
            "--max-total-output-entries",
            (entries - 1).to_string(),
        ),
    ] {
        let bounded =
            build::render_publications(&inputs, options(&SITES, &page, &alias, &model), limits)
                .expect("aggregate exhaustion is represented as a site-local failure");
        assert!(!bounded.is_complete());
        assert_eq!(
            bounded.sites().keys().cloned().collect::<Vec<_>>(),
            SITES.map(key)
        );
        let successful = bounded.sites()[&key("site:one")]
            .as_ref()
            .expect("first key fits");
        same_tree(
            successful.site().output(),
            baseline.sites()[&key("site:one")]
                .as_ref()
                .expect("baseline first")
                .site()
                .output(),
        );
        let error = bounded.sites()[&key("site:other")]
            .as_ref()
            .expect_err("second key exceeds remainder");
        assert!(
            matches!(
                error,
                build::PublicationSiteError::Render(build::PublicationRenderError::Site(
                    build::SiteRenderError::Output(_) | build::SiteRenderError::Render(_)
                )),
            ),
            "original site output failure remains typed: {error:?}"
        );
        assert!(successful.site().output().inventory().total_bytes() <= limits.bytes);
        assert!(successful.site().output().inventory().entries().len() <= limits.entries);
        let response = cli(
            &request(&config, &["site:other", "site:one"]).to_string(),
            &["render-sites", "--local", flag, &value],
        );
        assert!(!response.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&response.stdout).expect("bounded CLI outcome"),
            serde_json::to_value(bounded.inspect()).expect("bounded SDK outcome"),
        );
    }
}

#[test]
fn malformed_or_conflicting_shared_content_is_a_whole_operation_failure() {
    let duplicate = OTHER.replace("id='content:other'", "id='content:one'");
    for (document, syntax_failure) in [("+++\n[content\n+++\n", true), (duplicate.as_str(), false)]
    {
        let fixture = Fixture::new(&[("other/page.md", document.as_bytes())]);
        let config = fixture.config();
        let inputs = acquire_set(&config, &SITES);
        let page = "page.html".parse().expect("page");
        let alias = "alias.html".parse().expect("alias");
        let model = "model.json".parse().expect("model");
        let error = build::render_publications(
            &inputs,
            options(&SITES, &page, &alias, &model),
            Default::default(),
        )
        .expect_err("the common graph cannot be reconstructed as a successful subset");
        if syntax_failure {
            assert!(
                matches!(error, build::PublicationSetError::Input(_)),
                "{error:?}"
            );
        } else {
            assert!(
                matches!(error, build::PublicationSetError::Resolution(_)),
                "{error:?}"
            );
        }
        let response = cli(
            &request(&config, &SITES).to_string(),
            &["render-sites", "--local"],
        );
        assert!(!response.status.success());
        assert!(
            response.stdout.is_empty(),
            "shared failure returns no invented per-site results"
        );
        assert!(String::from_utf8_lossy(&response.stderr).contains(error.code()));
    }
}

#[test]
fn cli_set_transport_is_strict_private_and_requires_explicit_local_mode() {
    let fixture = Fixture::new(&[]);
    let valid = request(&fixture.config(), &SITES);
    let mut unknown_field = valid.clone();
    unknown_field["sites"][0]["unknown"] = json!("private-canary");
    let mut incomplete_graph = valid.clone();
    incomplete_graph["sites"][0]["graph"] = json!({"template": "graph.html"});
    let mut wrong_format = valid.clone();
    wrong_format["format"] = json!("private-canary");
    let invalid_workspace = "private-canary not valid TOML";
    let root_array = json!([
        "gazette-local-publication-set/1",
        invalid_workspace,
        valid["sites"],
    ]);
    let mut site_array = request(invalid_workspace, &["site:one"]);
    site_array["sites"][0] = json!(["site:one", "page.html", "alias.html", "model.json", null,]);
    let mut graph_array = request(invalid_workspace, &["site:one"]);
    graph_array["sites"][0]["graph"] = json!(["graph.html", "graph-view.html"]);
    for input in [
        request("not valid TOML", &[]),
        request("not valid TOML", &["site:one", "site:one"]),
        unknown_field,
        incomplete_graph,
        wrong_format,
        root_array,
        site_array,
        graph_array,
    ] {
        let response = cli(&input.to_string(), &["render-sites", "--local"]);
        assert!(!response.status.success());
        assert!(response.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&response.stderr);
        assert!(diagnostic.contains("publication_set_input"), "{diagnostic}");
        assert!(!diagnostic.contains("private-canary"));
    }
    let response = cli(&valid.to_string(), &["render-sites"]);
    assert!(!response.status.success());
    assert!(response.stdout.is_empty());
    assert!(String::from_utf8_lossy(&response.stderr).contains("--local"));
    let response = cli(
        &valid.to_string(),
        &["render-sites", "--local", "--max-source-bytes", "1"],
    );
    assert!(!response.status.success());
    assert!(response.stdout.is_empty());
    assert!(String::from_utf8_lossy(&response.stderr).contains("publication_set_input_limit"));
}

#[test]
fn explicit_corpus_byte_limit_preserves_default_and_accepts_larger_media_sets() {
    let padding = vec![0_u8; 16 * 1024 * 1024];
    let fixture = Fixture::new(&[("one/padding.png", &padding)]);
    let config = fixture.config().replace(
        "target='one'}",
        "target='one',static_assets={route_prefix='/media/'}}",
    );
    let input = request(&config, &SITES).to_string();
    let default = cli(&input, &["render-sites", "--local"]);
    assert!(!default.status.success());
    assert!(String::from_utf8_lossy(&default.stderr).contains("corpus_limit"));
    // The whole corpus includes authored pages as well as the opaque media bytes.
    let exact_media = cli(
        &input,
        &["render-sites", "--local", "--max-corpus-bytes", "16777216"],
    );
    assert!(!exact_media.status.success());
    let diagnostic = String::from_utf8_lossy(&exact_media.stderr);
    assert!(diagnostic.contains("corpus_limit"), "{diagnostic}");
    assert!(
        diagnostic.contains("corpus.source_bytes (--max-corpus-bytes)"),
        "{diagnostic}"
    );
    assert!(
        diagnostic.contains("maximum 16777216 bytes"),
        "{diagnostic}"
    );
    let raised = cli(
        &input,
        &["render-sites", "--local", "--max-corpus-bytes", "17825792"],
    );
    assert!(
        raised.status.success(),
        "stderr: {} stdout: {}",
        String::from_utf8_lossy(&raised.stderr),
        String::from_utf8_lossy(&raised.stdout)
    );
    let report: Value = serde_json::from_slice(&raised.stdout).expect("native report");
    assert_eq!(report["status"], "local_set_rendered");
    assert_eq!(
        report["sites"].as_object().expect("complete sites").len(),
        2
    );
}
