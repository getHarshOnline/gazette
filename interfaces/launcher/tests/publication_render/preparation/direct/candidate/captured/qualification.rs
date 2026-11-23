//! Qualification-only uses native candidate/output checks, never staging or release effects.

use super::*;

#[path = "qualification/carriers.rs"]
mod carriers;

fn selection() -> Value {
    json!({"content":{"collection:home":"include","content:one":"include","content:other":"include"},"assets":{}})
}

pub(super) fn qualification_options() -> build::QualificationOptions {
    build::QualificationOptions {
        allow_read: true,
        policy: gazette_sdk::process::Options {
            allow_execute: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

pub(super) fn recipe(value: &Value) -> build::QualificationRecipe {
    build::QualificationRecipe::parse_json(&serde_json::to_vec(value).unwrap(), 4 << 20, 2)
        .expect("one native qualification recipe")
}

fn expectation(
    rendered: &build::PublicationRender<'_>,
    candidate: &build::CandidatePolicyRequirement,
) -> build::PreparationQualification {
    // Scanner/profile identities do not depend on the tested output. Derive them
    // from an independent clean render so a canary reaches real qualification.
    // Rendering identity below still binds the actual, possibly forbidden bytes.
    let profile_fixture = policy_fixture(&[]);
    let profile_inputs = crate::acquire(&profile_fixture.config());
    let profile_render = crate::render(&profile_inputs).unwrap();
    let scan = build::check_disclosure_policy(
        profile_render.site().output(),
        profile_render.site().media_types(),
        &disclosure_policy(),
        Default::default(),
    )
    .unwrap();
    let scan = serde_json::to_value(scan).unwrap();
    let raster_profiles = ["files", "embedded"]
        .into_iter()
        .flat_map(|key| scan[key].as_array().unwrap())
        .filter_map(|file| {
            let raster = file["decoding"].get("raster-metadata")?;
            Some((
                raster["format"].as_str()?.to_owned(),
                raster["profile"].as_str()?.to_owned(),
            ))
        })
        .collect();
    build::PreparationQualification {
        scanner: scan["scanner"].as_str().unwrap().into(),
        encoded_profile: scan["encoded_profile"].as_str().unwrap().into(),
        filename_profile: scan["filename_profile"].as_str().unwrap().into(),
        generic_rules_sha256: serde_json::from_value(scan["generic_rules_sha256"].clone()).unwrap(),
        resource_profile: scan["resource_profile"].as_str().unwrap().into(),
        resource_limits_sha256: serde_json::from_value(scan["resource_limits_sha256"].clone())
            .unwrap(),
        exclusion_profile: "gazette-zero-output-exclusion/0.1".into(),
        removed: Vec::new(),
        raster_profiles,
        disclosure_policy_sha256: serde_json::from_value(scan["policy_sha256"].clone()).unwrap(),
        repository_source_provenance_sha256: source::Sha256Digest::of(
            &serde_json::to_vec(&build::repository_source_provenance()).unwrap(),
        ),
        runtime_build_profile: build::runtime_build_profile(),
        rendering_sha256: source::Sha256Digest::of(
            &serde_json::to_vec(rendered.site().evidence()).unwrap(),
        ),
        output_policy: build::OutputPolicyRequirement {
            policy: candidate.policy.clone(),
            command_sha256: candidate.command_sha256,
            response_bytes: 256 << 10,
        },
        candidate_policy: Some(candidate.clone()),
    }
}

pub(in crate::preparation) fn request(
    config: &str,
    candidate: &build::CandidatePolicyRequirement,
) -> Value {
    let inputs = crate::sites::acquire_set(config, &["site:one", "site:other"]);
    let page = "page.html".parse().unwrap();
    let alias = "alias.html".parse().unwrap();
    let model = "model.json".parse().unwrap();
    let mut options =
        crate::sites::yearless_options(&["site:one", "site:other"], &page, &alias, &model);
    options.selection = Some(serde_json::from_value(selection()).unwrap());
    // Independent expected rendering before the qualification operation; no checker,
    // stage or journal is needed to derive the already-owned source/render hashes.
    let renders = build::render_publications(&inputs, options, Default::default()).unwrap();
    let sites=["site:one","site:other"].map(|site| {
        let rendered=renders.sites()[&site.parse().unwrap()].as_ref().unwrap();
        json!({"render":{"site":site,"model_output":"model.json","graph":null},"qualification":expectation(rendered,candidate)})
    });
    json!({"workspace":{"kind":"supplied","source":config},"selection":selection(),"disclosure":disclosure_policy(),"sites":sites})
}

pub(super) fn assert_no_stage(parent: &Path, fixture: &crate::Fixture, original_head: &[u8]) {
    for entry in fs::read_dir(parent).unwrap() {
        let entry = entry.unwrap();
        assert!(
            entry.file_type().unwrap().is_file(),
            "only native checker observation files"
        );
        assert!(
            entry
                .file_name()
                .to_str()
                .unwrap()
                .starts_with("qualification.jsonl")
        );
    }
    assert_eq!(
        fs::read(fixture.directory.path().join(".git/HEAD")).unwrap(),
        original_head
    );
    assert!(!fixture.directory.path().join("journal.sqlite3").exists());
}

#[test]
fn qualification_only_matches_existing_native_certificates_without_stage_or_store() {
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("qualification.jsonl");
    let (config, required) = source_configuration(&fixture, &log, "none");
    let candidate: build::CandidatePolicyRequirement = serde_json::from_value(required).unwrap();
    let request = request(&config, &candidate);
    let before = fs::read(fixture.directory.path().join(".git/HEAD")).unwrap();
    let report = recipe(&request).execute(qualification_options());
    assert!(report.is_complete(), "{report:?}");
    let value = serde_json::to_value(report).unwrap();
    assert_eq!(value["requested"], json!(["site:one", "site:other"]));
    assert!(value["failure"].is_null());
    assert_eq!(value["candidate_checks"].as_object().unwrap().len(), 2);
    let calls = events(&log);
    assert_eq!(
        calls.len(),
        4,
        "one candidate and one output invocation per site"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["format"] == "gazette-candidate-policy/1")
            .count(),
        2
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["format"] == "gazette-output-policy/1")
            .count(),
        2
    );
    assert_no_stage(parent.path(), &fixture, &before);
    for via_mcp in [false, true] {
        let prior = events(&log).len();
        let observed = carriers::run(&request, via_mcp, true, true, true);
        assert_eq!(
            observed, value,
            "same complete SDK/CLI/MCP report and certificates"
        );
        let calls = events(&log);
        assert_eq!(
            calls.len(),
            prior + 4,
            "each explicit operation runs exactly the selected native checks"
        );
        assert_eq!(
            &calls[prior..],
            &calls[..4],
            "exact native requests unchanged across carriers"
        );
        assert_no_stage(parent.path(), &fixture, &before);
    }
    let inputs = crate::sites::acquire_set(&config, &["site:one", "site:other"]);
    let rendered = super::render(&inputs, &candidate);
    for (index, site) in ["site:one", "site:other"].into_iter().enumerate() {
        let required: build::PreparationQualification =
            serde_json::from_value(request["sites"][index]["qualification"].clone()).unwrap();
        let rendered = rendered.sites()[&site.parse().unwrap()].as_ref().unwrap();
        let admission = rendered
            .candidate_admission()
            .expect("required captured all-allow candidate becomes typed admission");
        assert_eq!(
            admission.requirement(),
            Some(
                required
                    .candidate_policy
                    .as_ref()
                    .expect("qualification requires candidate policy")
            )
        );
        assert_eq!(
            admission.check().review().request().site().as_str(),
            site,
            "admission retains the exact selected site review"
        );
        let mut check = build::check_preparation(
            rendered,
            &disclosure_policy(),
            qualification_options().policy,
            Default::default(),
            Some(&required.output_policy),
        )
        .unwrap();
        let certificate = check.qualify(&required, 1 << 20).unwrap();
        assert_eq!(value["sites"][index]["site"], site);
        assert_eq!(
            value["sites"][index]["status"],
            "supported-profile-technically-qualified"
        );
        assert_eq!(
            value["sites"][index]["certificate"],
            json!(certificate),
            "same existing certificate owner"
        );
        assert_eq!(
            value["sites"][index]["certificate"]["output"],
            json!(rendered.site().output().inventory().identity())
        );
    }
    assert_no_stage(parent.path(), &fixture, &before);
}

#[test]
fn omitted_selection_admits_complete_non_draft_content_and_assets_before_policy() {
    use crate::preparation::direct::default_selection;
    let fixture = default_selection::fixture();
    let parent = private_directory();
    let log = parent.path().join("qualification.jsonl");
    let (config, required) = source_configuration_with_base(
        &fixture,
        &default_selection::configuration(&fixture),
        &log,
        "none",
    );
    let candidate: build::CandidatePolicyRequirement =
        serde_json::from_value(required).expect("candidate requirement");
    let site = "site:one".parse().expect("site");
    let inputs = build::acquire_publications(
        &config,
        std::slice::from_ref(&site),
        Default::default(),
        Default::default(),
    )
    .expect("one exact native-documentation site");
    let retained = inputs.sites()[&site].as_ref().expect("site inputs");
    let parsed = retained
        .parse_publication(Default::default(), &gazette_adapter_markdown::Comrak)
        .expect("retained publication syntax");
    let resolved = parsed
        .resolve(Default::default())
        .expect("resolved meaning");
    let selection = resolved
        .authored()
        .non_draft_selection()
        .expect("closed non-draft default");
    default_selection::assert_selection(&selection);
    assert_eq!(
        selection.content[&"doc:native".parse().expect("documentation identity")],
        gazette_sdk::publication::content::Disposition::Include
    );
    for id in ["content:one", "content:other", "collection:home"] {
        assert_eq!(
            selection.content[&id.parse().expect("content identity")],
            gazette_sdk::publication::content::Disposition::Include
        );
    }

    let page = "page.html".parse().expect("page template");
    let alias = "alias.html".parse().expect("alias template");
    let model = "model.json".parse().expect("model output");
    let mut options = crate::sites::yearless_options(&["site:one"], &page, &alias, &model);
    options.selection = Some(selection.clone());
    let expected = build::render_publications(&inputs, options, Default::default())
        .expect("independent selected render");
    let rendered = expected.sites()[&site].as_ref().expect("selected site");
    let output = rendered.site().output();
    assert_eq!(
        output
            .file(&"downloads/required.txt".parse().unwrap())
            .unwrap(),
        default_selection::DOWNLOAD
    );
    assert!(
        output
            .file(&"downloads/unrelated.txt".parse().unwrap())
            .is_none()
    );
    assert!(
        output
            .file(&"archived-extra/index.html".parse().unwrap())
            .is_some()
    );
    assert!(
        output
            .file(&"draft-extra/index.html".parse().unwrap())
            .is_none()
    );
    let request = json!({
        "workspace": {"kind":"supplied","source":config},
        "disclosure": disclosure_policy(),
        "sites": [{
            "render": {"site":"site:one","model_output":"model.json","graph":null},
            "qualification": expectation(rendered, &candidate),
        }],
    });
    assert!(request.get("selection").is_none());
    let report = recipe(&request).execute(qualification_options());
    assert!(report.is_complete(), "{report:?}");
    let value = serde_json::to_value(report).expect("qualification report");
    assert_eq!(value["requested"], json!(["site:one"]));
    assert!(value["failure"].is_null());

    let calls = events(&log);
    assert_eq!(calls.len(), 2, "one candidate and one output check");
    let candidate_call = &calls[0];
    assert_eq!(candidate_call["format"], "gazette-candidate-policy/1");
    assert_eq!(candidate_call["review"]["request"]["site"], "site:one");
    assert_eq!(
        candidate_call["review"]["request"]["selection_sha256"],
        json!(source::Sha256Digest::of(
            &serde_json::to_vec(&selection).expect("selection bytes")
        ))
    );
    let surfaces = candidate_call["review"]["request"]["surfaces"].to_string();
    assert!(surfaces.contains("doc:native"));
    for included in ["content:one", "collection:home", "content:archived-extra"] {
        assert!(
            surfaces.contains(included),
            "included {included} missing from policy"
        );
    }
}

#[test]
fn qualification_late_candidate_or_render_expectation_failure_never_shrinks_the_set() {
    for mode in ["later-hold", "wrong-render"] {
        let fixture = policy_fixture(&[]);
        let parent = private_directory();
        let log = parent.path().join("qualification.jsonl");
        let (config, required) = source_configuration(
            &fixture,
            &log,
            if mode == "later-hold" { mode } else { "none" },
        );
        let candidate = serde_json::from_value(required).unwrap();
        let mut request = request(&config, &candidate);
        if mode == "wrong-render" {
            request["sites"][1]["qualification"]["rendering_sha256"] =
                json!(source::Sha256Digest::of(b"wrong rendering"));
        }
        let before = fs::read(fixture.directory.path().join(".git/HEAD")).unwrap();
        let report = recipe(&request).execute(qualification_options());
        assert!(!report.is_complete());
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["requested"], json!(["site:one", "site:other"]));
        assert_eq!(value["sites"].as_array().unwrap().len(), 2);
        assert_eq!(value["sites"][0]["site"], "site:one");
        assert_eq!(value["sites"][1]["site"], "site:other");
        if mode == "later-hold" {
            assert_eq!(value["failure"]["phase"], "candidate");
            assert_eq!(value["failure"]["site"], "site:other");
            assert!(value["failure"]["completed"].get("site:one").is_some());
            assert!(
                value["failure"]["completed_responses"]
                    .get("site:one")
                    .is_some()
            );
            assert_eq!(value["sites"][0]["status"], "not-run");
            assert_eq!(events(&log).len(), 2);
        } else {
            assert_eq!(value["failure"]["code"], "qualification_render");
            assert_eq!(value["sites"][0]["status"], "not-run");
            assert_eq!(value["sites"][1]["status"], "failed");
            assert!(value["sites"][1].get("certificate").is_none());
            assert!(
                value["sites"][1]["failure"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("preparation_qualification")
            );
            assert!(value["sites"][1]["output_policy"].is_null());
            assert!(value["sites"][0].get("certificate").is_none());
            let calls = events(&log);
            assert_eq!(calls.len(), 2);
            assert!(
                calls
                    .iter()
                    .all(|call| call["format"] == "gazette-candidate-policy/1"),
                "complete identity preflight refuses a known invalid set before any output checker"
            );
        }
        let native_count = events(&log).len();
        for via_mcp in [false, true] {
            let prior = events(&log).len();
            let observed = carriers::run(&request, via_mcp, true, true, false);
            assert_eq!(
                observed, value,
                "incomplete set/effect evidence must survive both carriers"
            );
            let calls = events(&log);
            assert_eq!(calls.len(), prior + native_count);
            assert_eq!(&calls[prior..], &calls[..native_count]);
        }
        assert_no_stage(parent.path(), &fixture, &before);
    }
}

#[test]
fn qualification_refuses_missing_requirements_denied_permissions_and_changed_source_pin_before_effects()
 {
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("qualification.jsonl");
    let (config, required) = source_configuration(&fixture, &log, "none");
    let candidate = serde_json::from_value(required).unwrap();
    let original = request(&config, &candidate);
    let schema = serde_json::to_value(build::QualificationRecipe::input_schema()).unwrap();
    assert!(
        jsonschema::validator_for(&schema)
            .unwrap()
            .is_valid(&original)
    );
    for field in ["candidate_policy", "output_policy"] {
        let mut invalid = original.clone();
        invalid["sites"][0]["qualification"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(
            build::QualificationRecipe::parse_json(
                &serde_json::to_vec(&invalid).unwrap(),
                4 << 20,
                2
            )
            .is_err()
        );
    }
    for (name, value) in [
        ("review_responses", json!({})),
        ("stage_parent", json!(parent.path())),
        ("journal", json!("not-created")),
    ] {
        let mut invalid = original.clone();
        invalid[name] = value;
        assert!(
            build::QualificationRecipe::parse_json(
                &serde_json::to_vec(&invalid).unwrap(),
                4 << 20,
                2
            )
            .is_err()
        );
    }
    for allow_read in [false, true] {
        let mut options = qualification_options();
        options.allow_read = allow_read;
        options.policy.allow_execute = !allow_read;
        let report = recipe(&original).execute(options);
        assert!(!report.is_complete());
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["failure"]["code"], "qualification_permission");
        for via_mcp in [false, true] {
            assert_eq!(
                carriers::run(&original, via_mcp, allow_read, !allow_read, false),
                value
            );
        }
    }
    let mut different_limits = original.clone();
    different_limits["sites"][0]["qualification"]["resource_limits_sha256"] =
        json!(source::Sha256Digest::of(b"independently different limits"));
    different_limits["workspace"]["source"] = json!("invalid configuration must not be read");
    let report = recipe(&different_limits).execute(qualification_options());
    assert!(!report.is_complete());
    assert_eq!(
        serde_json::to_value(report).unwrap()["failure"]["code"],
        "qualification_resources"
    );
    let mut stale = original.clone();
    stale["workspace"]["source"] = json!(config.replace(
        &fixture.commit.to_string(),
        "0000000000000000000000000000000000000000"
    ));
    let report = recipe(&stale).execute(qualification_options());
    assert!(!report.is_complete());
    assert_eq!(
        serde_json::to_value(report).unwrap()["failure"]["phase"],
        "before"
    );
    assert!(!log.exists());
    assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
}

#[test]
fn qualification_rejects_zero_exit_without_response_and_output_canary() {
    for mode in ["empty-response", "canary"] {
        let template = format!(
            "{}{}",
            crate::TEMPLATE,
            if mode == "canary" {
                "gazette_fixture_secret_982731"
            } else {
                ""
            }
        );
        let fixture = policy_fixture(&[("theme/templates/page.html", template.as_bytes())]);
        let parent = private_directory();
        let log = parent.path().join("qualification.jsonl");
        let (mut config, mut required) = source_configuration(&fixture, &log, "none");
        if mode == "empty-response" {
            config = config.replace(
                &serde_json::to_string(CHECKER).unwrap(),
                &serde_json::to_string(
                    "process.stdin.resume(); process.stdin.on('end',()=>process.exit(0));",
                )
                .unwrap(),
            );
            let inputs = crate::sites::acquire_set(&config, &["site:one", "site:other"]);
            required["command_sha256"] = json!(source::Sha256Digest::of(
                &serde_json::to_vec(
                    inputs
                        .configuration()
                        .policy_command()
                        .expect("bound policy command")
                        .unwrap()
                )
                .unwrap()
            ));
        }
        let candidate = serde_json::from_value(required).unwrap();
        let request = request(&config, &candidate);
        let before = fs::read(fixture.directory.path().join(".git/HEAD")).unwrap();
        let report = recipe(&request).execute(qualification_options());
        assert!(!report.is_complete());
        let value = serde_json::to_value(report).unwrap();
        assert_eq!(value["sites"].as_array().unwrap().len(), 2);
        assert!(
            value["sites"]
                .as_array()
                .unwrap()
                .iter()
                .all(|site| site.get("certificate").is_none())
        );
        if mode == "empty-response" {
            assert_eq!(value["failure"]["phase"], "candidate");
            assert_eq!(value["failure"]["evidence"]["response_bytes"], 0);
            assert!(value["failure"]["evidence"]["exit"].is_object());
        } else {
            assert_eq!(value["sites"][0]["status"], "failed");
            assert_eq!(value["sites"][1]["status"], "failed");
            let calls = events(&log);
            assert_eq!(calls.len(), 2, "both sites receive candidate review");
            assert!(
                calls
                    .iter()
                    .all(|call| call["format"] == "gazette-candidate-policy/1"),
                "disclosure refusal must precede every output checker"
            );
        }
        assert_no_stage(parent.path(), &fixture, &before);
    }
}

#[test]
fn qualification_binds_independently_selected_resource_limits_before_scanning() {
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("qualification.jsonl");
    let (config, required) = source_configuration(&fixture, &log, "none");
    let candidate = serde_json::from_value(required).unwrap();
    let mut request = request(&config, &candidate);
    let mut options = qualification_options();
    options.disclosure.html.tokens += 1;
    options.disclosure.encoded_bytes += 1;
    let expected = options.disclosure.resource_limits_sha256();
    for site in request["sites"].as_array_mut().unwrap() {
        site["qualification"]["resource_limits_sha256"] = json!(expected);
    }
    let before = fs::read(fixture.directory.path().join(".git/HEAD")).unwrap();
    let report = recipe(&request).execute(options);
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(events(&log).len(), 4);
    assert_no_stage(parent.path(), &fixture, &before);
}
