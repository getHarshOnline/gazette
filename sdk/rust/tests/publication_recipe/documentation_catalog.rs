//! Configured documentation follows native publication acquisition without a second content graph.

use super::*;
use std::collections::BTreeMap;

fn configured() -> (Fixture, TempDir, String) {
    let fixture = Fixture::new(&["one"]);
    let docs = tempdir().unwrap();
    fs::write(
        docs.path().join("guide.md"),
        "# Guide\n\nOriginal documentation words.\n",
    )
    .unwrap();
    fs::write(
        docs.path().join("start.md"),
        "# Start\n\nManual entrance.\n",
    )
    .unwrap();
    let repository = "documentation_repository={name='reading-docs',path='/not-opened/reading-docs',public_base_url='https://docs.example/v2/',build={target={kind='mintlify',theme='mint',primary_color='#123456'},modules=[{project='project:library',slug='library',title='Library',manual={source='source:docs'},navigation=[{kind='manual',title='Guide',pages=[{title='Start',route='library/start',input={kind='file',path='start.md'}}]},{kind='reference',slug='guide',title='Guide input',input={kind='artifact',source='source:docs',selection={kind='file',path='guide.md'},format='markdown'}}]}]}}";
    let mut workspace = fixture.workspace.replacen(
        "public_base_url='https://one.example/mount/'",
        &format!("public_base_url='https://one.example/mount/'\n{repository}"),
        1,
    );
    workspace.push_str(&format!("\n[[projects]]\nid='project:library'\nlifecycle='active'\nvisibility='public'\navailability='available'\n[[sources]]\nid='source:docs'\nlocation={{kind='filesystem',root={:?}}}\n", docs.path().to_str().unwrap()));
    (fixture, docs, workspace)
}

fn recipe(workspace: &str, explicit: bool) -> build::LocalPublicationRecipe {
    let mut request = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":workspace}});
    if explicit {
        request["sites"] = json!([site("one")]);
    }
    build::LocalPublicationRecipe::parse_json(request.to_string().as_bytes(), 1 << 20, 2).unwrap()
}

#[test]
fn imported_catalog_joins_explicit_and_default_selection_and_retains_independent_source_roots() {
    let (_fixture, docs, workspace) = configured();
    let workspace = format!("[build]\nsites=['site:one']\nlocal_drafts='include'\n{workspace}");
    for explicit in [false, true] {
        let resolved = recipe(&workspace, explicit)
            .resolve(Default::default())
            .unwrap();
        let mut inputs = build::acquire_publications(
            &workspace,
            &[key("one")],
            Default::default(),
            Default::default(),
        )
        .unwrap();
        assert!(
            !inputs
                .sources()
                .contains_key(&"source:docs".parse().unwrap())
        );
        resolved
            .acquire_documentation_catalogs(
                &mut inputs,
                build::PublicationSetAcquisitionLimits::default().sources,
                Default::default(),
            )
            .unwrap();
        let catalog = inputs.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .documentation_catalog()
            .unwrap();
        assert_eq!(catalog.entries().len(), 2);
        let guide = catalog
            .entries()
            .iter()
            .find(|entry| entry.page.route.as_str() == "library/guide")
            .unwrap();
        assert_eq!(guide.page.title.as_deref(), Some("Guide"));
        assert_eq!(
            catalog.canonical_url(guide).unwrap().as_deref(),
            Some("https://docs.example/v2/library/guide")
        );
        assert!(
            inputs
                .source_roots()
                .unwrap()
                .contains(&docs.path().to_owned())
        );
        let watch = gazette_application::preview::watch::InputIdentity::of(&inputs);
        assert_eq!(
            watch.documentation[&key("one")],
            *catalog.source_identities()
        );
        resolved
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |rendered| assert!(rendered.is_complete()),
            )
            .unwrap();
    }
}

#[test]
fn supplied_catalog_requires_current_exact_documentation_evidence() {
    let (_fixture, docs, workspace) = configured();
    let documentation = build::acquire_documentation(
        &workspace,
        &key("one"),
        Default::default(),
        build::PublicationSetAcquisitionLimits::default().sources,
    )
    .unwrap();
    let assembled = build::assemble_documentation(&documentation, Default::default()).unwrap();
    let catalog = assembled.catalog_capture().unwrap();
    assert!(
        recipe(&workspace, true)
            .with_documentation_catalogs(vec![catalog.clone(), catalog.clone()])
            .is_err()
    );
    let resolved = recipe(&workspace, true)
        .with_documentation_catalogs(vec![catalog.clone()])
        .unwrap()
        .resolve(Default::default())
        .unwrap();
    let mut inputs = build::acquire_publications(
        &workspace,
        &[key("one")],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    resolved
        .acquire_documentation_catalogs(
            &mut inputs,
            build::PublicationSetAcquisitionLimits::default().sources,
            Default::default(),
        )
        .unwrap();
    assert_eq!(
        inputs.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .documentation_catalog()
            .unwrap()
            .evidence_sha256(),
        catalog.evidence_sha256()
    );
    fs::write(
        docs.path().join("guide.md"),
        "# Guide\n\nChanged documentation words.\n",
    )
    .unwrap();
    let mut changed = build::acquire_publications(
        &workspace,
        &[key("one")],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    let error = resolved
        .acquire_documentation_catalogs(
            &mut changed,
            build::PublicationSetAcquisitionLimits::default().sources,
            Default::default(),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("source capture changed"),
        "{error}"
    );
    assert!(
        changed.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .documentation_catalog()
            .is_none()
    );
}

#[test]
fn unavailable_configured_documentation_is_not_an_empty_success() {
    let (_fixture, docs, workspace) = configured();
    fs::remove_file(docs.path().join("guide.md")).unwrap();
    let resolved = recipe(&workspace, true)
        .resolve(Default::default())
        .unwrap();
    let mut inputs = build::acquire_publications(
        &workspace,
        &[key("one")],
        Default::default(),
        Default::default(),
    )
    .unwrap();
    assert!(
        resolved
            .acquire_documentation_catalogs(
                &mut inputs,
                build::PublicationSetAcquisitionLimits::default().sources,
                Default::default()
            )
            .is_err()
    );
    assert!(
        inputs.sites()[&key("one")]
            .as_ref()
            .unwrap()
            .documentation_catalog()
            .is_none()
    );
}

fn generated() -> (Fixture, TempDir, TempDir, String) {
    let (fixture, docs, workspace) = configured();
    fs::write(
        docs.path().join("generate.sh"),
        b"/bin/cat \"$1\" > \"$2\"\n",
    )
    .unwrap();
    let program = fs::canonicalize("/bin/sh").unwrap();
    let executable = fs::read(&program).unwrap();
    let input = format!(
        "input={{kind='generated',source='source:docs',recipe={{program={:?},executable={{bytes={},sha256='{}'}},arguments=[{{kind='input',path='generate.sh'}},{{kind='input',path='guide.md'}},{{kind='output'}}],version_arguments=['-c',\"printf 'catalog-fixture/1\\n'\"],expected_version_sha256='{}',output={{kind='file',path='api.md'}},format='markdown'}}}}",
        program.to_str().unwrap(),
        executable.len(),
        source::Sha256Digest::of(&executable),
        source::Sha256Digest::of(b"catalog-fixture/1\n")
    );
    let workspace = workspace.replace("input={kind='artifact',source='source:docs',selection={kind='file',path='guide.md'},format='markdown'}", &input);
    (fixture, docs, tempdir().unwrap(), workspace)
}

fn generated_recipe(workspace: &str, jobs: &TempDir, allow: bool) -> build::LocalPublicationRecipe {
    let mut request = build::DocumentationGenerationRequest::new(jobs.path().to_owned());
    request.allow_execute = allow;
    recipe(workspace, true)
        .with_documentation_generation(Some(request))
        .unwrap()
}

#[test]
fn generated_catalog_requires_delivery_and_preserves_native_receipt_schema() {
    let (_fixture, _docs, jobs, workspace) = generated();
    let resolved = generated_recipe(&workspace, &jobs, true)
        .resolve(Default::default())
        .unwrap();
    assert!(
        resolved
            .with_rendered(
                Default::default(),
                Default::default(),
                Default::default(),
                |_| ()
            )
            .is_err()
    );
    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 0);
    let mut complete = false;
    resolved
        .with_rendered_documentation(
            Default::default(),
            Default::default(),
            Default::default(),
            |execution| match execution {
                build::DocumentationRenderExecution::Rendered { rendered, receipts } => {
                    assert!(rendered.is_complete());
                    assert_eq!(receipts.len(), 1);
                    assert!(!receipts[0].is_error());
                    let value = serde_json::to_value(&receipts[0]).unwrap();
                    let schema =
                        serde_json::to_value(schemars::schema_for!(build::DocumentationReceipt))
                            .unwrap();
                    assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
                    assert_eq!(value["is_error"], false);
                    let report: Value = serde_json::from_slice(receipts[0].bytes()).unwrap();
                    assert_eq!(report["jobs"].as_array().unwrap().len(), 1);
                    for name in ["input", "output"] {
                        assert!(
                            std::path::Path::new(report["jobs"][0][name].as_str().unwrap())
                                .is_dir()
                        );
                    }
                    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 2);
                    complete = true;
                }
                build::DocumentationRenderExecution::Refused { error, .. } => panic!("{error}"),
                build::DocumentationRenderExecution::Native { .. } => {
                    panic!("native operation should complete")
                }
            },
        )
        .unwrap();
    assert!(complete);
}

#[test]
fn generated_catalog_permission_and_source_separation_fail_before_staging() {
    let (fixture, _docs, jobs, workspace) = generated();
    let mut refused = false;
    generated_recipe(&workspace, &jobs, false)
        .resolve(Default::default())
        .unwrap()
        .with_rendered_documentation(
            Default::default(),
            Default::default(),
            Default::default(),
            |execution| match execution {
                build::DocumentationRenderExecution::Native {
                    execution: build::documentation_output::Execution::Delivered { response, .. },
                    ..
                } => {
                    assert!(response.is_error);
                    refused = true;
                }
                _ => panic!("native permission refusal retains its original delivery"),
            },
        )
        .unwrap();
    assert!(refused);
    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 0);
    let mut selection =
        build::DocumentationGenerationRequest::new(fixture.directory.path().to_owned());
    selection.allow_execute = true;
    let recipe = recipe(&workspace, true)
        .with_documentation_generation(Some(selection))
        .unwrap();
    let mut separated = false;
    recipe
        .resolve(Default::default())
        .unwrap()
        .with_rendered_documentation(
            Default::default(),
            Default::default(),
            Default::default(),
            |execution| {
                if let build::DocumentationRenderExecution::Refused {
                    error: build::DocumentationPublicationFailure::Separation(_),
                    receipts,
                } = execution
                {
                    assert!(receipts.is_empty());
                    separated = true;
                } else {
                    panic!("stage parent inside a publication source must be refused");
                }
            },
        )
        .unwrap();
    assert!(separated);
}

#[test]
fn generated_catalog_selection_is_bounded_and_background_preview_refuses_before_effects() {
    let (_fixture, _docs, jobs, workspace) = generated();
    let mut request = json!({"format":"gazette-local-publication-set/1", "workspace":{"kind":"supplied","source":workspace}, "sites":[site("one")], "documentation_generation":{"parent":jobs.path()}});
    let parse = |value: &Value| {
        build::LocalPublicationRecipe::parse_json(value.to_string().as_bytes(), 1 << 20, 2)
    };
    let schema = build::LocalPublicationRecipe::input_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(validator.is_valid(&request));
    for (field, value) in [
        ("jobs", json!(0)),
        ("jobs", json!(129)),
        ("timeout_ms", json!(300001)),
        ("response_bytes", json!(0)),
        ("response_bytes", json!(16777217)),
        ("program", json!("/invented/generator")),
    ] {
        let mut invalid = request.clone();
        invalid["documentation_generation"][field] = value;
        assert!(!validator.is_valid(&invalid), "{invalid}");
        assert!(parse(&invalid).is_err());
    }
    request["documentation_generation"]["allow_execute"] = true.into();
    let selected = parse(&request).unwrap();
    let addresses = BTreeMap::from([(key("one"), "127.0.0.1:0".parse().unwrap())]);
    let options = gazette_sdk::preview::runner::Options {
        listeners: gazette_sdk::preview::Options {
            allow_listeners: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let plan = gazette_sdk::preview::runner::plan(&selected, &addresses, &options).unwrap();
    assert!(plan.effects.derived_state_writes && plan.effects.output_writes);
    assert!(plan.effects.unconfined_process_execution);
    let refused = gazette_sdk::preview::session::Session::start(
        selected,
        addresses,
        options,
        Default::default(),
    )
    .err()
    .expect("background custody is refused before a worker starts");
    assert_eq!(refused.code(), "preview_documentation_custody_unavailable");
    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 0);
}

#[test]
fn documentation_watch_reuses_native_outputs_and_replays_unchanged_failures() {
    use gazette_sdk::preview::{runner, watch};
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };
    let (fixture, docs, jobs, workspace) = generated();
    let selected = generated_recipe(&workspace, &jobs, true);
    let addresses = BTreeMap::from([(key("one"), "127.0.0.1:0".parse().unwrap())]);
    let options = runner::Options {
        mode: runner::Mode::Development {
            interval: Duration::from_millis(100),
        },
        listeners: gazette_sdk::preview::Options {
            allow_listeners: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let start = Instant::now();
    let phase = Cell::new(0usize);
    let replayed = Cell::new(0usize);
    let page = fixture.directory.path().join("one/page.md");
    runner::run_with_documentation_delivery(
        &selected,
        addresses,
        options,
        |event| {
            match event {
                runner::Event::Ready { state } => {
                    assert_eq!(state.documentation.len(), 1);
                    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 2);
                    fs::write(
                        &page,
                        format!(
                            "{}\nUnrelated authored edit.\n",
                            fs::read_to_string(&page).unwrap()
                        ),
                    )
                    .unwrap();
                    phase.set(1);
                }
                runner::Event::Changed { state } if phase.get() == 1 => {
                    assert_eq!(state.documentation.len(), 1);
                    assert_eq!(
                        fs::read_dir(jobs.path()).unwrap().count(),
                        2,
                        "unchanged docs must not regenerate"
                    );
                    fs::write(
                        docs.path().join("guide.md"),
                        "# Updated guide\n\nChanged documentation.\n",
                    )
                    .unwrap();
                    phase.set(2);
                }
                runner::Event::Changed { state } if phase.get() == 2 => {
                    assert_eq!(state.documentation.len(), 1);
                    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 4);
                    fs::write(docs.path().join("generate.sh"), "exit 1\n").unwrap();
                    phase.set(3);
                }
                runner::Event::RebuildFailed { documentation, .. } if phase.get() == 3 => {
                    assert!(
                        documentation
                            .iter()
                            .any(build::DocumentationReceipt::is_error)
                    );
                    assert_eq!(fs::read_dir(jobs.path()).unwrap().count(), 6);
                    fs::write(
                        &page,
                        format!(
                            "{}\nAnother authored edit.\n",
                            fs::read_to_string(&page).unwrap()
                        ),
                    )
                    .unwrap();
                    phase.set(4);
                }
                runner::Event::RebuildFailed { documentation, .. } if phase.get() == 4 => {
                    assert!(
                        documentation
                            .iter()
                            .any(build::DocumentationReceipt::is_error)
                    );
                    assert_eq!(
                        fs::read_dir(jobs.path()).unwrap().count(),
                        6,
                        "same failed docs input must not retry on authored edits"
                    );
                    phase.set(5);
                }
                _ => {}
            }
            Ok(())
        },
        || {
            Ok(
                if phase.get() == 5 || start.elapsed() > Duration::from_secs(12) {
                    watch::Control::Stop
                } else {
                    watch::Control::Continue
                },
            )
        },
        |delivery| {
            if matches!(
                delivery,
                build::DocumentationDelivery::ReplayedFailure { .. }
            ) {
                replayed.set(replayed.get() + 1);
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        phase.get(),
        5,
        "watch should complete all bounded generation cases"
    );
    assert_eq!(replayed.get(), 1);
}
