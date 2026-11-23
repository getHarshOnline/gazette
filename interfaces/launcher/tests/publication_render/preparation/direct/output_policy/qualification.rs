//! Independent selected expectations qualify the same native preparation and history.

use super::*;
use serde_json::value::RawValue;

pub(in crate::preparation) fn expected(
    rendered: &build::PublicationRender<'_>,
    required: &build::OutputPolicyRequirement,
) -> build::PreparationQualification {
    // Resolve a separate retained render before the operation, never the certificate
    // or journal row being tested. This fixture is not an owner-approved profile.
    let retained = super::super::super::super::worktree::Retained::new(rendered);
    let original = &retained.receipt.record.intent.evidence;
    let fields: BTreeMap<String, &RawValue> =
        serde_json::from_str(original).expect("exact serialized subtrees");
    let scan = build::check_disclosure_policy(
        rendered.site().output(),
        rendered.site().media_types(),
        &disclosure_policy(),
        Default::default(),
    )
    .expect("independent required scan");
    let scan = serde_json::to_value(scan).expect("scan evidence");
    let raster_profiles = ["files", "embedded"]
        .into_iter()
        .flat_map(|key| scan[key].as_array().expect("complete coverage"))
        .filter_map(|file| {
            let raster = file["decoding"].get("raster-metadata")?;
            Some((
                raster["format"].as_str()?.to_owned(),
                raster["profile"].as_str()?.to_owned(),
            ))
        })
        .collect();
    build::PreparationQualification {
        scanner: scan["scanner"].as_str().expect("scanner").into(),
        encoded_profile: scan["encoded_profile"]
            .as_str()
            .expect("encoded profile")
            .into(),
        filename_profile: scan["filename_profile"]
            .as_str()
            .expect("filename profile")
            .into(),
        generic_rules_sha256: serde_json::from_value(scan["generic_rules_sha256"].clone()).unwrap(),
        resource_profile: scan["resource_profile"].as_str().unwrap().into(),
        resource_limits_sha256: serde_json::from_value(scan["resource_limits_sha256"].clone())
            .unwrap(),
        exclusion_profile: "gazette-zero-output-exclusion/0.1".into(),
        removed: Vec::new(),
        raster_profiles,
        disclosure_policy_sha256: serde_json::from_value(scan["policy_sha256"].clone()).unwrap(),
        repository_source_provenance_sha256: source::Sha256Digest::of(
            fields["repository_source_provenance"].get().as_bytes(),
        ),
        runtime_build_profile: build::runtime_build_profile(),
        rendering_sha256: source::Sha256Digest::of(fields["rendering"].get().as_bytes()),
        output_policy: required.clone(),
        candidate_policy: None,
    }
}

#[test]
fn qualified_sdk_and_cli_preparations_reopen_only_under_independent_matching_expectations() {
    let fixture = policy_fixture(&[]);
    for (via_cli, qualify, custom_limits) in [
        (false, true, false),
        (true, true, false),
        (false, false, false),
        (false, true, true),
    ] {
        let root = private_directory();
        let journal = initialize(root.path());
        let parent = private_directory();
        let log = parent.path().join("delivered.json");
        let (config, required) = configuration(&fixture, &log, "allow");
        let inputs = super::super::super::super::acquire(&config);
        let rendered = super::super::super::render(&inputs).expect("independent retained render");
        let mut qualification = expected(&rendered, &required);
        let mut selected_execution = execution();
        if custom_limits {
            selected_execution.preparation.disclosure.html.tokens += 1;
            selected_execution
                .preparation
                .disclosure
                .raster
                .decoded_bytes += 1;
            qualification.resource_limits_sha256 = selected_execution
                .preparation
                .disclosure
                .resource_limits_sha256();
        }
        let mut selected = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        if qualify {
            selected["sites"][0]["qualification"] = json!(qualification);
            // One effective requirement comes from qualification; no duplicate owner.
            selected["sites"][0]
                .as_object_mut()
                .unwrap()
                .remove("output_policy");
        }
        let report = if via_cli {
            let mut arguments = ARGUMENTS.to_vec();
            arguments.push("--allow-policy-execution");
            let (output, report) = result(&selected, &arguments);
            assert!(output.status.success(), "{output:?}; {report}");
            report
        } else {
            let outcome = recipe(&selected).execute(selected_execution);
            assert!(outcome.is_ok(), "{outcome:?}");
            serde_json::to_value(build::LocalPreparationRunReport::new(&outcome)).unwrap()
        };
        assert_eq!(
            report["status"],
            "local-preparations-complete-not-certified"
        );
        let observed =
            store::inspect_preparation(&journal.database, id("one"), None, read()).unwrap();
        let record = observed.record.as_ref().expect("durable preparation");
        assert_original_stage(record, &log);
        let check: Value = serde_json::from_str(&record.check.as_ref().unwrap().evidence).unwrap();
        if qualify {
            let certificate = &check["technical_certificate"];
            assert_eq!(
                certificate["profile"],
                "gazette-immutable-output-technical/0.1"
            );
            assert_eq!(certificate["requirements"], json!(qualification));
            assert_eq!(certificate["output"], json!(record.intent.output));
            assert_eq!(
                certificate["preparation_evidence_sha256"],
                json!(record.intent.evidence_sha256)
            );
        } else {
            assert!(check.get("technical_certificate").is_none());
        }
        let completion = record
            .worktree
            .as_ref()
            .unwrap()
            .completion
            .as_ref()
            .unwrap();
        let mut selection = release::PreparationSelection {
            preparation_id: id("one"),
            site: "site:one".parse().unwrap(),
            domain: "one.example".into(),
            commit: completion.report.commit.clone(),
            expected_remote_oid_or_unborn: release::RemoteState::Unborn,
            output_policy: None,
            qualification: Some(release::PreparationAdmission::authored(
                qualification.clone(),
            )),
        };
        let inspect = |selection: &release::PreparationSelection| {
            release::inspect_preparations(
                release::PreparationBatchRequest {
                    database: &journal.database,
                    expected: observed.journal.header,
                    selections: std::slice::from_ref(selection),
                    protected_roots: &[fixture.directory.path().to_owned()],
                },
                release::PreparationBatchOptions {
                    inspection: release_commit::JournalWorktreeInspectionOptions {
                        read: read(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        };
        if qualify {
            let batch = inspect(&selection).expect("native read-back matches independent profile");
            assert_eq!(
                serde_json::to_value(batch).unwrap()["status"],
                "supported-profile-technical-preparations"
            );
            if custom_limits {
                selection
                    .qualification
                    .as_mut()
                    .unwrap()
                    .as_authored_mut()
                    .unwrap()
                    .resource_limits_sha256 =
                    build::DisclosureProbeLimits::default().resource_limits_sha256();
                assert_eq!(
                    inspect(&selection)
                        .expect_err("default limits cannot admit custom history")
                        .code(),
                    "release_preparation_mismatch"
                );
                selection
                    .qualification
                    .as_mut()
                    .unwrap()
                    .as_authored_mut()
                    .unwrap()
                    .resource_limits_sha256 = qualification.resource_limits_sha256;
            }
            selection
                .qualification
                .as_mut()
                .unwrap()
                .as_authored_mut()
                .unwrap()
                .runtime_build_profile
                .build_profile = "other-profile".into();
            assert_eq!(
                inspect(&selection)
                    .expect_err("different running build profile refuses release admission")
                    .code(),
                "release_preparation_selection"
            );
            selection
                .qualification
                .as_mut()
                .unwrap()
                .as_authored_mut()
                .unwrap()
                .runtime_build_profile = build::runtime_build_profile();
            selection
                .qualification
                .as_mut()
                .unwrap()
                .as_authored_mut()
                .unwrap()
                .rendering_sha256 = source::Sha256Digest::of(b"other rendering");
        }
        assert_eq!(
            inspect(&selection)
                .expect_err("missing or mismatched certificate refuses")
                .code(),
            "release_preparation_mismatch"
        );
        selection.qualification = None;
        let batch = inspect(&selection).expect("unqualified inspection remains explicit");
        assert_eq!(
            serde_json::to_value(batch).unwrap()["status"],
            "local-preparations-inspected-not-certified"
        );
        assert_eq!(
            store::inspect_preparation(
                &journal.database,
                id("one"),
                Some(&observed.journal.header),
                read()
            )
            .unwrap()
            .record
            .as_ref(),
            Some(record)
        );
    }
}

#[test]
fn qualification_failures_preserve_completed_checks_without_reinvoking_the_checker() {
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "allow");
    let inputs = super::super::super::super::acquire(&config);
    let rendered = super::super::super::render(&inputs).unwrap();
    let qualification = expected(&rendered, &required);
    let mut checks = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution().preparation.policy,
        Default::default(),
        Some(&required),
    )
    .unwrap();
    let response = fs::read(log.with_extension("json.response")).unwrap();
    let mut wrong = qualification.clone();
    wrong.repository_source_provenance_sha256 =
        source::Sha256Digest::of(b"wrong repository source provenance");
    assert!(checks.qualify(&wrong, 1 << 20).is_err());
    assert!(checks.certificate().is_none());
    let certificate = checks.qualify(&qualification, 1 << 20).unwrap().clone();
    assert_eq!(
        checks.qualify(&qualification, 1 << 20).unwrap(),
        &certificate
    );
    for index in 0..12 {
        let mut wrong = qualification.clone();
        match index {
            0 => wrong.scanner = "other-scanner".into(),
            1 => wrong.encoded_profile = "other-encoded-profile".into(),
            2 => wrong.filename_profile = "other-filenames/1".into(),
            3 => wrong.generic_rules_sha256 = source::Sha256Digest::of(b"other rules"),
            4 => wrong.disclosure_policy_sha256 = source::Sha256Digest::of(b"other disclosure"),
            5 => wrong.runtime_build_profile.build_profile = "other-profile".into(),
            6 => wrong.runtime_build_profile.compilation_target = "other-target".into(),
            7 => wrong.rendering_sha256 = source::Sha256Digest::of(b"other rendering"),
            8 => wrong.exclusion_profile = "other-exclusion-profile/1".into(),
            9 => wrong.removed.push("guide/index.html".parse().unwrap()),
            10 => {
                wrong
                    .raster_profiles
                    .insert("png".into(), "other-raster/1".into());
            }
            _ => wrong.output_policy.policy.revision = "revision:other".parse().unwrap(),
        }
        assert!(checks.qualify(&wrong, 1 << 20).is_err());
        assert_eq!(checks.certificate(), Some(&certificate));
    }
    assert!(checks.qualify(&qualification, 1).is_err());
    assert_eq!(checks.certificate(), Some(&certificate));
    assert_eq!(
        fs::read(log.with_extension("json.response")).unwrap(),
        response
    );
}

#[test]
fn qualification_refuses_a_scan_run_with_widened_resource_ceilings() {
    let fixture = policy_fixture(&[]);
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "allow");
    let inputs = super::super::super::super::acquire(&config);
    let rendered = super::super::super::render(&inputs).unwrap();
    let qualification = expected(&rendered, &required);
    let mut limits = build::DisclosureProbeLimits::default();
    limits.work_bytes += 1;
    let mut checks = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution().preparation.policy,
        limits,
        Some(&required),
    )
    .expect("wider finite diagnostic scan completes");
    assert!(checks.qualify(&qualification, 1 << 20).is_err());
    assert!(checks.certificate().is_none());
}

#[test]
fn qualification_binds_the_independently_selected_native_raster_profile_exactly() {
    let png = crate::probe::raster::decode(crate::probe::raster::CLEAN);
    let fixture = policy_fixture(&[("one/image.png", &png)]);
    let parent = private_directory();
    let log = parent.path().join("delivered.json");
    let (config, required) = configuration(&fixture, &log, "allow");
    let config = config.replace(
        "authored=[{source='source:one',target='one'}]",
        "authored=[{source='source:one',target='one',static_assets={route_prefix='/media/authored/'}}]",
    );
    let inputs = super::super::super::super::acquire(&config);
    let rendered = super::super::super::render(&inputs).expect("retained raster render");
    let qualification = expected(&rendered, &required);
    let selected =
        gazette_sdk::raster::metadata(gazette_sdk::raster::Format::Png, &png, Default::default())
            .expect("independently selected native raster profile");
    assert_eq!(
        qualification.raster_profiles,
        BTreeMap::from([("png".into(), selected.tool.to_owned())])
    );

    let mut checks = build::check_preparation(
        &rendered,
        &disclosure_policy(),
        execution().preparation.policy,
        Default::default(),
        Some(&required),
    )
    .expect("complete retained raster checks");
    assert!(checks.qualify(&qualification, 1 << 20).is_ok());

    let mut missing = qualification.clone();
    missing.raster_profiles.clear();
    assert!(checks.qualify(&missing, 1 << 20).is_err());
    let mut alternate = qualification;
    alternate
        .raster_profiles
        .insert("png".into(), "other-raster/1".into());
    assert!(checks.qualify(&alternate, 1 << 20).is_err());
}

#[test]
fn conflicting_policy_or_wrong_render_or_build_profile_cannot_start_effects() {
    let fixture = policy_fixture(&[]);
    for mismatch in [
        "scanner",
        "encoded-profile",
        "filename-profile",
        "generic-rules",
        "resource-profile",
        "resource-limits",
        "exclusion-profile",
        "removed",
        "render",
        "policy",
        "build-profile",
        "compilation-target",
    ] {
        let root = private_directory();
        let journal = initialize(root.path());
        let before = fs::read(&journal.database).unwrap();
        let parent = private_directory();
        let log = parent.path().join("must-not-run.json");
        let (config, required) = configuration(&fixture, &log, "allow");
        let inputs = super::super::super::super::acquire(&config);
        let rendered = super::super::super::render(&inputs).unwrap();
        let mut qualification = expected(&rendered, &required);
        match mismatch {
            "scanner" => qualification.scanner = "other-scanner/1".into(),
            "encoded-profile" => qualification.encoded_profile = "other-encoded/1".into(),
            "filename-profile" => qualification.filename_profile = "other-filenames/1".into(),
            "generic-rules" => {
                qualification.generic_rules_sha256 =
                    source::Sha256Digest::of(b"other generic rules")
            }
            "resource-profile" => qualification.resource_profile = "other-resources/1".into(),
            "resource-limits" => {
                qualification.resource_limits_sha256 = source::Sha256Digest::of(b"other limits")
            }
            "exclusion-profile" => {
                qualification.exclusion_profile = "other-exclusion-profile/1".into()
            }
            "removed" => qualification
                .removed
                .push("guide/index.html".parse().unwrap()),
            "render" => {
                qualification.rendering_sha256 = source::Sha256Digest::of(b"different render")
            }
            "policy" => {
                qualification.output_policy.policy.revision = "revision:other".parse().unwrap()
            }
            "build-profile" => {
                qualification.runtime_build_profile.build_profile = "other-profile".into()
            }
            "compilation-target" => {
                qualification.runtime_build_profile.compilation_target = "other-target".into()
            }
            _ => unreachable!(),
        }
        let mut selected = request(
            &config,
            root.path(),
            journal.header,
            parent.path(),
            &required,
        );
        selected["sites"][0]["qualification"] = json!(qualification);
        let outcome = build::LocalPreparationRecipe::parse_json(
            &serde_json::to_vec(&selected).unwrap(),
            4 << 20,
            2,
        )
        .map(|recipe| recipe.execute(execution()));
        assert!(match outcome {
            Ok(outcome) => outcome.is_err(),
            Err(_) => true,
        });
        unchanged(&journal, id("one"), &before);
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}
